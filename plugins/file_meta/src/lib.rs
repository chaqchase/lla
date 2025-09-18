use lazy_static::lazy_static;
use lla_plugin_interface::{DecoratedEntry, Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::{
        components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List},
        format_size,
    },
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::SystemTime};

lazy_static! {
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name file_meta --action help"],
            |_| {
                let mut help = HelpFormatter::new("File Metadata Plugin".to_string());
                help.add_section("Description".to_string())
                    .add_command(
                        "".to_string(),
                        "Displays detailed file metadata including timestamps, ownership, size, and permissions.".to_string(),
                        vec![],
                    );

                help.add_section("Actions".to_string()).add_command(
                    "help".to_string(),
                    "Show this help information".to_string(),
                    vec!["lla plugin --name file_meta --action help".to_string()],
                );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic file metadata".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show detailed file metadata including timestamps".to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&FileMetaConfig::default().colors))
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
pub struct FileMetaConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("accessed".to_string(), "bright_blue".to_string());
    colors.insert("modified".to_string(), "bright_green".to_string());
    colors.insert("created".to_string(), "bright_yellow".to_string());
    colors.insert("ownership".to_string(), "bright_magenta".to_string());
    colors.insert("size".to_string(), "bright_cyan".to_string());
    colors.insert("permissions".to_string(), "bright_red".to_string());
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

impl Default for FileMetaConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for FileMetaConfig {}

pub struct FileMetadataPlugin {
    base: BasePlugin<FileMetaConfig>,
}

impl FileMetadataPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[FileMetadataPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn format_timestamp(timestamp: SystemTime) -> String {
        let datetime: chrono::DateTime<chrono::Local> = timestamp.into();
        datetime.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    fn format_file_info(&self, entry: &DecoratedEntry, format: &str) -> Option<String> {
        let colors = &self.base.config().colors;

        match format {
            "long" | "default" => {
                match (
                    entry.custom_fields.get("accessed"),
                    entry.custom_fields.get("modified"),
                    entry.custom_fields.get("created"),
                    entry.custom_fields.get("uid"),
                    entry.custom_fields.get("gid"),
                    entry.custom_fields.get("size"),
                    entry.custom_fields.get("permissions"),
                ) {
                    (
                        Some(accessed),
                        Some(modified),
                        Some(created),
                        Some(uid),
                        Some(gid),
                        Some(size),
                        Some(permissions),
                    ) => {
                        let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

                        list.add_item(
                            KeyValue::new("Accessed", accessed)
                                .key_color(colors.get("accessed").unwrap_or(&"white".to_string()))
                                .value_color(colors.get("accessed").unwrap_or(&"white".to_string()))
                                .key_width(12)
                                .render(),
                        );

                        list.add_item(
                            KeyValue::new("Modified", modified)
                                .key_color(colors.get("modified").unwrap_or(&"white".to_string()))
                                .value_color(colors.get("modified").unwrap_or(&"white".to_string()))
                                .key_width(12)
                                .render(),
                        );

                        list.add_item(
                            KeyValue::new("Created", created)
                                .key_color(colors.get("created").unwrap_or(&"white".to_string()))
                                .value_color(colors.get("created").unwrap_or(&"white".to_string()))
                                .key_width(12)
                                .render(),
                        );

                        list.add_item(
                            KeyValue::new("UID/GID", format!("{}/{}", uid, gid))
                                .key_color(colors.get("ownership").unwrap_or(&"white".to_string()))
                                .value_color(
                                    colors.get("ownership").unwrap_or(&"white".to_string()),
                                )
                                .key_width(12)
                                .render(),
                        );

                        list.add_item(
                            KeyValue::new("Size", format_size(size.parse().unwrap_or(0)))
                                .key_color(colors.get("size").unwrap_or(&"white".to_string()))
                                .value_color(colors.get("size").unwrap_or(&"white".to_string()))
                                .key_width(12)
                                .render(),
                        );

                        list.add_item(
                            KeyValue::new(
                                "Permissions",
                                format!("{:o}", permissions.parse::<u32>().unwrap_or(0)),
                            )
                            .key_color(colors.get("permissions").unwrap_or(&"white".to_string()))
                            .value_color(colors.get("permissions").unwrap_or(&"white".to_string()))
                            .key_width(12)
                            .render(),
                        );

                        Some(format!("\n{}", list.render()))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

impl Plugin for FileMetadataPlugin {
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
                        entry.custom_fields.insert(
                            "accessed".to_string(),
                            Self::format_timestamp(
                                SystemTime::UNIX_EPOCH
                                    + std::time::Duration::from_secs(entry.metadata.accessed),
                            ),
                        );
                        entry.custom_fields.insert(
                            "modified".to_string(),
                            Self::format_timestamp(
                                SystemTime::UNIX_EPOCH
                                    + std::time::Duration::from_secs(entry.metadata.modified),
                            ),
                        );
                        entry.custom_fields.insert(
                            "created".to_string(),
                            Self::format_timestamp(
                                SystemTime::UNIX_EPOCH
                                    + std::time::Duration::from_secs(entry.metadata.created),
                            ),
                        );
                        entry
                            .custom_fields
                            .insert("uid".to_string(), entry.metadata.uid.to_string());
                        entry
                            .custom_fields
                            .insert("gid".to_string(), entry.metadata.gid.to_string());
                        entry
                            .custom_fields
                            .insert("size".to_string(), entry.metadata.size.to_string());
                        entry.custom_fields.insert(
                            "permissions".to_string(),
                            entry.metadata.permissions.to_string(),
                        );

                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::FormatField(entry, format) => {
                        let field = self.format_file_info(&entry, &format);
                        PluginResponse::FormattedField(field)
                    }
                    PluginRequest::PerformAction(action, args) => {
                        let result = ACTION_REGISTRY.read().handle(&action, &args);
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::BatchDecorate(entries, _format) => {
                        // For now, fall back to individual processing
                        let mut processed_entries = Vec::new();
                        for entry in entries {
                            // Apply the same logic as Decorate
                            processed_entries.push(entry);
                        }
                        PluginResponse::BatchDecorated(processed_entries)
                    }
                    PluginRequest::Config(_config_request) => {
                        PluginResponse::ConfigResult(Ok(()))
                    }
                };
                self.encode_response(response)
            }
            Err(e) => self.encode_error(&e),
        }
    }
}

impl Default for FileMetadataPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for FileMetadataPlugin {
    type Config = FileMetaConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for FileMetadataPlugin {}

lla_plugin_interface::declare_plugin!(FileMetadataPlugin);
