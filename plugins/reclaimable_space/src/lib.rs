use lla_plugin_sdk::{
    interface::proto, manifest_action_infos, response, value, ActionArguments, ActionArgumentsExt,
    ActionError, DecoratedEntryExt, Plugin,
};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReclaimInfo {
    bytes: u64,
    reason: &'static str,
    confidence: &'static str,
}

#[derive(Default)]
struct ReclaimableSpacePlugin;

impl ReclaimableSpacePlugin {
    fn classify(path: &Path) -> Option<ReclaimInfo> {
        let metadata = std::fs::symlink_metadata(path).ok()?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        let (reason, confidence) = if metadata.is_dir() {
            match name.as_str() {
                "node_modules" => ("dependency cache", "high"),
                "target" | "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache"
                | ".cache" | ".parcel-cache" | ".turbo" => ("generated cache", "high"),
                ".next" | "coverage" | "dist" | "build" => ("build output", "medium"),
                _ => return None,
            }
        } else if metadata.is_file() {
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if matches!(name.as_str(), ".ds_store" | "thumbs.db") {
                ("system metadata", "high")
            } else if name.ends_with('~')
                || matches!(
                    extension.as_str(),
                    "tmp" | "temp" | "swp" | "swo" | "bak" | "old"
                )
            {
                ("temporary file", "high")
            } else if matches!(extension.as_str(), "pyc" | "class" | "o" | "obj") {
                ("generated artifact", "high")
            } else if extension == "log" {
                ("log file", "medium")
            } else {
                return None;
            }
        } else {
            return None;
        };

        Some(ReclaimInfo {
            bytes: path_size(path, &metadata),
            reason,
            confidence,
        })
    }

    fn decorate(mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        if let Some(info) = Self::classify(Path::new(&entry.path)) {
            entry.insert_field(
                "reclaimable_bytes",
                value::bytes(info.bytes),
                info.bytes.to_string(),
            );
            entry.insert_field("reclaim_reason", value::string(info.reason), info.reason);
            entry.insert_field(
                "reclaim_confidence",
                value::string(info.confidence),
                info.confidence,
            );
        }
        entry
    }

    fn inspect(path: PathBuf) -> Result<proto::TypedValue, ActionError> {
        if !path.exists() {
            return Err(ActionError::invalid_argument(
                "path",
                format!("path does not exist: {}", path.display()),
            ));
        }
        let info = Self::classify(&path);
        Ok(value::object([
            ("path".to_string(), value::path(path.to_string_lossy())),
            ("reclaimable".to_string(), value::boolean(info.is_some())),
            (
                "reclaimable_bytes".to_string(),
                value::bytes(info.as_ref().map_or(0, |info| info.bytes)),
            ),
            (
                "reason".to_string(),
                value::string(info.as_ref().map_or("none", |info| info.reason)),
            ),
            (
                "confidence".to_string(),
                value::string(info.as_ref().map_or("none", |info| info.confidence)),
            ),
        ]))
    }
}

impl Plugin for ReclaimableSpacePlugin {
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
        if format != "sizemap" {
            return None;
        }
        let bytes = entry.custom_fields.get("reclaimable_bytes")?.parse().ok()?;
        let reason = entry.custom_fields.get("reclaim_reason")?;
        Some(format!("[reclaim {} · {reason}]", format_bytes(bytes)))
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

fn path_size(path: &Path, metadata: &std::fs::Metadata) -> u64 {
    if metadata.is_file() {
        return metadata.len();
    }
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum()
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

lla_plugin_sdk::export_plugin!(ReclaimableSpacePlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_generated_directories_and_sums_their_files() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("artifact.o"), vec![0u8; 2048]).unwrap();

        let info = ReclaimableSpacePlugin::classify(&target).unwrap();
        assert_eq!(info.bytes, 2048);
        assert_eq!(info.reason, "generated cache");
        assert_eq!(info.confidence, "high");
    }

    #[test]
    fn leaves_regular_user_files_unclassified() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("notes.md");
        std::fs::write(&path, "important").unwrap();
        assert!(ReclaimableSpacePlugin::classify(&path).is_none());
    }
}
