use lazy_static::lazy_static;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json;
use std::{collections::HashMap, path::Path, process::Command};

lazy_static! {
    static ref SPINNER: RwLock<Spinner> = RwLock::new(Spinner::new());
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name last_git_commit --action help"],
            |_| {
                let mut help = HelpFormatter::new("Last Git Commit Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Shows information about the last Git commit for files.".to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string()).add_command(
                    "help".to_string(),
                    "Show this help information".to_string(),
                    vec!["lla plugin --name last_git_commit --action help".to_string()],
                );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic commit information".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show detailed commit information including author".to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&CommitConfig::default().colors))
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
pub struct CommitConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("hash".to_string(), "bright_yellow".to_string());
    colors.insert("author".to_string(), "bright_cyan".to_string());
    colors.insert("time".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

impl Default for CommitConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for CommitConfig {}

pub struct LastGitCommitPlugin {
    base: BasePlugin<CommitConfig>,
}

impl LastGitCommitPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[LastGitCommitPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn get_last_commit_info(path: &Path) -> Option<(String, String, String)> {
        let output = Command::new("git")
            .args([
                "log",
                "-1",
                "--format=format:{ \"hash\": \"%h\", \"author\": \"%an\", \"time\": \"%ar\" }",
                "--",
                path.to_str()?,
            ])
            .output()
            .ok()?;

        let output_str = String::from_utf8(output.stdout).ok()?;
        let trimmed = output_str.trim();

        if trimmed.is_empty() {
            return None;
        }

        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(json) => {
                let hash = match json.get("hash").and_then(|v| v.as_str()) {
                    Some(h) => h.to_string(),
                    None => return None,
                };

                let author = match json.get("author").and_then(|v| v.as_str()) {
                    Some(a) => a.to_string(),
                    None => return None,
                };

                let time = match json.get("time").and_then(|v| v.as_str()) {
                    Some(t) => t.to_string(),
                    None => return None,
                };

                Some((hash, author, time))
            }
            Err(_) => None,
        }
    }

    fn format_commit_info(
        &self,
        entry: &lla_plugin_interface::DecoratedEntry,
        format: &str,
    ) -> Option<String> {
        let colors = &self.base.config().colors;
        let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

        if let (Some(hash), Some(author), Some(time)) = (
            entry.custom_fields.get("commit_hash"),
            entry.custom_fields.get("commit_author"),
            entry.custom_fields.get("commit_time"),
        ) {
            match format {
                "long" => {
                    let key_color = colors
                        .get("info")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let hash_color = colors
                        .get("hash")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let kv = KeyValue::new("Commit", hash)
                        .key_color(&key_color)
                        .value_color(&hash_color)
                        .key_width(12);
                    list.add_item(kv.render());

                    let author_color = colors
                        .get("author")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let kv = KeyValue::new("Author", author)
                        .key_color(&key_color)
                        .value_color(&author_color)
                        .key_width(12);
                    list.add_item(kv.render());

                    let time_color = colors
                        .get("time")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let kv = KeyValue::new("Time", time)
                        .key_color(&key_color)
                        .value_color(&time_color)
                        .key_width(12);
                    list.add_item(kv.render());
                }
                "default" => {
                    let key_color = colors
                        .get("info")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let hash_color = colors
                        .get("hash")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let kv = KeyValue::new("Commit", format!("{} {}", hash, time))
                        .key_color(&key_color)
                        .value_color(&hash_color)
                        .key_width(12);
                    list.add_item(kv.render());
                }
                _ => return None,
            }

            Some(format!("\n{}", list.render()))
        } else {
            None
        }
    }
}

impl Plugin for LastGitCommitPlugin {
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
                        let spinner = SPINNER.write();
                        spinner.set_status("Checking last commit...".to_string());

                        if let Some((commit_hash, author, time)) =
                            Self::get_last_commit_info(&entry.path)
                        {
                            entry
                                .custom_fields
                                .insert("commit_hash".to_string(), commit_hash);
                            entry
                                .custom_fields
                                .insert("commit_author".to_string(), author);
                            entry.custom_fields.insert("commit_time".to_string(), time);
                        }

                        spinner.finish();
                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::FormatField(entry, format) => {
                        let field = self.format_commit_info(&entry, &format);
                        PluginResponse::FormattedField(field)
                    }
                    PluginRequest::PerformAction(action, args) => {
                        let result = ACTION_REGISTRY.read().handle(&action, &args);
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::BatchDecorate(entries, _format) => {
                        PluginResponse::BatchDecorated(entries)
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

impl Default for LastGitCommitPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for LastGitCommitPlugin {
    type Config = CommitConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for LastGitCommitPlugin {}

lla_plugin_interface::declare_plugin!(LastGitCommitPlugin);
