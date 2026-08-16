use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, value, ActionArguments, DecoratedEntryExt, Plugin};
use lla_plugin_utils::DecoratedEntry;
use lla_plugin_utils::{
    config::PluginConfig,
    decode_decorated_entry, run_cli_action,
    ui::{
        components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
        format_size, TextBlock,
    },
    ActionRegistry, BasePlugin, ConfigurablePlugin,
};
use parking_lot::RwLock;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    time::SystemTime,
};
use walkdir::WalkDir;

type DirStats = (usize, usize, u64);
type CacheEntry = (SystemTime, DirStats);
type DirCache = HashMap<String, CacheEntry>;

lazy_static! {
    static ref CACHE: RwLock<DirCache> = RwLock::new(HashMap::new());
    static ref SPINNER: RwLock<Spinner> = RwLock::new(Spinner::new());
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "clear-cache",
            "clear-cache",
            "Clear the directory analysis cache",
            ["lla plugin run dirs_meta clear-cache"],
            |_| {
                let spinner = SPINNER.write();
                spinner.set_status("Clearing cache...".to_string());
                CACHE.write().clear();
                spinner.finish();
                drop(spinner);
                println!(
                    "{}",
                    BoxComponent::new(
                        TextBlock::new("Cache cleared successfully")
                            .color("bright_green")
                            .build()
                    )
                    .style(BoxStyle::Minimal)
                    .padding(1)
                    .render()
                );
                Ok(())
            }
        );
        lla_plugin_utils::define_action!(
            registry,
            "stats",
            "stats <path>",
            "Show detailed statistics for a directory",
            ["lla plugin run dirs_meta stats -- \"/path/to/dir\""],
            DirsPlugin::stats_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            ["lla plugin run dirs_meta help"],
            |_| {
                let mut help = HelpFormatter::new("Directory Metadata Plugin".to_string());
                help.add_section("Description".to_string())
                    .add_command(
                        "".to_string(),
                        "Analyzes directories to provide information about their contents, including file count, subdirectory count, and total size.".to_string(),
                        vec![],
                    );

                help.add_section("Actions".to_string())
                    .add_command(
                        "clear-cache".to_string(),
                        "Clear the directory analysis cache".to_string(),
                        vec!["lla plugin run dirs_meta clear-cache".to_string()],
                    )
                    .add_command(
                        "stats".to_string(),
                        "Show detailed statistics for a directory".to_string(),
                        vec!["lla plugin run dirs_meta stats -- \"/path/to/dir\"".to_string()],
                    )
                    .add_command(
                        "help".to_string(),
                        "Show this help information".to_string(),
                        vec!["lla plugin run dirs_meta help".to_string()],
                    );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic directory information (file count and total size)".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show detailed directory information including subdirectories and modification time".to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&DirsConfig::default().colors))
                        .style(BoxStyle::Minimal)
                        .padding(2)
                        .render()
                );
                Ok(())
            }
        );

        registry
    });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirsConfig {
    #[serde(default = "default_cache_size")]
    cache_size: usize,
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
    #[serde(default = "default_scan_depth")]
    max_scan_depth: usize,
    #[serde(default = "default_parallel_threshold")]
    parallel_threshold: usize,
}

