use lazy_static::lazy_static;
use lla_plugin_interface::{DecoratedEntry, Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::{
        components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
        TextBlock,
    },
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone)]
struct FileInfo {
    path: PathBuf,
    modified: SystemTime,
}

lazy_static! {
    static ref CACHE: RwLock<HashMap<String, Vec<FileInfo>>> = RwLock::new(HashMap::new());
    static ref SPINNER: RwLock<Spinner> = RwLock::new(Spinner::new());
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "clear-cache",
            "clear-cache",
            "Clear the duplicate file detection cache",
            vec!["lla plugin --name duplicate_file_detector --action clear-cache"],
            |_| {
                let spinner = SPINNER.write();
                spinner.set_status("Clearing cache...".to_string());
                CACHE.write().clear();
                spinner.finish();
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
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name duplicate_file_detector --action help"],
            |_| {
                let mut help = HelpFormatter::new("Duplicate File Detector Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Detects duplicate files by comparing their content hashes.".to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string())
                    .add_command(
                        "clear-cache".to_string(),
                        "Clear the duplicate file detection cache".to_string(),
                        vec![
                            "lla plugin --name duplicate_file_detector --action clear-cache"
                                .to_string(),
                        ],
                    )
                    .add_command(
                        "help".to_string(),
                        "Show this help information".to_string(),
                        vec!["lla plugin --name duplicate_file_detector --action help".to_string()],
                    );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic duplicate information".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show detailed duplicate information including paths".to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&DuplicateConfig::default().colors))
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
pub struct DuplicateConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("duplicate".to_string(), "bright_red".to_string());
    colors.insert("has_duplicates".to_string(), "bright_yellow".to_string());
    colors.insert("path".to_string(), "bright_cyan".to_string());
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

impl Default for DuplicateConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for DuplicateConfig {}

pub struct DuplicateFileDetectorPlugin {
    base: BasePlugin<DuplicateConfig>,
}

impl DuplicateFileDetectorPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[DuplicateFileDetectorPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn get_file_hash(path: &Path) -> Option<String> {
        let mut file = File::open(path).ok()?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];

        loop {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => hasher.update(&buffer[..n]),
                Err(_) => return None,
            }
        }
        Some(format!("{:x}", hasher.finalize()))
    }

    fn process_entry(&self, mut entry: DecoratedEntry) -> DecoratedEntry {
        if !entry.metadata.is_file {
            return entry;
        }

        let spinner = SPINNER.write();
        spinner.set_status("Checking for duplicates...".to_string());

        if let Some(hash) = Self::get_file_hash(&entry.path) {
            let mut cache = CACHE.write();
            let entries = cache.entry(hash).or_default();

            let modified = entry
                .path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or_else(SystemTime::now);

            if !entries.iter().any(|f| f.path == entry.path) {
                entries.push(FileInfo {
                    path: entry.path.clone(),
                    modified,
                });
            }

            if entries.len() > 1 {
                let oldest = entries.iter().min_by_key(|f| f.modified).unwrap();

                if oldest.path == entry.path {
                    entry.custom_fields.insert("has_duplicates".to_string(), "true".to_string());
                    entry.field_types.insert("has_duplicates".to_string(), lla_plugin_interface::FieldType {
                        field_type: "boolean".to_string(),
                        format: Some("red".to_string()),
                        unit: None,
                    });

                    let duplicate_paths: Vec<String> = entries
                        .iter()
                        .filter(|f| f.path != oldest.path)
                        .map(|f| f.path.to_string_lossy().to_string())
                        .collect();

                    entry.custom_fields.insert("duplicate_paths".to_string(), duplicate_paths.join(", "));
                    entry.field_types.insert("duplicate_paths".to_string(), lla_plugin_interface::FieldType {
                        field_type: "string".to_string(),
                        format: Some("list".to_string()),
                        unit: Some("paths".to_string()),
                    });
                } else {
                    entry.custom_fields.insert("is_duplicate".to_string(), "true".to_string());
                    entry.field_types.insert("is_duplicate".to_string(), lla_plugin_interface::FieldType {
                        field_type: "boolean".to_string(),
                        format: Some("yellow".to_string()),
                        unit: None,
                    });

                    entry.custom_fields.insert(
                        "original_path".to_string(),
                        oldest.path.to_string_lossy().to_string(),
                    );
                    entry.field_types.insert("original_path".to_string(), lla_plugin_interface::FieldType {
                        field_type: "string".to_string(),
                        format: Some("path".to_string()),
                        unit: None,
                    });
                }
            }
        }

        spinner.finish();
        entry
    }

    fn format_duplicate_info(&self, entry: &DecoratedEntry, format: &str) -> Option<String> {
        let colors = &self.base.config().colors;
        let mut list = List::new().style(BoxStyle::Minimal).key_width(15);

        if entry.custom_fields.get("has_duplicates").is_some() {
            match format {
                "long" => {
                    list.add_item(
                        KeyValue::new("Status", "HAS DUPLICATES")
                            .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                            .value_color(
                                colors.get("has_duplicates").unwrap_or(&"white".to_string()),
                            )
                            .key_width(15)
                            .render(),
                    );

                    if let Some(paths) = entry.custom_fields.get("duplicate_paths") {
                        list.add_item(
                            KeyValue::new("Duplicate Copies", paths)
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(colors.get("path").unwrap_or(&"white".to_string()))
                                .key_width(15)
                                .render(),
                        );
                    }
                }
                "default" => {
                    if let Some(paths) = entry.custom_fields.get("duplicate_paths") {
                        list.add_item(
                            KeyValue::new("Status", format!("HAS DUPLICATES: {}", paths))
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(
                                    colors.get("has_duplicates").unwrap_or(&"white".to_string()),
                                )
                                .key_width(15)
                                .render(),
                        );
                    } else {
                        list.add_item(
                            KeyValue::new("Status", "HAS DUPLICATES")
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(
                                    colors.get("has_duplicates").unwrap_or(&"white".to_string()),
                                )
                                .key_width(15)
                                .render(),
                        );
                    }
                }
                _ => return None,
            }
        } else if entry.custom_fields.get("is_duplicate").is_some() {
            match format {
                "long" => {
                    list.add_item(
                        KeyValue::new("Status", "DUPLICATE")
                            .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                            .value_color(colors.get("duplicate").unwrap_or(&"white".to_string()))
                            .key_width(15)
                            .render(),
                    );

                    if let Some(original) = entry.custom_fields.get("original_path") {
                        list.add_item(
                            KeyValue::new("Original File", original)
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(colors.get("path").unwrap_or(&"white".to_string()))
                                .key_width(15)
                                .render(),
                        );
                    }
                }
                "default" => {
                    if let Some(original) = entry.custom_fields.get("original_path") {
                        list.add_item(
                            KeyValue::new("Status", format!("DUPLICATE of {}", original))
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(
                                    colors.get("duplicate").unwrap_or(&"white".to_string()),
                                )
                                .key_width(15)
                                .render(),
                        );
                    } else {
                        list.add_item(
                            KeyValue::new("Status", "DUPLICATE")
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(
                                    colors.get("duplicate").unwrap_or(&"white".to_string()),
                                )
                                .key_width(15)
                                .render(),
                        );
                    }
                }
                _ => return None,
            }
        } else {
            return None;
        }

        Some(format!("\n{}", list.render()))
    }
}

