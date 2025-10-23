use lazy_static::lazy_static;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
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
            vec!["lla plugin --name git_status --action help"],
            |_| {
                let mut help = HelpFormatter::new("Git Status Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Shows Git repository status information for files and directories."
                        .to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string()).add_command(
                    "help".to_string(),
                    "Show this help information".to_string(),
                    vec!["lla plugin --name git_status --action help".to_string()],
                );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic Git status information".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show detailed Git status information including branch and commit details"
                            .to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&GitConfig::default().colors))
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
pub struct GitConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("clean".to_string(), "bright_green".to_string());
    colors.insert("modified".to_string(), "bright_yellow".to_string());
    colors.insert("staged".to_string(), "bright_green".to_string());
    colors.insert("untracked".to_string(), "bright_blue".to_string());
    colors.insert("conflict".to_string(), "bright_red".to_string());
    colors.insert("branch".to_string(), "bright_cyan".to_string());
    colors.insert("commit".to_string(), "bright_yellow".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for GitConfig {}

pub struct GitStatusPlugin {
    base: BasePlugin<GitConfig>,
}

impl GitStatusPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[GitStatusPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn is_git_repo(path: &Path) -> bool {
        let mut current_dir = Some(path);
        while let Some(dir) = current_dir {
            if dir.join(".git").exists() {
                return true;
            }
            current_dir = dir.parent();
        }
        false
    }

    fn get_git_info(path: &Path) -> Option<(String, String, String)> {
        if !Self::is_git_repo(path) {
            return None;
        }

        let path_str = path.to_string_lossy();
        let parent = path.parent().unwrap_or(path);

        let status_output = Command::new("git")
            .args(["status", "--porcelain", "--ignored"])
            .arg(&*path_str)
            .current_dir(parent)
            .output()
            .ok()?;

        let branch_output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(parent)
            .output()
            .ok()?;

        let commit_output = Command::new("git")
            .args(["log", "-1", "--format=%h %s"])
            .current_dir(parent)
            .output()
            .ok()?;

        let status = String::from_utf8(status_output.stdout).ok()?;
        let branch = String::from_utf8(branch_output.stdout)
            .ok()?
            .trim()
            .to_string();
        let commit = String::from_utf8(commit_output.stdout)
            .ok()?
            .trim()
            .to_string();

        Some((status, branch, commit))
    }

    fn format_git_status(status: &str) -> (String, usize, usize, usize, usize) {
        let mut staged = 0;
        let mut modified = 0;
        let mut untracked = 0;
        let mut conflicts = 0;

        let mut formatted_entries = Vec::new();

        for line in status.lines() {
            let status_chars: Vec<char> = line.chars().take(2).collect();
            let index_status = status_chars.first().copied().unwrap_or(' ');
            let worktree_status = status_chars.get(1).copied().unwrap_or(' ');

            match (index_status, worktree_status) {
                ('M', ' ') => {
                    staged += 1;
                    formatted_entries.push("staged");
                }
                (' ', 'M') => {
                    modified += 1;
                    formatted_entries.push("modified");
                }
                ('M', 'M') => {
                    staged += 1;
                    modified += 1;
                    formatted_entries.push("staged & modified");
                }
                ('A', ' ') => {
                    staged += 1;
                    formatted_entries.push("new file");
                }
                ('D', ' ') | (' ', 'D') => {
                    modified += 1;
                    formatted_entries.push("deleted");
                }
                ('R', _) => {
                    staged += 1;
                    formatted_entries.push("renamed");
                }
                ('C', _) => {
                    staged += 1;
                    formatted_entries.push("copied");
                }
                ('U', _) | (_, 'U') => {
                    conflicts += 1;
                    formatted_entries.push("conflict");
                }
                (' ', '?') => {
                    untracked += 1;
                    formatted_entries.push("untracked");
                }
                _ => {}
            }
        }

        let status_summary = if formatted_entries.is_empty() {
            "clean".to_string()
        } else {
            formatted_entries.join(", ")
        };

        (status_summary, staged, modified, untracked, conflicts)
    }

    fn format_git_info(
        &self,
        entry: &lla_plugin_interface::DecoratedEntry,
        format: &str,
    ) -> Option<String> {
        let colors = &self.base.config().colors;
        let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

        if let (Some(status), Some(branch), Some(commit)) = (
            entry.custom_fields.get("git_status"),
            entry.custom_fields.get("git_branch"),
            entry.custom_fields.get("git_commit"),
        ) {
            match format {
                "long" => {
                    let key_color = colors
                        .get("info")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let value_color = colors
                        .get("branch")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let kv = KeyValue::new("Branch", branch)
                        .key_color(&key_color)
                        .value_color(&value_color)
                        .key_width(12);
                    list.add_item(kv.render());

                    let commit_parts: Vec<&str> = commit.split_whitespace().collect();
                    if let Some((hash, msg)) = commit_parts.split_first() {
                        let key_color = colors
                            .get("info")
                            .unwrap_or(&"white".to_string())
                            .to_string();
                        let value_color = colors
                            .get("commit")
                            .unwrap_or(&"white".to_string())
                            .to_string();
                        let kv = KeyValue::new("Commit", format!("{} {}", hash, msg.join(" ")))
                            .key_color(&key_color)
                            .value_color(&value_color)
                            .key_width(12);
                        list.add_item(kv.render());
                    }

                    let mut status_items = Vec::new();
                    if let Some(staged) = entry.custom_fields.get("git_staged") {
                        if let Ok(count) = staged.parse::<usize>() {
                            if count > 0 {
                                status_items.push(format!("{} staged", count));
                            }
                        }
                    }
                    if let Some(modified) = entry.custom_fields.get("git_modified") {
                        if let Ok(count) = modified.parse::<usize>() {
                            if count > 0 {
                                status_items.push(format!("{} modified", count));
                            }
                        }
                    }
                    if let Some(untracked) = entry.custom_fields.get("git_untracked") {
                        if let Ok(count) = untracked.parse::<usize>() {
                            if count > 0 {
                                status_items.push(format!("{} untracked", count));
                            }
                        }
                    }
                    if let Some(conflicts) = entry.custom_fields.get("git_conflicts") {
                        if let Ok(count) = conflicts.parse::<usize>() {
                            if count > 0 {
                                status_items.push(format!("{} conflicts", count));
                            }
                        }
                    }

                    let status_text = if status_items.is_empty() {
                        "working tree clean".to_string()
                    } else {
                        status_items.join(", ")
                    };

                    let key_color = colors
                        .get("info")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let value_color = if status_items.is_empty() {
                        colors
                            .get("clean")
                            .unwrap_or(&"white".to_string())
                            .to_string()
                    } else {
                        colors
                            .get("modified")
                            .unwrap_or(&"white".to_string())
                            .to_string()
                    };
                    let kv = KeyValue::new("Status", status_text)
                        .key_color(&key_color)
                        .value_color(&value_color)
                        .key_width(12);
                    list.add_item(kv.render());
                }
                "default" => {
                    let key_color = colors
                        .get("info")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let value_color = if status == "clean" {
                        colors
                            .get("clean")
                            .unwrap_or(&"white".to_string())
                            .to_string()
                    } else {
                        colors
                            .get("modified")
                            .unwrap_or(&"white".to_string())
                            .to_string()
                    };
                    let kv = KeyValue::new("Git", status)
                        .key_color(&key_color)
                        .value_color(&value_color)
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

impl Plugin for GitStatusPlugin {
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
                        spinner.set_status("Checking Git status...".to_string());

                        if let Some((status, branch, commit)) = Self::get_git_info(&entry.path) {
                            let (status_summary, staged, modified, untracked, conflicts) =
                                Self::format_git_status(&status);
                            entry
                                .custom_fields
                                .insert("git_status".to_string(), status_summary);
                            entry.custom_fields.insert("git_branch".to_string(), branch);
                            entry.custom_fields.insert("git_commit".to_string(), commit);
                            entry
                                .custom_fields
                                .insert("git_staged".to_string(), staged.to_string());
                            entry
                                .custom_fields
                                .insert("git_modified".to_string(), modified.to_string());
                            entry
                                .custom_fields
                                .insert("git_untracked".to_string(), untracked.to_string());
                            entry
                                .custom_fields
                                .insert("git_conflicts".to_string(), conflicts.to_string());
                        }

                        spinner.finish();
                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::FormatField(entry, format) => {
                        let field = self.format_git_info(&entry, &format);
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

impl Default for GitStatusPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for GitStatusPlugin {
    type Config = GitConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for GitStatusPlugin {}

lla_plugin_interface::declare_plugin!(GitStatusPlugin);