fn default_cache_size() -> usize {
    1000
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("files".to_string(), "bright_cyan".to_string());
    colors.insert("dirs".to_string(), "bright_green".to_string());
    colors.insert("size".to_string(), "bright_yellow".to_string());
    colors.insert("time".to_string(), "bright_magenta".to_string());
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

fn default_scan_depth() -> usize {
    100
}

fn default_parallel_threshold() -> usize {
    1000
}

impl Default for DirsConfig {
    fn default() -> Self {
        Self {
            cache_size: default_cache_size(),
            colors: default_colors(),
            max_scan_depth: default_scan_depth(),
            parallel_threshold: default_parallel_threshold(),
        }
    }
}

impl PluginConfig for DirsConfig {}

pub struct DirsPlugin {
    base: BasePlugin<DirsConfig>,
    persistent_cache: lla_plugin_utils::PersistentCache<DirectoryStats>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
struct DirectoryStats {
    files: usize,
    directories: usize,
    size: u64,
}

impl DirsPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
            persistent_cache: lla_plugin_utils::PersistentCache::for_plugin(
                plugin_name,
                "directory-cache.toml",
                1,
                20_000,
            ),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[DirsPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn analyze_directory(path: &Path) -> Option<(usize, usize, u64)> {
        let path_str = path.to_string_lossy().to_string();

        if let Ok(metadata) = path.metadata() {
            if let Ok(modified_time) = metadata.modified() {
                let cache = CACHE.read();
                if let Some((cached_time, stats)) = cache.get(&path_str) {
                    if *cached_time >= modified_time {
                        return Some(*stats);
                    }
                }
            }
        }

        let file_count = AtomicUsize::new(0);
        let dir_count = AtomicUsize::new(0);
        let total_size = AtomicU64::new(0);

        let entries: Vec<_> = WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .collect();

        entries.into_par_iter().for_each(|entry| {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    file_count.fetch_add(1, Ordering::Relaxed);
                    total_size.fetch_add(metadata.len(), Ordering::Relaxed);
                } else if metadata.is_dir() {
                    dir_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        let result = (
            file_count.load(Ordering::Relaxed),
            dir_count.load(Ordering::Relaxed),
            total_size.load(Ordering::Relaxed),
        );

        if let Ok(metadata) = path.metadata() {
            if let Ok(modified_time) = metadata.modified() {
                let mut cache = CACHE.write();
                if cache.len() >= DirsConfig::default().cache_size {
                    cache.clear();
                }
                cache.insert(path_str.clone(), (modified_time, result));
            }
        }

        Some(result)
    }

    fn stats_action(args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Path argument is required".to_string());
        }
        let path = Path::new(&args[0]);
        if !path.is_dir() {
            return Err("Path must be a directory".to_string());
        }

        let spinner = SPINNER.write();
        spinner.set_status("Analyzing directory...".to_string());

        let result = Self::analyze_directory(path);

        spinner.finish();
        drop(spinner);

        if let Some((files, dirs, size)) = result {
            let colors = DirsConfig::default().colors;
            let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

            list.add_item(
                KeyValue::new("Files", files.to_string())
                    .key_color(colors.get("files").unwrap_or(&"white".to_string()))
                    .value_color(colors.get("files").unwrap_or(&"white".to_string()))
                    .key_width(12)
                    .render(),
            );

            list.add_item(
                KeyValue::new("Directories", dirs.to_string())
                    .key_color(colors.get("dirs").unwrap_or(&"white".to_string()))
                    .value_color(colors.get("dirs").unwrap_or(&"white".to_string()))
                    .key_width(12)
                    .render(),
            );

            list.add_item(
                KeyValue::new("Total Size", format_size(size))
                    .key_color(colors.get("size").unwrap_or(&"white".to_string()))
                    .value_color(colors.get("size").unwrap_or(&"white".to_string()))
                    .key_width(12)
                    .render(),
            );

            println!("{}", list.render());
            Ok(())
        } else {
            Err("Failed to analyze directory".to_string())
        }
    }

    fn format_directory_info(&self, entry: &DecoratedEntry, format: &str) -> Option<String> {
        if !entry.metadata.is_dir {
            return None;
        }

        let (file_count, dir_count, total_size) = match (
            entry.custom_fields.get("dir_file_count"),
            entry.custom_fields.get("dir_subdir_count"),
            entry.custom_fields.get("dir_total_size"),
        ) {
            (Some(f), Some(d), Some(s)) => (f, d, s),
            _ => return None,
        };

        let colors = &self.base.config().colors;
        let total_size_display = Self::format_total_size(total_size);
        match format {
            "long" => {
                let modified = entry
                    .path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.elapsed().ok())
                    .map(|e| {
                        let secs = e.as_secs();
                        if secs < 60 {
                            format!("{} secs ago", secs)
                        } else if secs < 3600 {
                            format!("{} mins ago", secs / 60)
                        } else if secs < 86400 {
                            format!("{} hours ago", secs / 3600)
                        } else {
                            format!("{} days ago", secs / 86400)
                        }
                    })
                    .unwrap_or_else(|| "unknown time".to_string());

                let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

                list.add_item(
                    KeyValue::new("Files", file_count)
                        .key_color(colors.get("files").unwrap_or(&"white".to_string()))
                        .value_color(colors.get("files").unwrap_or(&"white".to_string()))
                        .key_width(12)
                        .render(),
                );

                list.add_item(
                    KeyValue::new("Directories", dir_count)
                        .key_color(colors.get("dirs").unwrap_or(&"white".to_string()))
                        .value_color(colors.get("dirs").unwrap_or(&"white".to_string()))
                        .key_width(12)
                        .render(),
                );

                list.add_item(
                    KeyValue::new("Total Size", &total_size_display)
                        .key_color(colors.get("size").unwrap_or(&"white".to_string()))
                        .value_color(colors.get("size").unwrap_or(&"white".to_string()))
                        .key_width(12)
                        .render(),
                );

                list.add_item(
                    KeyValue::new("Modified", modified)
                        .key_color(colors.get("time").unwrap_or(&"white".to_string()))
                        .value_color(colors.get("time").unwrap_or(&"white".to_string()))
                        .key_width(12)
                        .render(),
                );

                Some(format!("\n{}", list.render()))
            }
            "default" => Some(format!(
                "\n{}\n",
                TextBlock::new(format!("{} files, {}", file_count, total_size_display))
                    .color(colors.get("info").unwrap_or(&"white".to_string()))
                    .build()
            )),
            _ => None,
        }
    }

    fn format_total_size(value: &str) -> String {
        value
            .parse::<u64>()
            .map(format_size)
            .unwrap_or_else(|_| value.to_string())
    }

    fn analyze_batch(&mut self, requested: &[PathBuf]) -> HashMap<PathBuf, DirectoryStats> {
        let mut results = HashMap::new();
        let mut misses = Vec::new();
        for path in requested {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            let fingerprint = lla_plugin_utils::file_fingerprint(&canonical).unwrap_or_default();
            let key = canonical.to_string_lossy().into_owned();
            if let Some(stats) = self.persistent_cache.get_fresh_matching(
                &key,
                Some(&fingerprint),
                std::time::Duration::from_secs(2),
            ) {
                results.insert(canonical, stats);
            } else {
                misses.push((canonical, key, fingerprint));
            }
        }

        let mut roots = misses
            .iter()
            .map(|(path, _, _)| path.clone())
            .collect::<Vec<_>>();
        roots.sort_by_key(|path| path.components().count());
        let mut scan_roots = Vec::<PathBuf>::new();
        for path in roots {
            if !scan_roots.iter().any(|root| path.starts_with(root)) {
                scan_roots.push(path);
            }
        }
        let scans = scan_roots
            .par_iter()
            .map(|root| scan_directory_tree(root))
            .collect::<Vec<_>>();
        for (path, key, fingerprint) in misses {
            let stats = scans
                .iter()
                .find_map(|scan| scan.get(&path).copied())
                .unwrap_or_default();
            self.persistent_cache.insert(key, fingerprint, stats);
            results.insert(path, stats);
        }
        let _ = self.persistent_cache.persist();
        results
    }
}

impl Plugin for DirsPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        self.decorate_batch(vec![entry], "default")
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        let requested = entries
            .iter()
            .filter(|entry| {
                entry
                    .metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.is_dir)
            })
            .map(|entry| PathBuf::from(&entry.path))
            .collect::<Vec<_>>();
        let stats = self.analyze_batch(&requested);
        entries
            .into_iter()
            .map(|mut entry| {
                let path = Path::new(&entry.path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&entry.path));
                if let Some(stats) = stats.get(&path) {
                    entry.insert_field(
                        "dir_file_count",
                        value::integer(stats.files as i64),
                        stats.files.to_string(),
                    );
                    entry.insert_field(
                        "dir_subdir_count",
                        value::integer(stats.directories as i64),
                        stats.directories.to_string(),
                    );
                    entry.insert_field(
                        "dir_total_size",
                        value::bytes(stats.size),
                        stats.size.to_string(),
                    );
                }
                entry
            })
            .collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| self.format_directory_info(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        if action == "clear-cache" {
            self.persistent_cache.clear();
            let _ = self.persistent_cache.persist();
        }
        run_cli_action(
            &action,
            arguments,
            include_str!("../plugin.toml"),
            |arguments| ACTION_REGISTRY.read().handle(&action, arguments),
        )
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        lla_plugin_utils::manifest_action_infos(include_str!("../plugin.toml"))
    }
}

