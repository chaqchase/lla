use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, value, ActionArguments, DecoratedEntryExt, Plugin};
use lla_plugin_utils::{
    config::PluginConfig,
    decode_decorated_entry, run_cli_action,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
    ActionRegistry, BasePlugin, ConfigurablePlugin,
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
            ["lla plugin run file_hash help"],
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
                    vec!["lla plugin run file_hash help".to_string()],
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
        entry: &lla_plugin_utils::DecoratedEntry,
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
    fn decorate_entry(&mut self, mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        if entry
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.is_file)
        {
            let spinner = SPINNER.write();
            spinner.set_status("Calculating hashes...".to_string());
            if let Some((sha1, sha256)) = Self::calculate_hashes(entry.path.as_ref()) {
                entry.insert_field("sha1", value::string(&sha1), sha1);
                entry.insert_field("sha256", value::string(&sha256), sha256);
            }
            spinner.finish();
        }
        entry
    }

    fn decorate_batch(
        &mut self,
        mut entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        let spinner = SPINNER.write();
        spinner.set_status("Calculating hashes...".to_string());
        for entry in &mut entries {
            if entry
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.is_file)
            {
                if let Some((sha1, sha256)) = Self::calculate_hashes(entry.path.as_ref()) {
                    entry.insert_field("sha1", value::string(&sha1), sha1);
                    entry.insert_field("sha256", value::string(&sha256), sha256);
                }
            }
        }
        spinner.finish();
        entries
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| self.format_hash_info(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
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

lla_plugin_sdk::export_plugin!(FileHashPlugin);
