use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, ActionArguments, DecoratedEntryExt, Plugin};
use lla_plugin_utils::{
    decode_decorated_entry, map_decorated_entry, run_cli_action, ActionRegistry, DecoratedEntry,
};
use parking_lot::RwLock;
use std::{fs, path::Path};
use walkdir::WalkDir;

lazy_static! {
    static ref ACTIONS: RwLock<ActionRegistry> = RwLock::new({
        let mut actions = ActionRegistry::new();
        lla_plugin_utils::define_action!(
            actions,
            "audit",
            "audit <path> [--recursive]",
            "Audit a file or directory and print security findings",
            [
                "lla plugin run security_audit audit -- .",
                "lla plugin run security_audit audit -- . -- --recursive"
            ],
            SecurityAuditPlugin::audit_action
        );
        lla_plugin_utils::define_action!(
            actions,
            "help",
            "help",
            "Show security audit usage and risk rules",
            ["lla plugin run security_audit help"],
            |_| SecurityAuditPlugin::help_action()
        );
        actions
    });
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AuditResult {
    score: u32,
    findings: Vec<String>,
    suspicious_symlink: bool,
    secret_exposed: bool,
}

impl AuditResult {
    fn risk(&self) -> &'static str {
        match self.score {
            0 => "clean",
            1..=29 => "low",
            30..=59 => "medium",
            60..=89 => "high",
            _ => "critical",
        }
    }
}

pub struct SecurityAuditPlugin;

impl SecurityAuditPlugin {
    fn inspect(
        path: &Path,
        permissions: u32,
        is_file: bool,
        is_dir: bool,
        is_symlink: bool,
    ) -> AuditResult {
        let mut result = AuditResult::default();

        if permissions & 0o002 != 0 {
            if is_dir && permissions & 0o1000 != 0 {
                result.score += 20;
                result
                    .findings
                    .push("world-writable directory protected by sticky bit".to_string());
            } else {
                result.score += 65;
                result.findings.push("world-writable entry".to_string());
            }
        }
        if is_file && permissions & 0o4000 != 0 {
            result.score += 80;
            result
                .findings
                .push("SUID executable bit is set".to_string());
        }
        if is_file && permissions & 0o2000 != 0 {
            result.score += 65;
            result
                .findings
                .push("SGID executable bit is set".to_string());
        }
        if is_file && permissions & 0o111 != 0 && permissions & 0o022 != 0 {
            result.score += 50;
            result
                .findings
                .push("executable is writable by group or others".to_string());
        }
        if is_dir && permissions & 0o020 != 0 && permissions & 0o1000 == 0 {
            result.score += 30;
            result
                .findings
                .push("group-writable directory lacks a sticky bit".to_string());
        }

        if is_symlink {
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            match fs::read_link(path) {
                Err(_) => {
                    result.score += 50;
                    result.suspicious_symlink = true;
                    result
                        .findings
                        .push("unreadable symlink target".to_string());
                }
                Ok(target) => {
                    let resolved = if target.is_absolute() {
                        target.clone()
                    } else {
                        parent.join(&target)
                    };
                    if !resolved.exists() {
                        result.score += 45;
                        result.suspicious_symlink = true;
                        result.findings.push("dangling symlink".to_string());
                    }
                    let outside_parent = parent
                        .canonicalize()
                        .ok()
                        .zip(resolved.canonicalize().ok())
                        .is_some_and(|(parent, resolved)| !resolved.starts_with(parent));
                    if target.is_absolute() || outside_parent {
                        result.score += 35;
                        result.suspicious_symlink = true;
                        result
                            .findings
                            .push("symlink targets outside its containing directory".to_string());
                    }
                }
            }
        }

        if is_secret_name(path) && permissions & 0o077 != 0 {
            result.score += 75;
            result.secret_exposed = true;
            result
                .findings
                .push("secret-like file is readable or writable by group/others".to_string());
        }

        result.score = result.score.min(100);
        result
    }

