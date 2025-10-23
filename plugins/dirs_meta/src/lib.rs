use lazy_static::lazy_static;
use lla_plugin_interface::{DecoratedEntry, Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::{
        components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
        format_size, TextBlock,
    },
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::Path,
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
            vec!["lla plugin --name dirs_meta --action clear-cache"],
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
            vec!["lla plugin --name dirs_meta --action stats --args \"/path/to/dir\""],
            |args| DirsPlugin::stats_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name dirs_meta --action help"],
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
                        vec!["lla plugin --name dirs_meta --action clear-cache".to_string()],
                    )
                    .add_command(
                        "stats".to_string(),
                        "Show detailed statistics for a directory".to_string(),
                        vec![
                            "lla plugin --name dirs_meta --action stats --args \"/path/to/dir\""
                                .to_string(),
                        ],
                    )
                    .add_command(
                        "help".to_string(),
                        "Show this help information".to_string(),
                        vec!["lla plugin --name dirs_meta --action help".to_string()],
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
}

impl DirsPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
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
                    KeyValue::new("Total Size", total_size)
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
                TextBlock::new(format!("{} files, {}", file_count, total_size))
                    .color(colors.get("info").unwrap_or(&"white".to_string()))
                    .build()
            )),
            _ => None,
        }
    }
}

impl Plugin for DirsPlugin {
    fn handle_raw_request(&mut self, request: &[u8]) -> Vec<u8> {
        match self.decode_request(request) {
            Ok(request) => {
                let response = match request {
                    PluginRequest::GetName => {
                        PluginResponse::Name(env!("CARGO_PKG_NAME").to_string())
                    }
                    PluginRequest::GetVersion => {
                        PluginResponse::Version(env!("CARGO_PKG_VERSION").to_string())
                    }
                    PluginRequest::GetDescription => {
                        PluginResponse::Description(env!("CARGO_PKG_DESCRIPTION").to_string())
                    }
                    PluginRequest::GetSupportedFormats => PluginResponse::SupportedFormats(vec![
                        "default".to_string(),
                        "long".to_string(),
                    ]),
                    PluginRequest::Decorate(mut entry) => {
                        if entry.metadata.is_dir {
                            let result = Self::analyze_directory(&entry.path);

                            if let Some((file_count, dir_count, total_size)) = result {
                                entry
                                    .custom_fields
                                    .insert("dir_file_count".to_string(), file_count.to_string());
                                entry
                                    .custom_fields
                                    .insert("dir_subdir_count".to_string(), dir_count.to_string());
                                entry
                                    .custom_fields
                                    .insert("dir_total_size".to_string(), format_size(total_size));
                            }
                        }
                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::FormatField(entry, format) => {
                        let field = self.format_directory_info(&entry, &format);
                        PluginResponse::FormattedField(field)
                    }
                    PluginRequest::PerformAction(action, args) => {
                        let result = ACTION_REGISTRY.read().handle(&action, &args);
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::GetAvailableActions => {
                        PluginResponse::AvailableActions(ACTION_REGISTRY.read().list_actions())
                    }
                };
                self.encode_response(response)
            }
            Err(e) => self.encode_error(&e),
        }
    }
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

impl ProtobufHandler for DirsPlugin {}

lla_plugin_interface::declare_plugin!(DirsPlugin);
