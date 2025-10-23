use lazy_static::lazy_static;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
};

lazy_static! {
    static ref SPINNER: RwLock<Spinner> = RwLock::new(Spinner::new());
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name file_hash --action help"],
            |_| {
                let mut help = HelpFormatter::new("File Hash Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Calculates SHA1 and SHA256 hashes for files.".to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string()).add_command(
                    "help".to_string(),
                    "Show this help information".to_string(),
                    vec!["lla plugin --name file_hash --action help".to_string()],
                );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic hash information (first 8 characters)".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show complete hash values".to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&FileHashConfig::default().colors))
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
pub struct FileHashConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("sha1".to_string(), "bright_green".to_string());
    colors.insert("sha256".to_string(), "bright_yellow".to_string());
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

impl Default for FileHashConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for FileHashConfig {}

pub struct FileHashPlugin {
    base: BasePlugin<FileHashConfig>,
}

impl FileHashPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[FileHashPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn calculate_hashes(path: &std::path::Path) -> Option<(String, String)> {
        let file = File::open(path).ok()?;
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).ok()?;

        let sha1 = format!("{:x}", Sha1::digest(&buffer));
        let sha256 = format!("{:x}", Sha256::digest(&buffer));

        Some((sha1, sha256))
    }

    fn format_hash_info(
        &self,
        entry: &lla_plugin_interface::DecoratedEntry,
        format: &str,
    ) -> Option<String> {
        if !entry.metadata.is_file {
            return None;
        }

        let (sha1, sha256) = match (
            entry.custom_fields.get("sha1"),
            entry.custom_fields.get("sha256"),
        ) {
            (Some(s1), Some(s2)) => (s1, s2),
            _ => return None,
        };

        let colors = &self.base.config().colors;
        let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

        match format {
            "long" => {
                list.add_item(
                    KeyValue::new("SHA1", sha1)
                        .key_color(colors.get("sha1").unwrap_or(&"white".to_string()))
                        .value_color(colors.get("sha1").unwrap_or(&"white".to_string()))
                        .key_width(12)
                        .render(),
                );

                list.add_item(
                    KeyValue::new("SHA256", sha256)
                        .key_color(colors.get("sha256").unwrap_or(&"white".to_string()))
                        .value_color(colors.get("sha256").unwrap_or(&"white".to_string()))
                        .key_width(12)
                        .render(),
                );
            }
            "default" => {
                let sha1_short = &sha1[..8];
                let sha256_short = &sha256[..8];

                list.add_item(
                    KeyValue::new("SHA1", sha1_short)
                        .key_color(colors.get("sha1").unwrap_or(&"white".to_string()))
                        .value_color(colors.get("sha1").unwrap_or(&"white".to_string()))
                        .key_width(12)
                        .render(),
                );

                list.add_item(
                    KeyValue::new("SHA256", sha256_short)
                        .key_color(colors.get("sha256").unwrap_or(&"white".to_string()))
                        .value_color(colors.get("sha256").unwrap_or(&"white".to_string()))
                        .key_width(12)
                        .render(),
                );
            }
            _ => return None,
        };

        Some(format!("\n{}", list.render()))
    }
}

impl Plugin for FileHashPlugin {
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
                        if entry.metadata.is_file {
                            let spinner = SPINNER.write();
                            spinner.set_status("Calculating hashes...".to_string());

                            if let Some((sha1, sha256)) = Self::calculate_hashes(&entry.path) {
                                entry.custom_fields.insert("sha1".to_string(), sha1);
                                entry.custom_fields.insert("sha256".to_string(), sha256);
                            }

                            spinner.finish();
                        }
                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::FormatField(entry, format) => {
                        let field = self.format_hash_info(&entry, &format);
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

impl Default for FileHashPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for FileHashPlugin {
    type Config = FileHashConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for FileHashPlugin {}

lla_plugin_interface::declare_plugin!(FileHashPlugin);
