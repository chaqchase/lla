use lla_plugin_sdk::{
    interface::proto, manifest_action_infos, response, value, ActionArguments, ActionArgumentsExt,
    ActionError, DecoratedEntryExt, Plugin,
};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct GitStats {
    churn: u64,
    commits: u64,
    dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RiskInfo {
    score: u8,
    level: &'static str,
    churn: u64,
    commits: u64,
    reasons: Vec<String>,
}

#[derive(Default)]
struct ChangeRiskPlugin;

impl ChangeRiskPlugin {
    fn analyze(path: &Path) -> Option<RiskInfo> {
        if !path.is_file() {
            return None;
        }
        let metadata = path.metadata().ok()?;
        let source = read_source(path, metadata.len());
        let (lines, structural_markers) = source.as_deref().map(source_metrics).unwrap_or_default();
        let git = git_stats(path).unwrap_or_default();
        Some(score_risk(
            path,
            metadata.len(),
            lines,
            structural_markers,
            git,
        ))
    }

    fn decorate(mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        if let Some(info) = Self::analyze(Path::new(&entry.path)) {
            entry.insert_field("change_risk_level", value::string(info.level), info.level);
            entry.insert_field(
                "change_risk_score",
                value::integer(info.score as i64),
                info.score.to_string(),
            );
            entry.insert_field(
                "change_churn",
                value::integer(info.churn as i64),
                info.churn.to_string(),
            );
            entry.insert_field(
                "change_commits",
                value::integer(info.commits as i64),
                info.commits.to_string(),
            );
            entry.insert_field(
                "change_risk_reasons",
                value::string(info.reasons.join("; ")),
                info.reasons.join("; "),
            );
        }
        entry
    }

    fn inspect(path: PathBuf) -> Result<proto::TypedValue, ActionError> {
        let info = Self::analyze(&path).ok_or_else(|| {
            ActionError::invalid_argument(
                "path",
                format!("path is not a readable file: {}", path.display()),
            )
        })?;
        Ok(value::object([
            ("path".to_string(), value::path(path.to_string_lossy())),
            ("level".to_string(), value::string(info.level)),
            ("score".to_string(), value::integer(info.score as i64)),
            ("churn".to_string(), value::integer(info.churn as i64)),
            ("commits".to_string(), value::integer(info.commits as i64)),
            (
                "reasons".to_string(),
                value::list(info.reasons.into_iter().map(value::string)),
            ),
        ]))
    }
}

impl Plugin for ChangeRiskPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        Self::decorate(entry)
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        entries.into_iter().map(Self::decorate).collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        if format != "git" {
            return None;
        }
        let level = entry.custom_fields.get("change_risk_level")?;
        let score = entry.custom_fields.get("change_risk_score")?;
        let churn = entry.custom_fields.get("change_churn")?;
        Some(format!("[risk:{level} {score} · churn:{churn}]"))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        if action != "inspect" {
            return response::error(ActionError::new(
                "unknown-action",
                format!("plugin does not implement action '{action}'"),
            ));
        }
        let result = arguments
            .path("path")
            .and_then(|path| {
                path.ok_or_else(|| ActionError::invalid_argument("path", "path is required"))
            })
            .and_then(Self::inspect);
        match result {
            Ok(value) => response::value(value),
            Err(error) => response::error(error),
        }
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        manifest_action_infos(include_str!("../plugin.toml"))
    }
}