    fn decorate(mut entry: DecoratedEntry) -> DecoratedEntry {
        let is_symlink = fs::symlink_metadata(&entry.path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(entry.metadata.is_symlink);
        let result = Self::inspect(
            &entry.path,
            entry.metadata.permissions,
            entry.metadata.is_file,
            entry.metadata.is_dir,
            is_symlink,
        );
        entry
            .custom_fields
            .insert("security_risk".to_string(), result.risk().to_string());
        entry
            .custom_fields
            .insert("security_score".to_string(), result.score.to_string());
        entry.custom_fields.insert(
            "security_findings".to_string(),
            if result.findings.is_empty() {
                "none".to_string()
            } else {
                result.findings.join("; ")
            },
        );
        entry.custom_fields.insert(
            "suspicious_symlink".to_string(),
            result.suspicious_symlink.to_string(),
        );
        entry.custom_fields.insert(
            "secret_exposed".to_string(),
            result.secret_exposed.to_string(),
        );
        entry
    }

    fn format(entry: &DecoratedEntry, format: &str) -> Option<String> {
        let risk = entry.custom_fields.get("security_risk")?;
        let score = entry.custom_fields.get("security_score")?;
        let findings = entry.custom_fields.get("security_findings")?;
        match format {
            "default" => Some(format!("[security: {risk} ({score})]")),
            "long" => Some(format!(
                "Security risk: {risk} ({score}/100)\nFindings: {findings}"
            )),
            _ => None,
        }
    }

    fn audit_action(args: &[String]) -> Result<(), String> {
        let path = args
            .first()
            .map(Path::new)
            .ok_or_else(|| "Usage: audit <path> [--recursive]".to_string())?;
        let recursive = args.iter().any(|arg| arg == "--recursive");
        if !path.exists() && fs::symlink_metadata(path).is_err() {
            return Err(format!("Path does not exist: {}", path.display()));
        }
        let walker = WalkDir::new(path)
            .follow_links(false)
            .max_depth(if recursive { usize::MAX } else { 1 });
        let mut scanned = 0usize;
        let mut risky = 0usize;
        for item in walker {
            let item = item.map_err(|error| format!("Audit traversal failed: {error}"))?;
            let metadata = fs::symlink_metadata(item.path()).map_err(|error| {
                format!("Failed to inspect '{}': {error}", item.path().display())
            })?;
            #[cfg(unix)]
            let permissions = {
                use std::os::unix::fs::MetadataExt;
                metadata.mode()
            };
            #[cfg(not(unix))]
            let permissions = 0;
            let result = Self::inspect(
                item.path(),
                permissions,
                metadata.is_file(),
                metadata.is_dir(),
                metadata.file_type().is_symlink(),
            );
            scanned += 1;
            if result.score > 0 {
                risky += 1;
                println!(
                    "{}  {:>3}/100  {}",
                    result.risk(),
                    result.score,
                    item.path().display()
                );
                for finding in result.findings {
                    println!("  - {finding}");
                }
            }
        }
        println!("Audited {scanned} entries; {risky} had findings.");
        Ok(())
    }

    fn help_action() -> Result<(), String> {
        println!(
            "security_audit\n\n  audit <path> [--recursive]  Scan permissions, SUID/SGID, symlinks, and exposed secret files\n  help                        Show this help\n\nRisk scores are emitted as typed fields for JSON, sorting, and filtering."
        );
        Ok(())
    }
}

fn is_secret_name(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        ".env"
            | ".env.local"
            | ".env.production"
            | ".npmrc"
            | ".pypirc"
            | ".netrc"
            | "id_rsa"
            | "id_dsa"
            | "id_ed25519"
            | "credentials"
            | "credentials.json"
            | "service-account.json"
    ) || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
}

impl Plugin for SecurityAuditPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        promote_v3_fields(map_decorated_entry(entry, Self::decorate))
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        entries
            .into_iter()
            .map(|entry| promote_v3_fields(map_decorated_entry(entry, Self::decorate)))
            .collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| Self::format(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        run_cli_action(
            &action,
            arguments,
            include_str!("../plugin.toml"),
            |arguments| ACTIONS.read().handle(&action, arguments),
        )
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        lla_plugin_utils::manifest_action_infos(include_str!("../plugin.toml"))
    }
}

fn promote_v3_fields(mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
    entry.promote_string_field("security_risk");
    entry.promote_integer_field("security_score");
    entry.promote_string_field("security_findings");
    entry.promote_boolean_field("suspicious_symlink");
    entry.promote_boolean_field("secret_exposed");
    entry
}

lla_plugin_sdk::export_plugin!(SecurityAuditPlugin);

impl Default for SecurityAuditPlugin {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_world_writable_privileged_and_exposed_secret_files() {
        let result = SecurityAuditPlugin::inspect(Path::new(".env"), 0o106677, true, false, false);
        assert_eq!(result.risk(), "critical");
        assert!(result.secret_exposed);
        assert!(result
            .findings
            .iter()
            .any(|finding| finding.contains("SUID")));
        assert!(result
            .findings
            .iter()
            .any(|finding| finding.contains("SGID")));
        assert!(result
            .findings
            .iter()
            .any(|finding| finding.contains("world-writable")));
        assert!(result
            .findings
            .iter()
            .any(|finding| finding.contains("executable is writable")));
    }

    #[cfg(unix)]
    #[test]
    fn detects_dangling_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let link = root.path().join("dangling");
        std::os::unix::fs::symlink("missing", &link).unwrap();
        let result = SecurityAuditPlugin::inspect(&link, 0o777, false, false, true);
        assert!(result.suspicious_symlink);
        assert!(result
            .findings
            .iter()
            .any(|finding| finding.contains("dangling")));
    }

    #[cfg(unix)]
    #[test]
    fn recursive_audit_action_accepts_real_unsafe_fixtures() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let secret = root.path().join(".env");
        fs::write(&secret, "TOKEN=test").unwrap();
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o666)).unwrap();
        SecurityAuditPlugin::audit_action(&[
            root.path().to_string_lossy().to_string(),
            "--recursive".to_string(),
        ])
        .unwrap();
    }
}
