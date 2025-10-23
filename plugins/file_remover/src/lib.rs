use colored::Colorize;
use dialoguer::{Confirm, MultiSelect};
use lazy_static::lazy_static;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
};

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
            "Remove files/directories from current or specified directory",
            vec![
                "lla plugin --name file_remover --action remove",
                "lla plugin --name file_remover --action remove --args /path/to/dir"
            ],
            |args| FileRemoverPlugin::remove_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name file_remover --action help"],
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

    fn remove_directory_recursively(path: &Path) -> Result<(), String> {
        if !path.is_dir() {
            return Err(format!("{} is not a directory", path.display()));
        }

        fs::remove_dir_all(path)
            .map_err(|e| format!("Failed to remove directory {}: {}", path.display(), e))
    }

    fn remove_item(path: &Path) -> Result<(), String> {
        if path.is_dir() {
            Self::remove_directory_recursively(path)
        } else {
            fs::remove_file(path)
                .map_err(|e| format!("Failed to remove file {}: {}", path.display(), e))
        }
    }

    fn remove_action(args: &[String]) -> Result<(), String> {
        let dir = Self::get_directory(args.get(0).map(|s| s.as_str()))?;

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
            .with_prompt("Select items to remove (Space to select, Enter to confirm)")
            .items(&items)
            .interact()
            .map_err(|e| format!("Failed to show selector: {}", e))?;

        if selections.is_empty() {
            println!("{} No items selected", "Info:".bright_blue());
            return Ok(());
        }

        println!(
            "\n{} The following items will be removed:",
            "Warning:".bright_yellow()
        );
        for &idx in &selections {
            println!("  {} {}", "→".bright_red(), items[idx].bright_yellow());
        }

        let confirmed = Confirm::with_theme(&theme)
            .with_prompt("Are you sure you want to remove these items?")
            .default(false)
            .interact()
            .map_err(|e| format!("Failed to show confirmation: {}", e))?;

        if !confirmed {
            println!("{} Operation cancelled", "Info:".bright_blue());
            return Ok(());
        }

        let mut success_count = 0;
        let mut error_count = 0;

        for &idx in &selections {
            let path = &entries[idx];
            match Self::remove_item(path) {
                Ok(()) => {
                    println!(
                        "{} Removed: {}",
                        "Success:".bright_green(),
                        path.display().to_string().bright_yellow()
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
            "\n{} Operation completed: {} items removed, {} errors",
            "Summary:".bright_blue(),
            success_count.to_string().bright_green(),
            error_count.to_string().bright_red()
        );

        Ok(())
    }

    fn help_action() -> Result<(), String> {
        let mut help = HelpFormatter::new("File Remover".to_string());
        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Remove files and directories with interactive selection".to_string(),
            vec![],
        );

        help.add_section("Commands".to_string()).add_command(
            "remove [path]".to_string(),
            "Remove files/directories from current or specified directory".to_string(),
            vec![
                "lla plugin --name file_remover --action remove".to_string(),
                "lla plugin --name file_remover --action remove --args /path/to/dir".to_string(),
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
                    PluginRequest::GetSupportedFormats => {
                        PluginResponse::SupportedFormats(vec!["default".to_string()])
                    }
                    PluginRequest::Decorate(entry) => PluginResponse::Decorated(entry),
                    PluginRequest::FormatField(_, _) => PluginResponse::FormattedField(None),
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

impl ConfigurablePlugin for FileRemoverPlugin {
    type Config = RemoverConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for FileRemoverPlugin {}

lla_plugin_interface::declare_plugin!(FileRemoverPlugin);

impl Default for FileRemoverPlugin {
    fn default() -> Self {
        Self::new()
    }
}