fn read_source(path: &Path, size: u64) -> Option<String> {
    if size > MAX_SOURCE_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(size as usize);
    File::open(path).ok()?.read_to_end(&mut bytes).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn source_metrics(source: &str) -> (u64, u64) {
    let lines = source.lines().count() as u64;
    let structural_markers = source
        .lines()
        .map(str::trim)
        .map(|line| {
            [
                "if ", "else", "match ", "for ", "while ", "&&", "||", "catch", "except",
            ]
            .into_iter()
            .filter(|marker| line.contains(marker))
            .count() as u64
        })
        .sum();
    (lines, structural_markers)
}

fn score_risk(
    path: &Path,
    bytes: u64,
    lines: u64,
    structural_markers: u64,
    git: GitStats,
) -> RiskInfo {
    let mut score = 0u64;
    let mut reasons = Vec::new();

    if bytes > MAX_SOURCE_BYTES || lines > 1_000 {
        score += 25;
        reasons.push("very large file".to_string());
    } else if lines > 400 {
        score += 15;
        reasons.push("large file".to_string());
    } else if lines > 150 {
        score += 8;
        reasons.push("moderate file size".to_string());
    }

    if structural_markers > 80 {
        score += 20;
        reasons.push("high structural complexity".to_string());
    } else if structural_markers > 30 {
        score += 10;
        reasons.push("moderate structural complexity".to_string());
    }

    if git.churn > 2_000 {
        score += 30;
        reasons.push("very high Git churn".to_string());
    } else if git.churn > 500 {
        score += 20;
        reasons.push("high Git churn".to_string());
    } else if git.churn > 100 {
        score += 10;
        reasons.push("moderate Git churn".to_string());
    }

    if git.commits > 30 {
        score += 15;
        reasons.push("frequently changed".to_string());
    } else if git.commits > 10 {
        score += 8;
        reasons.push("regularly changed".to_string());
    }

    if git.dirty {
        score += 15;
        reasons.push("currently modified".to_string());
    }
    if is_core_configuration(path) {
        score += 10;
        reasons.push("core configuration".to_string());
    }
    if is_test_path(path) {
        score = score.saturating_sub(15);
        reasons.push("test file".to_string());
    }
    if is_generated_path(path) {
        score = score.min(10);
        reasons.clear();
        reasons.push("generated artifact".to_string());
    }
    if reasons.is_empty() {
        reasons.push("no elevated indicators".to_string());
    }

    let score = score.min(100) as u8;
    let level = match score {
        0..=24 => "low",
        25..=49 => "medium",
        50..=74 => "high",
        _ => "critical",
    };
    RiskInfo {
        score,
        level,
        churn: git.churn,
        commits: git.commits,
        reasons,
    }
}

fn git_stats(path: &Path) -> Option<GitStats> {
    let root = repository_root(path)?;
    let relative = path.strip_prefix(&root).ok()?;
    let log = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["log", "--format=commit", "--numstat", "--"])
        .arg(relative)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !log.status.success() {
        return None;
    }
    let mut stats = parse_git_stats(&String::from_utf8_lossy(&log.stdout));
    let status = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["status", "--porcelain", "--"])
        .arg(relative)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    stats.dirty = status.status.success() && !status.stdout.is_empty();
    Some(stats)
}

fn parse_git_stats(output: &str) -> GitStats {
    let mut stats = GitStats::default();
    for line in output.lines() {
        if line == "commit" {
            stats.commits += 1;
            continue;
        }
        let mut fields = line.split('\t');
        let additions = fields.next().and_then(|value| value.parse::<u64>().ok());
        let deletions = fields.next().and_then(|value| value.parse::<u64>().ok());
        if let (Some(additions), Some(deletions)) = (additions, deletions) {
            stats.churn = stats
                .churn
                .saturating_add(additions.saturating_add(deletions));
        }
    }
    stats
}

fn repository_root(path: &Path) -> Option<PathBuf> {
    let start = path.parent()?;
    let output = Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string()))
}

fn lower_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_test_path(path: &Path) -> bool {
    let name = lower_name(path);
    name.contains("test")
        || name.contains("spec")
        || path
            .components()
            .any(|component| matches!(component.as_os_str().to_str(), Some("tests" | "test")))
}

fn is_generated_path(path: &Path) -> bool {
    let name = lower_name(path);
    name.ends_with(".min.js")
        || name.ends_with(".min.css")
        || matches!(
            name.as_str(),
            "cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock"
        )
        || path.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("dist" | "build" | "target" | "generated")
            )
        })
}

fn is_core_configuration(path: &Path) -> bool {
    matches!(
        lower_name(path).as_str(),
        "cargo.toml"
            | "package.json"
            | "dockerfile"
            | "makefile"
            | "build.rs"
            | "pyproject.toml"
            | "go.mod"
    )
}

lla_plugin_sdk::export_plugin!(ChangeRiskPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_churn_and_commit_counts() {
        let stats = parse_git_stats("commit\n10\t5\tsrc/lib.rs\ncommit\n3\t2\tsrc/lib.rs\n");
        assert_eq!(stats.commits, 2);
        assert_eq!(stats.churn, 20);
    }

    #[test]
    fn generated_files_remain_low_risk() {
        let info = score_risk(
            Path::new("dist/app.min.js"),
            3_000_000,
            5_000,
            500,
            GitStats {
                churn: 20_000,
                commits: 100,
                dirty: true,
            },
        );
        assert_eq!(info.level, "low");
        assert_eq!(info.reasons, vec!["generated artifact"]);
    }

    #[test]
    fn tests_reduce_the_score() {
        let source = score_risk(
            Path::new("src/service.rs"),
            2_000,
            500,
            40,
            GitStats::default(),
        );
        let test = score_risk(
            Path::new("tests/service_test.rs"),
            2_000,
            500,
            40,
            GitStats::default(),
        );
        assert!(test.score < source.score);
    }
}
