use lazy_static::lazy_static;
use lla_plugin_sdk::{
    interface::proto, response, value, ActionArguments, DecoratedEntryExt, Plugin,
};
use lla_plugin_utils::DecoratedEntry;
use lla_plugin_utils::{
    action_arguments_as_strings, action_infos,
    config::PluginConfig,
    decode_decorated_entry,
    ui::{
        components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List},
        format_size,
    },
    ActionRegistry, BasePlugin, ConfigurablePlugin,
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
            ["lla plugin --name file_meta --action help"],
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

    fn format_timestamp_value(value: &str) -> String {
        value
            .parse::<u64>()
            .ok()
            .map(|seconds| {
                Self::format_timestamp(
                    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds),
                )
            })
            .unwrap_or_else(|| value.to_string())
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
                        let accessed_display = Self::format_timestamp_value(accessed);
                        let modified_display = Self::format_timestamp_value(modified);
                        let created_display = Self::format_timestamp_value(created);
                        let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

                        list.add_item(
                            KeyValue::new("Accessed", &accessed_display)
                                .key_color(colors.get("accessed").unwrap_or(&"white".to_string()))
                                .value_color(colors.get("accessed").unwrap_or(&"white".to_string()))
                                .key_width(12)
                                .render(),
                        );

                        list.add_item(
                            KeyValue::new("Modified", &modified_display)
                                .key_color(colors.get("modified").unwrap_or(&"white".to_string()))
                                .value_color(colors.get("modified").unwrap_or(&"white".to_string()))
                                .key_width(12)
                                .render(),
                        );

                        list.add_item(
                            KeyValue::new("Created", &created_display)
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
    fn decorate_entry(&mut self, mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        let Some(metadata) = entry.metadata.clone() else {
            return entry;
        };
        entry
            .custom_fields
            .insert("accessed".into(), metadata.accessed.to_string());
        entry.insert_field(
            "modified",
            value::timestamp(metadata.modified),
            metadata.modified.to_string(),
        );
        entry
            .custom_fields
            .insert("created".into(), metadata.created.to_string());
        entry
            .custom_fields
            .insert("uid".into(), metadata.uid.to_string());
        entry
            .custom_fields
            .insert("gid".into(), metadata.gid.to_string());
        entry.insert_field(
            "size",
            value::bytes(metadata.size),
            metadata.size.to_string(),
        );
        entry.insert_field(
            "permissions",
            value::integer(i64::from(metadata.permissions)),
            metadata.permissions.to_string(),
        );
        entry
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        entries
            .into_iter()
            .map(|entry| self.decorate_entry(entry))
            .collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| self.format_file_info(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        let arguments = action_arguments_as_strings(arguments);
        response::from_result(ACTION_REGISTRY.read().handle(&action, &arguments))
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        action_infos(ACTION_REGISTRY.read().list_actions())
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

lla_plugin_sdk::export_plugin!(FileMetadataPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_timestamps_keep_the_existing_human_display() {
        assert_eq!(
            FileMetadataPlugin::format_timestamp_value("0"),
            FileMetadataPlugin::format_timestamp(SystemTime::UNIX_EPOCH)
        );
        assert_eq!(
            FileMetadataPlugin::format_timestamp_value("legacy"),
            "legacy"
        );
    }
}