impl Plugin for DuplicateFileDetectorPlugin {
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
                    PluginRequest::Decorate(entry) => {
                        PluginResponse::Decorated(self.process_entry(entry))
                    }
                    PluginRequest::BatchDecorate(entries, _format) => {
                        let mut processed_entries = Vec::new();
                        for entry in entries {
                            processed_entries.push(self.process_entry(entry));
                        }
                        PluginResponse::BatchDecorated(processed_entries)
                    }
                    PluginRequest::Config(_config_request) => {
                        PluginResponse::ConfigResult(Ok(()))
                    }
                    PluginRequest::FormatField(entry, format) => {
                        let field = self.format_duplicate_info(&entry, &format);
                        PluginResponse::FormattedField(field)
                    }
                    PluginRequest::PerformAction(action, args) => {
                        let result = ACTION_REGISTRY.read().handle(&action, &args);
                        PluginResponse::ActionResult(result)
                    }
                };
                self.encode_response(response)
            }
            Err(e) => self.encode_error(&e),
        }
    }
}

impl Default for DuplicateFileDetectorPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for DuplicateFileDetectorPlugin {
    type Config = DuplicateConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for DuplicateFileDetectorPlugin {}

lla_plugin_interface::declare_plugin!(DuplicateFileDetectorPlugin);
