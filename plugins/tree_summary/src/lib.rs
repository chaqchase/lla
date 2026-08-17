use lla_plugin_sdk::{
    interface::proto, manifest_action_infos, response, value, ActionArguments, ActionArgumentsExt,
    ActionError, DecoratedEntryExt, Plugin,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TreeStats {
    files: u64,
    directories: u64,
    bytes: u64,
}

#[derive(Default)]
struct TreeSummaryPlugin;

impl TreeSummaryPlugin {
    fn summarize(path: &Path) -> Option<TreeStats> {
        if !path.is_dir() {
            return None;
        }
        let mut stats = TreeStats::default();
        for entry in WalkDir::new(path)
            .follow_links(false)
            .min_depth(1)
            .into_iter()
            .filter_map(Result::ok)
        {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_file() {
                stats.files += 1;
                stats.bytes = stats.bytes.saturating_add(metadata.len());
            } else if metadata.is_dir() {
                stats.directories += 1;
            }
        }
        Some(stats)
    }

    fn summarize_many(paths: &[PathBuf]) -> HashMap<PathBuf, TreeStats> {
        let mut roots = paths.to_vec();
        roots.sort_by_key(|path| path.components().count());
        let mut scan_roots = Vec::<PathBuf>::new();
        for path in roots {
            if !scan_roots.iter().any(|root| path.starts_with(root)) {
                scan_roots.push(path);
            }
        }

        let mut summaries = HashMap::new();
        for root in scan_roots {
            summaries.extend(summarize_tree(&root));
        }
        summaries
    }

    fn decorate_with(
        mut entry: proto::DecoratedEntry,
        stats: Option<TreeStats>,
    ) -> proto::DecoratedEntry {
        if let Some(stats) = stats {
            entry.insert_field(
                "tree_file_count",
                value::integer(stats.files as i64),
                stats.files.to_string(),
            );
            entry.insert_field(
                "tree_directory_count",
                value::integer(stats.directories as i64),
                stats.directories.to_string(),
            );
            entry.insert_field(
                "tree_total_bytes",
                value::bytes(stats.bytes),
                stats.bytes.to_string(),
            );
        }
        entry
    }

    fn decorate(entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        let stats = Self::summarize(Path::new(&entry.path));
        Self::decorate_with(entry, stats)
    }

    fn inspect(path: PathBuf) -> Result<proto::TypedValue, ActionError> {
        let stats = Self::summarize(&path).ok_or_else(|| {
            ActionError::invalid_argument(
                "path",
                format!("path is not a readable directory: {}", path.display()),
            )
        })?;
        Ok(value::object([
            ("path".to_string(), value::path(path.to_string_lossy())),
            ("files".to_string(), value::integer(stats.files as i64)),
            (
                "directories".to_string(),
                value::integer(stats.directories as i64),
            ),
            ("bytes".to_string(), value::bytes(stats.bytes)),
        ]))
    }
}

impl Plugin for TreeSummaryPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        Self::decorate(entry)
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        let directories = entries
            .iter()
            .filter(|entry| Path::new(&entry.path).is_dir())
            .map(|entry| PathBuf::from(&entry.path))
            .collect::<Vec<_>>();
        let summaries = Self::summarize_many(&directories);
        entries
            .into_iter()
            .map(|entry| {
                let stats = summaries.get(Path::new(&entry.path)).copied();
                Self::decorate_with(entry, stats)
            })
            .collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        if format != "tree" {
            return None;
        }
        let files = entry.custom_fields.get("tree_file_count")?;
        let directories = entry.custom_fields.get("tree_directory_count")?;
        let bytes = entry.custom_fields.get("tree_total_bytes")?.parse().ok()?;
        Some(format!(
            "[{files} files · {directories} dirs · {}]",
            format_bytes(bytes)
        ))
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

fn summarize_tree(root: &Path) -> HashMap<PathBuf, TreeStats> {
    let mut summaries = HashMap::from([(root.to_path_buf(), TreeStats::default())]);
    for entry in WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if entry.file_type().is_dir() {
            summaries.entry(path.to_path_buf()).or_default();
            update_ancestors(path.parent(), root, &mut summaries, |stats| {
                stats.directories += 1;
            });
        } else if entry.file_type().is_file() {
            let bytes = entry.metadata().map_or(0, |metadata| metadata.len());
            update_ancestors(path.parent(), root, &mut summaries, |stats| {
                stats.files += 1;
                stats.bytes = stats.bytes.saturating_add(bytes);
            });
        }
    }
    summaries
}

fn update_ancestors(
    mut current: Option<&Path>,
    root: &Path,
    summaries: &mut HashMap<PathBuf, TreeStats>,
    mut update: impl FnMut(&mut TreeStats),
) {
    while let Some(directory) = current {
        if !directory.starts_with(root) {
            break;
        }
        update(summaries.entry(directory.to_path_buf()).or_default());
        if directory == root {
            break;
        }
        current = directory.parent();
    }
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

lla_plugin_sdk::export_plugin!(TreeSummaryPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_nested_directories_without_counting_the_root() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(root.path().join("one.txt"), b"1234").unwrap();
        std::fs::write(nested.join("two.txt"), b"123456").unwrap();

        let stats = TreeSummaryPlugin::summarize(root.path()).unwrap();
        assert_eq!(stats.files, 2);
        assert_eq!(stats.directories, 1);
        assert_eq!(stats.bytes, 10);
    }

    #[test]
    fn ignores_files() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("file.txt");
        std::fs::write(&file, "content").unwrap();
        assert!(TreeSummaryPlugin::summarize(&file).is_none());
    }

    #[test]
    fn batch_summary_matches_individual_directory_summaries() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(root.path().join("one.txt"), b"1234").unwrap();
        std::fs::write(nested.join("two.txt"), b"123456").unwrap();

        let summaries =
            TreeSummaryPlugin::summarize_many(&[root.path().to_path_buf(), nested.clone()]);
        assert_eq!(
            summaries.get(root.path()),
            TreeSummaryPlugin::summarize(root.path()).as_ref()
        );
        assert_eq!(
            summaries.get(&nested),
            TreeSummaryPlugin::summarize(&nested).as_ref()
        );
    }
}
