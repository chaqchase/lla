use arboard::Clipboard;
use colored::Colorize;
use dialoguer::{Input, Select};
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserProfile {
    pub name: String,
    pub profile_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetHistoryEntry {
    pub link: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleMeetConfig {
    #[serde(default = "default_true")]
    pub auto_copy_link: bool,
    #[serde(default)]
    pub browser_profiles: Vec<BrowserProfile>,
    #[serde(default)]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub meet_history: Vec<MeetHistoryEntry>,
    #[serde(default = "default_max_history")]
    pub max_history_size: usize,
    #[serde(default = "default_colors")]
    pub colors: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

fn default_max_history() -> usize {
    50
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_cyan".to_string());
    colors.insert("warning".to_string(), "bright_yellow".to_string());
    colors.insert("error".to_string(), "bright_red".to_string());
    colors.insert("link".to_string(), "bright_blue".to_string());
    colors
}

impl Default for GoogleMeetConfig {
    fn default() -> Self {
        Self {
            auto_copy_link: true,
            browser_profiles: Vec::new(),
            default_profile: None,
            meet_history: Vec::new(),
            max_history_size: 50,
            colors: default_colors(),
        }
    }
}

impl PluginConfig for GoogleMeetConfig {}

pub struct GoogleMeetPlugin {
    base: BasePlugin<GoogleMeetConfig>,
}

impl GoogleMeetPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[GoogleMeetPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn generate_meet_link(&self) -> String {
        // Google Meet links follow the pattern: https://meet.google.com/xxx-xxxx-xxx
        // Generate a random code (10 characters with dashes)
        let code = uuid::Uuid::new_v4()
            .to_string()
            .replace("-", "")
            .chars()
            .take(10)
            .collect::<String>();

        // Format as xxx-xxxx-xxx
        format!("{}-{}-{}", &code[0..3], &code[3..7], &code[7..10])
    }

    fn add_to_history(&mut self, link: &str) {
        let config = self.base.config_mut();

        // Remove duplicate if exists
        config.meet_history.retain(|e| e.link != link);

        // Add new entry at the beginning
        config.meet_history.insert(
            0,
            MeetHistoryEntry {
                link: link.to_string(),
                timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            },
        );

        // Trim history if needed
        if config.meet_history.len() > config.max_history_size {
            config.meet_history.truncate(config.max_history_size);
        }

        if let Err(e) = self.base.save_config() {
            eprintln!("Failed to save meeting history: {}", e);
        }
    }

    fn copy_to_clipboard(&self, text: &str) -> Result<(), String> {
        match Clipboard::new() {
            Ok(mut clipboard) => clipboard
                .set_text(text)
                .map_err(|e| format!("Failed to copy to clipboard: {}", e)),
            Err(e) => Err(format!("Failed to access clipboard: {}", e)),
        }
    }

    fn open_meet_url(&self, url: &str, profile: Option<&str>) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut cmd = std::process::Command::new("open");
            if let Some(prof) = profile {
                // Try to use Chrome profile on macOS
                cmd.args(&[
                    "-a",
                    "Google Chrome",
                    "--args",
                    &format!("--profile-directory={}", prof),
                ]);
            }
            cmd
        };

        #[cfg(target_os = "linux")]
        let mut command = {
            let mut cmd = std::process::Command::new("xdg-open");
            if let Some(prof) = profile {
                // For Linux, we'd need to handle browser-specific profile flags
                cmd.env("BROWSER_PROFILE", prof);
            }
            cmd
        };

        #[cfg(target_os = "windows")]
        let mut command = {
            let mut cmd = std::process::Command::new("cmd");
            cmd.args(&["/C", "start"]);
            if let Some(prof) = profile {
                cmd.arg(&format!("--profile-directory={}", prof));
            }
            cmd
        };

        command
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;

        Ok(())
    }

    fn create_meet(&mut self, profile: Option<&str>) -> Result<(), String> {
        let meet_code = self.generate_meet_link();
        let meet_url = format!("https://meet.google.com/{}", meet_code);

        println!("{} Creating Google Meet room...", "Info:".bright_cyan());
        println!("{} {}", "Link:".bright_blue(), meet_url.bright_yellow());

        // Copy to clipboard if enabled
        if self.base.config().auto_copy_link {
            self.copy_to_clipboard(&meet_url)?;
            println!("{} Link copied to clipboard!", "Success:".bright_green());
        }

        // Open in browser
        if let Some(prof) = profile {
            println!(
                "{} Opening with profile: {}",
                "Info:".bright_cyan(),
                prof.bright_magenta()
            );
        }

        self.open_meet_url(&meet_url, profile)?;
        self.add_to_history(&meet_url);

        println!(
            "{} Meeting room created successfully!",
            "Success:".bright_green()
        );

        Ok(())
    }

    fn create_meet_with_profile(&mut self) -> Result<(), String> {
        let profiles = self.base.config().browser_profiles.clone();

        if profiles.is_empty() {
            println!(
                "{} No browser profiles configured. Use preferences to add profiles.",
                "Warning:".bright_yellow()
            );
            println!(
                "{} Creating meeting with default browser...",
                "Info:".bright_cyan()
            );
            return self.create_meet(None);
        }

        let theme = LlaDialoguerTheme::default();
        let profile_names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Select browser profile", "👤".bright_cyan()))
            .items(&profile_names)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show profile selector: {}", e))?;

        let profile_path = profiles[selection].profile_path.clone();
        self.create_meet(Some(&profile_path))
    }

    fn manage_history(&mut self) -> Result<(), String> {
        let history = self.base.config().meet_history.clone();

        if history.is_empty() {
            println!("{} No meeting history available", "Info:".bright_cyan());
            return Ok(());
        }

        let theme = LlaDialoguerTheme::default();

        let items: Vec<String> = history
            .iter()
            .map(|entry| {
                format!(
                    "{} {} {}",
                    "🔗".bright_cyan(),
                    entry.link,
                    format!("({})", entry.timestamp).bright_black()
                )
            })
            .collect();

        let actions = vec![
            "📋 Copy selected link",
            "🌐 Open selected link",
            "🗑️  Clear all history",
        ];

        let action_selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Choose action", "⚡".bright_cyan()))
            .items(&actions)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show action menu: {}", e))?;

        match action_selection {
            0 => {
                // Copy selected link
                let selection = Select::with_theme(&theme)
                    .with_prompt(format!("{} Select link to copy", "📋".bright_cyan()))
                    .items(&items)
                    .default(0)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                let link = &history[selection].link;
                self.copy_to_clipboard(link)?;
                println!("{} Link copied to clipboard!", "Success:".bright_green());
            }
            1 => {
                // Open selected link
                let selection = Select::with_theme(&theme)
                    .with_prompt(format!("{} Select link to open", "🌐".bright_cyan()))
                    .items(&items)
                    .default(0)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                let link = &history[selection].link;
                self.open_meet_url(link, None)?;
                println!("{} Meeting opened!", "Success:".bright_green());
            }
            2 => {
                // Clear all history
                let confirm: bool = dialoguer::Confirm::with_theme(&theme)
                    .with_prompt("Are you sure you want to clear all meeting history?")
                    .default(false)
                    .interact()
                    .map_err(|e| format!("Failed to get confirmation: {}", e))?;

                if confirm {
                    self.base.config_mut().meet_history.clear();
                    self.base.save_config()?;
                    println!("{} All meeting history cleared!", "Success:".bright_green());
                } else {
                    println!("{} Operation cancelled", "Info:".bright_blue());
                }
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    fn manage_profiles(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let actions = vec![
            "➕ Add browser profile",
            "📋 List profiles",
            "🗑️  Remove profile",
            "← Back",
        ];

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Manage Browser Profiles", "👤".bright_cyan()))
            .items(&actions)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        match selection {
            0 => {
                // Add profile
                let name: String = Input::with_theme(&theme)
                    .with_prompt("Profile name")
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;

                let path: String = Input::with_theme(&theme)
                    .with_prompt("Profile path (e.g., 'Default', 'Profile 1')")
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;

                let config = self.base.config_mut();
                config.browser_profiles.push(BrowserProfile {
                    name,
                    profile_path: path,
                });

                self.base.save_config()?;
                println!("{} Profile added successfully!", "Success:".bright_green());
            }
            1 => {
                // List profiles
                let profiles = &self.base.config().browser_profiles;
                if profiles.is_empty() {
                    println!("{} No profiles configured", "Info:".bright_cyan());
                } else {
                    println!("\n{} Browser Profiles:", "📋".bright_cyan());
                    for (i, profile) in profiles.iter().enumerate() {
                        println!(
                            " {}. {} {}",
                            (i + 1).to_string().bright_yellow(),
                            profile.name.bright_magenta(),
                            format!("({})", profile.profile_path).bright_black()
                        );
                    }
                }
            }
            2 => {
                // Remove profile
                let profiles = &self.base.config().browser_profiles;
                if profiles.is_empty() {
                    println!("{} No profiles to remove", "Info:".bright_cyan());
                    return Ok(());
                }

                let profile_names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();

                let selection = Select::with_theme(&theme)
                    .with_prompt(format!("{} Select profile to remove", "🗑️ ".bright_cyan()))
                    .items(&profile_names)
                    .default(0)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                self.base.config_mut().browser_profiles.remove(selection);
                self.base.save_config()?;
                println!("{} Profile removed!", "Success:".bright_green());
            }
            3 => {
                // Back - do nothing
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    fn configure_preferences(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let options = vec![
            format!(
                "Auto-copy link: {}",
                if self.base.config().auto_copy_link {
                    "✓ Enabled".bright_green()
                } else {
                    "✗ Disabled".bright_red()
                }
            ),
            format!(
                "Max History Size: {}",
                self.base
                    .config()
                    .max_history_size
                    .to_string()
                    .bright_yellow()
            ),
            "← Back".to_string(),
        ];

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Configure Preferences", "⚙️ ".bright_cyan()))
            .items(&options)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show preferences: {}", e))?;

        match selection {
            0 => {
                let new_value = {
                    let config = self.base.config_mut();
                    config.auto_copy_link = !config.auto_copy_link;
                    config.auto_copy_link
                };
                self.base.save_config()?;
                println!(
                    "{} Auto-copy link: {}",
                    "Success:".bright_green(),
                    if new_value { "enabled" } else { "disabled" }
                );
            }
            1 => {
                let input: usize = Input::with_theme(&theme)
                    .with_prompt("Enter max history size")
                    .default(self.base.config().max_history_size)
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;

                let config = self.base.config_mut();
                config.max_history_size = input;

                // Trim history if needed
                if config.meet_history.len() > config.max_history_size {
                    config.meet_history.truncate(config.max_history_size);
                }

                self.base.save_config()?;
                println!(
                    "{} Max history size set to: {}",
                    "Success:".bright_green(),
                    input
                );
            }
            2 => {
                // Back - do nothing
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    fn show_help(&self) -> Result<(), String> {
        let auto_copy = self.base.config().auto_copy_link;
        let colors = self.base.config().colors.clone();

        let mut help = HelpFormatter::new("Google Meet Plugin".to_string());

        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Create Google Meet rooms and manage meeting links with browser profile support."
                .to_string(),
            vec![],
        );

        help.add_section("Actions".to_string())
            .add_command(
                "create".to_string(),
                "Create a new meeting room".to_string(),
                vec!["create".to_string()],
            )
            .add_command(
                "create-with-profile".to_string(),
                "Create meeting with specified browser profile".to_string(),
                vec!["create-with-profile".to_string()],
            )
            .add_command(
                "history".to_string(),
                "Manage meeting history".to_string(),
                vec!["history".to_string()],
            )
            .add_command(
                "profiles".to_string(),
                "Manage browser profiles".to_string(),
                vec!["profiles".to_string()],
            )
            .add_command(
                "preferences".to_string(),
                "Configure plugin preferences".to_string(),
                vec!["preferences".to_string()],
            )
            .add_command(
                "help".to_string(),
                "Show this help information".to_string(),
                vec!["help".to_string()],
            );

        help.add_section("Preferences".to_string()).add_command(
            "Auto-copy Link".to_string(),
            format!(
                "Currently: {}",
                if auto_copy {
                    "✓ Enabled".bright_green().to_string()
                } else {
                    "✗ Disabled".bright_red().to_string()
                }
            ),
            vec![],
        );

        println!(
            "{}",
            BoxComponent::new(help.render(&colors))
                .style(BoxStyle::Minimal)
                .padding(1)
                .render()
        );

        Ok(())
    }
}

impl Plugin for GoogleMeetPlugin {
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
                    PluginRequest::Decorate(entry) => {
                        // This plugin doesn't decorate entries
                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::FormatField(_entry, _format) => {
                        // This plugin doesn't format fields
                        PluginResponse::FormattedField(None)
                    }
                    PluginRequest::PerformAction(action, _args) => {
                        let result = match action.as_str() {
                            "create" => self.create_meet(None),
                            "create-with-profile" => self.create_meet_with_profile(),
                            "history" => self.manage_history(),
                            "profiles" => self.manage_profiles(),
                            "preferences" => self.configure_preferences(),
                            "help" => self.show_help(),
                            _ => Err(format!("Unknown action: {}", action)),
                        };
                        PluginResponse::ActionResult(result)
                    }
                };
                self.encode_response(response)
            }
            Err(e) => self.encode_error(&e),
        }
    }
}

impl Default for GoogleMeetPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for GoogleMeetPlugin {
    type Config = GoogleMeetConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for GoogleMeetPlugin {}

lla_plugin_interface::declare_plugin!(GoogleMeetPlugin);
