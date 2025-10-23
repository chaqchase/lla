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

/// Represents a Google account for creating meetings
/// Despite the name, this now stores Google account info (for authuser parameter)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserProfile {
    /// Display name for the account (e.g., "Work", "Personal")
    pub name: String,
    /// Google account identifier - email address or account number (e.g., "user@example.com" or "0")
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

    fn get_browser_name(&self) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        {
            let script = r#"
                set browserList to {"Google Chrome", "Safari", "Brave Browser", "Arc", "Firefox", "Microsoft Edge", "Chromium", "Opera"}
                tell application "System Events"
                    set frontApp to name of first application process whose frontmost is true
                    repeat with browserName in browserList
                        if frontApp contains browserName then
                            return browserName
                        end if
                    end repeat
                    return ""
                end tell
            "#;

            let output = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .output()
                .map_err(|e| format!("Failed to run AppleScript: {}", e))?;

            let browser = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if browser.is_empty() {
                Err("No supported browser is currently active".to_string())
            } else {
                Ok(browser)
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err("Browser detection is only supported on macOS".to_string())
        }
    }

    fn get_active_tab_url(&self, browser: &str) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        {
            let script = if browser.contains("Arc") {
                r#"
                    tell application "Arc"
                        if (count of windows) > 0 then
                            tell front window
                                if (count of tabs) > 0 then
                                    return URL of active tab
                                end if
                            end tell
                        end if
                    end tell
                    return ""
                "#
            } else if browser.contains("Firefox") {
                // Firefox doesn't support direct AppleScript URL access
                // Fall back to copying from URL bar
                r#"
                    tell application "Firefox" to activate
                    delay 0.2
                    tell application "System Events"
                        keystroke "l" using command down
                        delay 0.1
                        keystroke "c" using command down
                    end tell
                    delay 0.1
                    return the clipboard
                "#
            } else {
                // Works for Chrome, Safari, Brave, Edge, etc.
                &format!(
                    r#"
                    tell application "{}"
                        if (count of windows) > 0 then
                            return URL of active tab of front window
                        end if
                    end tell
                    return ""
                "#,
                    browser
                )
            };

            let output = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .output()
                .map_err(|e| format!("Failed to get tab URL: {}", e))?;

            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if url.is_empty() {
                Err("Could not get active tab URL".to_string())
            } else {
                Ok(url)
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = browser;
            Err("Tab URL detection is only supported on macOS".to_string())
        }
    }

    fn wait_for_meet_url(&self, browser: &str, max_attempts: u32) -> Result<String, String> {
        use std::thread;
        use std::time::Duration;

        for attempt in 0..max_attempts {
            if attempt > 0 {
                thread::sleep(Duration::from_millis(500));
            }

            match self.get_active_tab_url(browser) {
                Ok(url) => {
                    // Check if it's a meet.google.com URL and not the /new endpoint
                    if url.contains("meet.google.com") && !url.ends_with("/new") {
                        return Ok(url);
                    }
                }
                Err(_) if attempt < max_attempts - 1 => {
                    // Continue trying
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        Err("Timeout waiting for Google Meet to generate the meeting URL".to_string())
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

    fn open_meet_new_url(&self, authuser: Option<&str>) -> Result<(), String> {
        let url = if let Some(user) = authuser {
            format!("https://meet.google.com/new?authuser={}", user)
        } else {
            "https://meet.google.com/new".to_string()
        };

        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(&url)
                .spawn()
                .map_err(|e| format!("Failed to open browser: {}", e))?;
        }

        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(&url)
                .spawn()
                .map_err(|e| format!("Failed to open browser: {}", e))?;
        }

        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(&["/C", "start", &url])
                .spawn()
                .map_err(|e| format!("Failed to open browser: {}", e))?;
        }

        Ok(())
    }

    fn open_existing_meet_url(&self, url: &str) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(url)
                .spawn()
                .map_err(|e| format!("Failed to open browser: {}", e))?;
        }

        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(url)
                .spawn()
                .map_err(|e| format!("Failed to open browser: {}", e))?;
        }

        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(&["/C", "start", url])
                .spawn()
                .map_err(|e| format!("Failed to open browser: {}", e))?;
        }

        Ok(())
    }

    fn create_meet(&mut self, authuser: Option<&str>) -> Result<(), String> {
        println!("{} Creating Google Meet room...", "Info:".bright_cyan());

        // Open the /new endpoint
        if let Some(user) = authuser {
            println!(
                "{} Opening with Google account: {}",
                "Info:".bright_cyan(),
                user.bright_magenta()
            );
        }

        self.open_meet_new_url(authuser)?;

        // Wait a moment for the browser to open
        std::thread::sleep(std::time::Duration::from_millis(1000));

        // Get the browser name and wait for the real URL
        println!(
            "{} Waiting for Google Meet to generate the link...",
            "Info:".bright_cyan()
        );

        let browser = self.get_browser_name()?;
        let meet_url = self.wait_for_meet_url(&browser, 20)?; // 20 attempts = 10 seconds max

        println!("{} {}", "Link:".bright_blue(), meet_url.bright_yellow());

        // Copy to clipboard if enabled
        if self.base.config().auto_copy_link {
            self.copy_to_clipboard(&meet_url)?;
            println!("{} Link copied to clipboard!", "Success:".bright_green());
        }

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
                "{} No Google accounts configured. Use preferences to add accounts.",
                "Warning:".bright_yellow()
            );
            println!(
                "{} Creating meeting with default account...",
                "Info:".bright_cyan()
            );
            return self.create_meet(None);
        }

        let theme = LlaDialoguerTheme::default();
        let profile_names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Select Google account", "👤".bright_cyan()))
            .items(&profile_names)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show account selector: {}", e))?;

        let authuser = profiles[selection].profile_path.clone();
        self.create_meet(Some(&authuser))
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
                self.open_existing_meet_url(link)?;
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
            "➕ Add Google account",
            "📋 List accounts",
            "🗑️  Remove account",
            "← Back",
        ];

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Manage Google Accounts", "👤".bright_cyan()))
            .items(&actions)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        match selection {
            0 => {
                // Add account
                let name: String = Input::with_theme(&theme)
                    .with_prompt("Account name (e.g., 'Work', 'Personal')")
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;

                let authuser: String = Input::with_theme(&theme)
                    .with_prompt(
                        "Google account (email or account number, e.g., 'user@example.com' or '0')",
                    )
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;

                let config = self.base.config_mut();
                config.browser_profiles.push(BrowserProfile {
                    name,
                    profile_path: authuser,
                });

                self.base.save_config()?;
                println!(
                    "{} Google account added successfully!",
                    "Success:".bright_green()
                );
            }
            1 => {
                // List accounts
                let profiles = &self.base.config().browser_profiles;
                if profiles.is_empty() {
                    println!("{} No Google accounts configured", "Info:".bright_cyan());
                } else {
                    println!("\n{} Google Accounts:", "📋".bright_cyan());
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
                // Remove account
                let profiles = &self.base.config().browser_profiles;
                if profiles.is_empty() {
                    println!("{} No accounts to remove", "Info:".bright_cyan());
                    return Ok(());
                }

                let profile_names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();

                let selection = Select::with_theme(&theme)
                    .with_prompt(format!("{} Select account to remove", "🗑️ ".bright_cyan()))
                    .items(&profile_names)
                    .default(0)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                self.base.config_mut().browser_profiles.remove(selection);
                self.base.save_config()?;
                println!("{} Google account removed!", "Success:".bright_green());
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
            "Create real Google Meet rooms by opening meet.google.com/new and capturing the generated link. Supports multiple Google accounts."
                .to_string(),
            vec![],
        );

        help.add_section("Actions".to_string())
            .add_command(
                "create".to_string(),
                "Create a new meeting room with default account".to_string(),
                vec!["create".to_string()],
            )
            .add_command(
                "create-with-profile".to_string(),
                "Create meeting with a specific Google account".to_string(),
                vec!["create-with-profile".to_string()],
            )
            .add_command(
                "history".to_string(),
                "View, copy, or reopen past meeting links".to_string(),
                vec!["history".to_string()],
            )
            .add_command(
                "profiles".to_string(),
                "Manage Google accounts (add/list/remove)".to_string(),
                vec!["profiles".to_string()],
            )
            .add_command(
                "preferences".to_string(),
                "Configure auto-copy and history settings".to_string(),
                vec!["preferences".to_string()],
            )
            .add_command(
                "help".to_string(),
                "Show this help information".to_string(),
                vec!["help".to_string()],
            );

        help.add_section("How It Works".to_string()).add_command(
            "".to_string(),
            "Opens https://meet.google.com/new in your browser, waits for Google to generate the real meeting link, then captures and saves it. Browser detection works on macOS."
                .to_string(),
            vec![],
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
                    PluginRequest::GetAvailableActions => {
                        use lla_plugin_interface::ActionInfo;
                        PluginResponse::AvailableActions(vec![
                            ActionInfo {
                                name: "create".to_string(),
                                usage: "create".to_string(),
                                description: "Create a Google Meet".to_string(),
                                examples: vec!["lla plugin google_meet create".to_string()],
                            },
                            ActionInfo {
                                name: "create-with-profile".to_string(),
                                usage: "create-with-profile".to_string(),
                                description: "Create a Meet with profile selection".to_string(),
                                examples: vec![
                                    "lla plugin google_meet create-with-profile".to_string()
                                ],
                            },
                            ActionInfo {
                                name: "history".to_string(),
                                usage: "history".to_string(),
                                description: "Manage Meet history".to_string(),
                                examples: vec!["lla plugin google_meet history".to_string()],
                            },
                            ActionInfo {
                                name: "profiles".to_string(),
                                usage: "profiles".to_string(),
                                description: "Manage profiles".to_string(),
                                examples: vec!["lla plugin google_meet profiles".to_string()],
                            },
                            ActionInfo {
                                name: "preferences".to_string(),
                                usage: "preferences".to_string(),
                                description: "Configure preferences".to_string(),
                                examples: vec!["lla plugin google_meet preferences".to_string()],
                            },
                            ActionInfo {
                                name: "help".to_string(),
                                usage: "help".to_string(),
                                description: "Show help information".to_string(),
                                examples: vec!["lla plugin google_meet help".to_string()],
                            },
                        ])
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
