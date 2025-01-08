use colored::Colorize;
use dialoguer::MultiSelect;
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
    collections::HashSet,
    fs,
    ops::Deref,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopierConfig {
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

impl Default for CopierConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for CopierConfig {}

#[derive(Default, Clone, Serialize, Deserialize)]
struct Clipboard {
    items: HashSet<PathBuf>,
}

impl Clipboard {
    fn add(&mut self, path: PathBuf) {
        self.items.insert(path);
    }

    fn remove(&mut self, path: &Path) {
        self.items.remove(path);
    }

    fn clear(&mut self) {
        self.items.clear();
    }

    fn get_items(&self) -> &HashSet<PathBuf> {
        &self.items
    }

    fn save(&self) -> Result<(), String> {
        let clipboard_file = Self::get_clipboard_path();
        if let Some(parent) = clipboard_file.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
        }
        let content =
            serde_json::to_string(self).map_err(|e| format!("Failed to serialize: {}", e))?;
        fs::write(&clipboard_file, content).map_err(|e| format!("Failed to write file: {}", e))?;
        Ok(())
    }

    fn load() -> Self {
        let clipboard_file = Self::get_clipboard_path();
        if let Ok(content) = fs::read_to_string(clipboard_file) {
            if let Ok(clipboard) = serde_json::from_str(&content) {
                return clipboard;
            }
        }
        Self::default()
    }

    fn get_clipboard_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("lla")
            .join("cp_clipboard.json")
    }
}

lazy_static! {
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "add",
            "add [path]",
            "Add files/directories to clipboard from current or specified directory",
            vec![
                "lla plugin --name file_copier --action add",
                "lla plugin --name file_copier --action add --args /path/to/dir"
            ],
            |args| FileCopierPlugin::add_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "copy-all",
            "copy-all [target_path]",
            "Copy all items from clipboard to current or specified directory",
            vec![
                "lla plugin --name file_copier --action copy-all",
                "lla plugin --name file_copier --action copy-all --args /path/to/target"
            ],
            |args| FileCopierPlugin::copy_all_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "copy-partial",
            "copy-partial [target_path]",
            "Copy selected items from clipboard to current or specified directory",
            vec![
                "lla plugin --name file_copier --action copy-partial",
                "lla plugin --name file_copier --action copy-partial --args /path/to/target"
            ],
            |args| FileCopierPlugin::copy_partial_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "clear",
            "clear",
            "Clear the clipboard",
            vec!["lla plugin --name file_copier --action clear"],
            |_| FileCopierPlugin::clear_action()
        );

        lla_plugin_utils::define_action!(
            registry,
            "show",
            "show",
            "Show clipboard contents",
            vec!["lla plugin --name file_copier --action show"],
            |_| FileCopierPlugin::show_action()
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name file_copier --action help"],
            |_| FileCopierPlugin::help_action()
        );

        registry
    });
}

pub struct FileCopierPlugin {
    base: BasePlugin<CopierConfig>,
    clipboard: Clipboard,
}