fn scan_directory_tree(root: &Path) -> HashMap<PathBuf, DirectoryStats> {
    let mut stats = HashMap::<PathBuf, DirectoryStats>::new();
    for item in WalkDir::new(root).follow_links(false).into_iter().flatten() {
        let path = item.path();
        let Ok(metadata) = item.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            stats.entry(path.to_path_buf()).or_insert(DirectoryStats {
                directories: 1,
                ..DirectoryStats::default()
            });
        } else if metadata.is_file() {
            if let Some(parent) = path.parent() {
                let parent = stats.entry(parent.to_path_buf()).or_insert(DirectoryStats {
                    directories: 1,
                    ..DirectoryStats::default()
                });
                parent.files += 1;
                parent.size = parent.size.saturating_add(metadata.len());
            }
        }
    }
    let mut directories = stats.keys().cloned().collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        if directory == root {
            continue;
        }
        let Some(parent_path) = directory.parent() else {
            continue;
        };
        let child = stats.get(&directory).copied().unwrap_or_default();
        if let Some(parent) = stats.get_mut(parent_path) {
            parent.files = parent.files.saturating_add(child.files);
            parent.directories = parent.directories.saturating_add(child.directories);
            parent.size = parent.size.saturating_add(child.size);
        }
    }
    stats
}

impl Default for DirsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for DirsPlugin {
    type Config = DirsConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

lla_plugin_sdk::export_plugin!(DirsPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_byte_values_keep_the_existing_human_size_display() {
        assert_eq!(DirsPlugin::format_total_size("1024"), format_size(1024));
        assert_eq!(DirsPlugin::format_total_size("legacy"), "legacy");
    }

    #[test]
    fn one_tree_scan_aggregates_overlapping_directories() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        let nested = child.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(child.join("one.bin"), [0_u8; 3]).unwrap();
        std::fs::write(nested.join("two.bin"), [0_u8; 5]).unwrap();

        let stats = scan_directory_tree(root.path());
        assert_eq!(stats[root.path()].files, 2);
        assert_eq!(stats[root.path()].directories, 3);
        assert_eq!(stats[root.path()].size, 8);
        assert_eq!(stats[&child].files, 2);
        assert_eq!(stats[&child].directories, 2);
    }
}
