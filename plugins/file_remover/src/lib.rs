use colored::Colorize;
use dialoguer::{Confirm, MultiSelect};
use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, response, ActionArguments, Plugin};
use lla_plugin_utils::{
    action_arguments_as_strings, action_infos,
    config::PluginConfig,
    trash::{remove_path, TrashStore},
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    ActionRegistry, BasePlugin, ConfigurablePlugin,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{fs, ops::Deref, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoverConfig {
    #[serde(default = "default_colors")]
    colors: std::collections::HashMap<String, String>,
}

fn default_colors() -> std::collections::HashMap<String, String> {
    let mut colors = std::collections::HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("error".to_string(), "bright_red".to_string());
    colors.insert("path".to_string(), "bright_yellow".to_string());
    colors
}

impl Default for RemoverConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for RemoverConfig {}

lazy_static! {
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "remove",
            "remove [path]",
            "Move selected files/directories into recoverable trash",
            [
                "lla plugin --name file_remover --action remove",
                "lla plugin --name file_remover --action remove --args /path/to/dir"
            ],
            FileRemoverPlugin::remove_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "purge",
            "purge [path]",
            "Permanently delete selected files/directories after confirmation",
            [
                "lla plugin --name file_remover --action purge",
                "lla plugin --name file_remover --action purge --args /path/to/dir"
            ],
            FileRemoverPlugin::purge_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            ["lla plugin --name file_remover --action help"],
            |_| FileRemoverPlugin::help_action()
        );

        registry
    });
}

pub struct FileRemoverPlugin {
    base: BasePlugin<RemoverConfig>,
}

impl FileRemoverPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        Self {
            base: BasePlugin::with_name(plugin_name),
        }
    }

    fn get_directory(path_arg: Option<&str>) -> Result<PathBuf, String> {
        match path_arg {
            Some(path) => Ok(PathBuf::from(path)),
            None => std::env::current_dir()
                .map_err(|e| format!("Failed to get current directory: {}", e)),
        }
    }

    fn remove_action(args: &[String]) -> Result<(), String> {
        Self::select_and_remove(args, false)
    }

    fn purge_action(args: &[String]) -> Result<(), String> {
        Self::select_and_remove(args, true)
    }

    fn select_and_remove(args: &[String], permanent: bool) -> Result<(), String> {
        let dir = Self::get_directory(args.first().map(|s| s.as_str()))?;

        let entries = fs::read_dir(&dir)
            .map_err(|e| format!("Failed to read directory '{}': {}", dir.display(), e))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect::<Vec<_>>();

        if entries.is_empty() {
            println!(
                "{} Directory is empty: {}",
                "Info:".bright_blue(),
                dir.display()
            );
            return Ok(());
        }

        let items: Vec<String> = entries
            .iter()
            .map(|p| {
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if p.is_dir() {
                    format!("{} (directory)", name)
                } else {
                    name
                }
            })
            .collect();

        let theme = LlaDialoguerTheme::default();
        let selections = MultiSelect::with_theme(&theme)
            .with_prompt(if permanent {
                "Select items to PERMANENTLY delete (Space to select, Enter to confirm)"
            } else {
                "Select items to move to recoverable trash (Space to select, Enter to confirm)"
            })
            .items(&items)
            .interact()
            .map_err(|e| format!("Failed to show selector: {}", e))?;

        if selections.is_empty() {
            println!("{} No items selected", "Info:".bright_blue());
            return Ok(());
        }

        println!(
            "\n{} The following items will be {}:",
            "Warning:".bright_yellow(),
            if permanent {
                "permanently deleted"
            } else {
                "moved to trash"
            }
        );
        for &idx in &selections {
            println!("  {} {}", "→".bright_red(), items[idx].bright_yellow());
        }

        let confirmed = Confirm::with_theme(&theme)
            .with_prompt(if permanent {
                "Permanently delete these items? This cannot be undone"
            } else {
                "Move these items to recoverable trash?"
            })
            .default(false)
            .interact()
            .map_err(|e| format!("Failed to show confirmation: {}", e))?;

        if !confirmed {
            println!("{} Operation cancelled", "Info:".bright_blue());
            return Ok(());
        }

        let mut success_count = 0;
        let mut error_count = 0;
        let trash = TrashStore::for_plugin_data();

        for &idx in &selections {
            let path = &entries[idx];
            let result = if permanent {
                remove_path(path).map(|_| None)
            } else {
                trash.put(path).map(Some)
            };
            match result {
                Ok(record) => {
                    println!(
                        "{} {}: {}{}",
                        "Success:".bright_green(),
                        if permanent {
                            "Permanently deleted"
                        } else {
                            "Trashed"
                        },
                        path.display().to_string().bright_yellow(),
                        record
                            .map(|record| format!(" (id: {})", record.id))
                            .unwrap_or_default()
                    );
                    success_count += 1;
                }
                Err(e) => {
                    println!(
                        "{} Failed to remove {}: {}",
                        "Error:".bright_red(),
                        path.display().to_string().bright_yellow(),
                        e
                    );
                    error_count += 1;
                }
            }
        }

        println!(
            "\n{} Operation completed: {} items {}, {} errors",
            "Summary:".bright_blue(),
            success_count.to_string().bright_green(),
            if permanent {
                "permanently deleted"
            } else {
                "trashed"
            },
            error_count.to_string().bright_red()
        );

        Ok(())
    }

    fn help_action() -> Result<(), String> {
        let mut help = HelpFormatter::new("File Remover".to_string());
        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Move files and directories to recoverable trash by default".to_string(),
            vec![],
        );

        help.add_section("Commands".to_string())
            .add_command(
                "remove [path]".to_string(),
                "Move selected files/directories into recoverable trash".to_string(),
                vec![
                    "lla plugin --name file_remover --action remove".to_string(),
                    "lla plugin --name file_remover --action remove --args /path/to/dir"
                        .to_string(),
                ],
            )
            .add_command(
                "purge [path]".to_string(),
                "Permanently delete selected items after an explicit confirmation".to_string(),
                vec![
                    "lla plugin --name file_remover --action purge --args /path/to/dir".to_string(),
                ],
            );

        println!(
            "{}",
            BoxComponent::new(help.render(&RemoverConfig::default().colors))
                .style(BoxStyle::Minimal)
                .padding(1)
                .render()
        );
        Ok(())
    }
}

impl Deref for FileRemoverPlugin {
    type Target = RemoverConfig;

    fn deref(&self) -> &Self::Target {
        self.base.config()
    }
}

impl Plugin for FileRemoverPlugin {
    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        let arguments = action_arguments_as_strings(arguments);
        response::from_result(ACTION_REGISTRY.read().handle(&action, &arguments))
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        action_infos(ACTION_REGISTRY.read().list_actions())
    }
}

impl ConfigurablePlugin for FileRemoverPlugin {
    type Config = RemoverConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

lla_plugin_sdk::export_plugin!(FileRemoverPlugin);

impl Default for FileRemoverPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recoverable_and_permanent_actions_are_both_explicit() {
        let actions = ACTION_REGISTRY
            .read()
            .list_actions()
            .into_iter()
            .map(|action| action.name)
            .collect::<std::collections::HashSet<_>>();
        assert!(actions.contains("remove"));
        assert!(actions.contains("purge"));
        assert!(actions.contains("help"));
    }
}