impl FileCopierPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        Self {
            base: BasePlugin::with_name(plugin_name),
            clipboard: Clipboard::load(),
        }
    }

    fn get_directory(path_arg: Option<&str>) -> Result<PathBuf, String> {
        match path_arg {
            Some(path) => Ok(PathBuf::from(path)),
            None => std::env::current_dir()
                .map_err(|e| format!("Failed to get current directory: {}", e)),
        }
    }

    fn copy_directory_recursively(src: &Path, dst: &Path) -> Result<(), String> {
        if !src.is_dir() {
            return Err(format!("{} is not a directory", src.display()));
        }

        fs::create_dir_all(dst)
            .map_err(|e| format!("Failed to create directory {}: {}", dst.display(), e))?;

        for entry in fs::read_dir(src)
            .map_err(|e| format!("Failed to read directory {}: {}", src.display(), e))?
        {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();
            let dest_path = dst.join(path.file_name().ok_or("Invalid file name")?);

            if path.is_dir() {
                Self::copy_directory_recursively(&path, &dest_path)?;
            } else {
                fs::copy(&path, &dest_path).map_err(|e| {
                    format!(
                        "Failed to copy {} to {}: {}",
                        path.display(),
                        dest_path.display(),
                        e
                    )
                })?;
            }
        }

        Ok(())
    }

    fn copy_item(src: &Path, dst: &Path) -> Result<(), String> {
        if src.is_dir() {
            Self::copy_directory_recursively(src, dst)
        } else {
            fs::copy(src, dst).map_err(|e| {
                format!(
                    "Failed to copy {} to {}: {}",
                    src.display(),
                    dst.display(),
                    e
                )
            })?;
            Ok(())
        }
    }

    fn add_action(args: &[String]) -> Result<(), String> {
        let mut plugin = Self::new();
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

        let items: Vec<String> = entries.iter().map(|p| p.display().to_string()).collect();

        let theme = LlaDialoguerTheme::default();
        let selections = MultiSelect::with_theme(&theme)
            .with_prompt("Select items to copy")
            .items(&items)
            .interact()
            .map_err(|e| format!("Failed to show selector: {}", e))?;

        if selections.is_empty() {
            println!("{} No items selected", "Info:".bright_blue());
            return Ok(());
        }

        for &idx in &selections {
            plugin.clipboard.add(entries[idx].clone());
        }

        plugin.clipboard.save()?;

        println!(
            "{} Added {} items to clipboard from {}",
            "Success:".bright_green(),
            selections.len(),
            dir.display()
        );
        Ok(())
    }

    fn copy_all_action(args: &[String]) -> Result<(), String> {
        let plugin = Self::new();
        let items = plugin.clipboard.get_items().clone();

        if items.is_empty() {
            println!("{} Clipboard is empty", "Info:".bright_blue());
            return Ok(());
        }

        let target_dir = Self::get_directory(args.get(0).map(|s| s.as_str()))?;

        for path in items {
            let new_path = target_dir.join(path.file_name().ok_or("Invalid file name")?);
            Self::copy_item(&path, &new_path)?;
        }

        println!(
            "{} Copied all items to {}",
            "Success:".bright_green(),
            target_dir.display()
        );
        Ok(())
    }

    fn copy_partial_action(args: &[String]) -> Result<(), String> {
        let plugin = Self::new();
        let items: Vec<PathBuf> = plugin.clipboard.get_items().iter().cloned().collect();

        if items.is_empty() {
            println!("{} Clipboard is empty", "Info:".bright_blue());
            return Ok(());
        }

        let item_strings: Vec<String> = items.iter().map(|p| p.display().to_string()).collect();

        let theme = LlaDialoguerTheme::default();
        let selections = MultiSelect::with_theme(&theme)
            .with_prompt("Select items to copy")
            .items(&item_strings)
            .interact()
            .map_err(|e| format!("Failed to show selector: {}", e))?;

        if selections.is_empty() {
            println!("{} No items selected", "Info:".bright_blue());
            return Ok(());
        }

        let target_dir = Self::get_directory(args.get(0).map(|s| s.as_str()))?;

        for &idx in &selections {
            let path = &items[idx];
            let new_path = target_dir.join(path.file_name().ok_or("Invalid file name")?);
            Self::copy_item(path, &new_path)?;
        }

        println!(
            "{} Copied {} items to {}",
            "Success:".bright_green(),
            selections.len(),
            target_dir.display()
        );
        Ok(())
    }

    fn clear_action() -> Result<(), String> {
        let mut plugin = Self::new();
        plugin.clipboard.clear();
        plugin.clipboard.save()?;
        println!("{} Clipboard cleared", "Success:".bright_green());
        Ok(())
    }

    fn show_action() -> Result<(), String> {
        let mut plugin = Self::new();
        let items = plugin.clipboard.get_items();

        if items.is_empty() {
            println!("{} Clipboard is empty", "Info:".bright_blue());
            return Ok(());
        }

        let item_strings: Vec<String> = items.iter().map(|p| p.display().to_string()).collect();

        let theme = LlaDialoguerTheme::default();
        let selections = MultiSelect::with_theme(&theme)
            .with_prompt("Select items to remove from clipboard")
            .items(&item_strings)
            .interact()
            .map_err(|e| format!("Failed to show selector: {}", e))?;

        if !selections.is_empty() {
            let items_vec: Vec<PathBuf> = items.iter().cloned().collect();
            for &idx in &selections {
                plugin.clipboard.remove(&items_vec[idx]);
            }
            plugin.clipboard.save()?;
            println!(
                "{} Removed {} items from clipboard",
                "Success:".bright_green(),
                selections.len()
            );
        }

        Ok(())
    }

    fn help_action() -> Result<(), String> {
        let mut help = HelpFormatter::new("File Copier".to_string());
        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Copy files and directories using a clipboard-like interface".to_string(),
            vec![],
        );

        help.add_section("Basic Commands".to_string())
            .add_command(
                "add [path]".to_string(),
                "Add files/directories to clipboard from current or specified directory"
                    .to_string(),
                vec![
                    "lla plugin --name file_copier --action add".to_string(),
                    "lla plugin --name file_copier --action add --args /path/to/dir".to_string(),
                ],
            )
            .add_command(
                "copy-all [target_path]".to_string(),
                "Copy all items from clipboard to current or specified directory".to_string(),
                vec![
                    "lla plugin --name file_copier --action copy-all".to_string(),
                    "lla plugin --name file_copier --action copy-all --args /path/to/target"
                        .to_string(),
                ],
            )
            .add_command(
                "copy-partial [target_path]".to_string(),
                "Copy selected items from clipboard to current or specified directory".to_string(),
                vec![
                    "lla plugin --name file_copier --action copy-partial".to_string(),
                    "lla plugin --name file_copier --action copy-partial --args /path/to/target"
                        .to_string(),
                ],
            );

        help.add_section("Clipboard Management".to_string())
            .add_command(
                "show".to_string(),
                "Show clipboard contents with option to remove items".to_string(),
                vec!["lla plugin --name file_copier --action show".to_string()],
            )
            .add_command(
                "clear".to_string(),
                "Clear the clipboard".to_string(),
                vec!["lla plugin --name file_copier --action clear".to_string()],
            );

        println!(
            "{}",
            BoxComponent::new(help.render(&CopierConfig::default().colors))
                .style(BoxStyle::Minimal)
                .padding(1)
                .render()
        );
        Ok(())
    }
}

impl Deref for FileCopierPlugin {
    type Target = CopierConfig;

    fn deref(&self) -> &Self::Target {
        self.base.config()
    }
}

impl Plugin for FileCopierPlugin {
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
                };
                self.encode_response(response)
            }
            Err(e) => self.encode_error(&e),
        }
    }
}

impl ConfigurablePlugin for FileCopierPlugin {
    type Config = CopierConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for FileCopierPlugin {}

lla_plugin_interface::declare_plugin!(FileCopierPlugin);

impl Default for FileCopierPlugin {
    fn default() -> Self {
        Self::new()
    }
}
