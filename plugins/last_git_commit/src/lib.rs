use lazy_static::lazy_static;
use lla_plugin_sdk::{
    interface::proto, response, value, ActionArguments, DecoratedEntryExt, Plugin,
};
use lla_plugin_utils::{
    action_arguments_as_strings, action_infos,
    config::PluginConfig,
    decode_decorated_entry,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
    ActionRegistry, BasePlugin, ConfigurablePlugin,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
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
            ["lla plugin --name last_git_commit --action help"],
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
                "--format=format:{ \"hash\": \"%h\", \"author\": \"%an\", \"time\": \"%at\" }",
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
                let hash = json.get("hash").and_then(|v| v.as_str())?.to_string();
                let author = json.get("author").and_then(|v| v.as_str())?.to_string();
                let time = json.get("time").and_then(|v| v.as_str())?.to_string();

                Some((hash, author, time))
            }
            Err(_) => None,
        }
    }

    fn format_relative_time(value: &str) -> String {
        let Some(timestamp) = value.parse::<u64>().ok() else {
            return value.to_string();
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let elapsed = now.saturating_sub(timestamp);
        if elapsed < 60 {
            format!("{} seconds ago", elapsed)
        } else if elapsed < 3_600 {
            format!("{} minutes ago", elapsed / 60)
        } else if elapsed < 86_400 {
            format!("{} hours ago", elapsed / 3_600)
        } else {
            format!("{} days ago", elapsed / 86_400)
        }
    }

    fn format_commit_info(
        &self,
        entry: &lla_plugin_utils::DecoratedEntry,
        format: &str,
    ) -> Option<String> {
        let colors = &self.base.config().colors;
        let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

        if let (Some(hash), Some(author), Some(time)) = (
            entry.custom_fields.get("commit_hash"),
            entry.custom_fields.get("commit_author"),
            entry.custom_fields.get("commit_time"),
        ) {
            let time_display = Self::format_relative_time(time);
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
                    let kv = KeyValue::new("Time", &time_display)
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
                    let kv = KeyValue::new("Commit", format!("{} {}", hash, time_display))
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
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        let spinner = SPINNER.write();
        spinner.set_status("Checking last commit...".to_string());
        let entry = decorate_commit_entry(entry);
        spinner.finish();
        entry
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        let spinner = SPINNER.write();
        spinner.set_status("Checking last commit...".to_string());
        let entries = entries.into_iter().map(decorate_commit_entry).collect();
        spinner.finish();
        entries
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| self.format_commit_info(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        let arguments = action_arguments_as_strings(arguments);
        response::from_result(ACTION_REGISTRY.read().handle(&action, &arguments))
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        action_infos(ACTION_REGISTRY.read().list_actions())
    }
}

fn decorate_commit_entry(mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
    if let Some((hash, author, time)) =
        LastGitCommitPlugin::get_last_commit_info(entry.path.as_ref())
    {
        entry.insert_field("commit_hash", value::string(&hash), hash);
        entry.insert_field("commit_author", value::string(&author), author);
        let timestamp = time.parse::<u64>().unwrap_or_default();
        entry.insert_field("commit_time", value::timestamp(timestamp), time);
    }
    entry
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

lla_plugin_sdk::export_plugin!(LastGitCommitPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_commit_timestamps_keep_a_relative_human_display() {
        let two_minutes_ago = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(120);
        assert_eq!(
            LastGitCommitPlugin::format_relative_time(&two_minutes_ago.to_string()),
            "2 minutes ago"
        );
        assert_eq!(
            LastGitCommitPlugin::format_relative_time("legacy"),
            "legacy"
        );
    }
}
