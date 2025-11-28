use arboard::Clipboard;
use colored::Colorize;
use dialoguer::Select;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, LlaDialoguerTheme},
    BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaywallService {
    TwelveFt,
    ArchiveIs,
    RemovePaywall,
    Freedium,
    GoogleCache,
}

impl PaywallService {
    fn base_url(&self) -> &'static str {
        match self {
            PaywallService::TwelveFt => "https://12ft.io",
            PaywallService::ArchiveIs => "https://archive.is",
            PaywallService::RemovePaywall => "https://www.removepaywall.com",
            PaywallService::Freedium => "https://freedium.cfd",
            PaywallService::GoogleCache => "https://webcache.googleusercontent.com/search?q=cache:",
        }
    }

    fn name(&self) -> &'static str {
        match self {
            PaywallService::TwelveFt => "12ft.io",
            PaywallService::ArchiveIs => "archive.is",
            PaywallService::RemovePaywall => "RemovePaywall.com",
            PaywallService::Freedium => "Freedium",
            PaywallService::GoogleCache => "Google Cache",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            PaywallService::TwelveFt => "Bypasses most paywalls by showing Google's cached version",
            PaywallService::ArchiveIs => "Archives and displays cached versions of web pages",
            PaywallService::RemovePaywall => "Removes paywalls from news articles",
            PaywallService::Freedium => "Free access to Medium articles",
            PaywallService::GoogleCache => "Google's web cache (if available)",
        }
    }

    fn build_url(&self, original_url: &str) -> String {
        match self {
            PaywallService::TwelveFt => format!("{}/{}", self.base_url(), original_url),
            PaywallService::ArchiveIs => format!("{}/?run=1&url={}", self.base_url(), original_url),
            PaywallService::RemovePaywall => format!("{}/{}", self.base_url(), original_url),
            PaywallService::Freedium => format!("{}/{}", self.base_url(), original_url),
            PaywallService::GoogleCache => format!("{}{}", self.base_url(), original_url),
        }
    }

    fn best_for(&self) -> &'static str {
        match self {
            PaywallService::TwelveFt => "News sites (NYT, WaPo, etc.)",
            PaywallService::ArchiveIs => "General articles, creating permanent archives",
            PaywallService::RemovePaywall => "News articles, subscriptions sites",
            PaywallService::Freedium => "Medium articles only",
            PaywallService::GoogleCache => "Recently indexed pages",
        }
    }

    fn all() -> Vec<PaywallService> {
        vec![
            PaywallService::TwelveFt,
            PaywallService::ArchiveIs,
            PaywallService::RemovePaywall,
            PaywallService::Freedium,
            PaywallService::GoogleCache,
        ]
    }
}

impl Default for PaywallService {
    fn default() -> Self {
        PaywallService::TwelveFt
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageHistoryEntry {
    pub url: String,
    pub service: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovePaywallConfig {
    #[serde(default)]
    pub default_service: PaywallService,
    #[serde(default = "default_true")]
    pub auto_open_browser: bool,
    #[serde(default = "default_true")]
    pub copy_to_clipboard: bool,
    #[serde(default = "default_true")]
    pub remember_history: bool,
    #[serde(default)]
    pub usage_history: Vec<UsageHistoryEntry>,
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
    colors.insert("url".to_string(), "bright_blue".to_string());
    colors.insert("service".to_string(), "bright_magenta".to_string());
    colors
}

impl Default for RemovePaywallConfig {
    fn default() -> Self {
        Self {
            default_service: PaywallService::default(),
            auto_open_browser: true,
            copy_to_clipboard: true,
            remember_history: true,
            usage_history: Vec::new(),
            max_history_size: 50,
            colors: default_colors(),
        }
    }
}

impl PluginConfig for RemovePaywallConfig {}

pub struct RemovePaywallPlugin {
    base: BasePlugin<RemovePaywallConfig>,
}

impl RemovePaywallPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[RemovePaywallPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn add_to_history(&mut self, url: &str, service: &str) {
        let config = self.base.config_mut();

        if !config.remember_history {
            return;
        }

        // Remove duplicate if exists
        config.usage_history.retain(|e| e.url != url);

        config.usage_history.insert(
            0,
            UsageHistoryEntry {
                url: url.to_string(),
                service: service.to_string(),
                timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            },
        );

        if config.usage_history.len() > config.max_history_size {
            config.usage_history.truncate(config.max_history_size);
        }

        if let Err(e) = self.base.save_config() {
            eprintln!("Failed to save history: {}", e);
        }
    }

    fn validate_url(url: &str) -> Result<String, String> {
        let url = url.trim();

        if url.is_empty() {
            return Err("URL cannot be empty".to_string());
        }

        // Add https:// if no scheme present
        let url = if !url.starts_with("http://") && !url.starts_with("https://") {
            format!("https://{}", url)
        } else {
            url.to_string()
        };

        // Validate URL
        Url::parse(&url).map_err(|e| format!("Invalid URL: {}", e))?;

        Ok(url)
    }

    fn get_clipboard_url(&self) -> Option<String> {
        match Clipboard::new() {
            Ok(mut clipboard) => clipboard.get_text().ok().and_then(|text| {
                let text = text.trim();
                // Check if it looks like a URL
                if text.starts_with("http://")
                    || text.starts_with("https://")
                    || text.contains(".")
                {
                    Self::validate_url(text).ok()
                } else {
                    None
                }
            }),
            Err(_) => None,
        }
    }

    fn open_in_browser(url: &str) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(target_os = "windows")]
        let cmd = "start";

        std::process::Command::new(cmd)
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;

        Ok(())
    }

    fn copy_to_clipboard(text: &str) -> Result<(), String> {
        match Clipboard::new() {
            Ok(mut clipboard) => {
                clipboard
                    .set_text(text)
                    .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;
                Ok(())
            }
            Err(e) => Err(format!("Failed to access clipboard: {}", e)),
        }
    }

    fn remove_paywall(&mut self, url: &str, service: Option<PaywallService>) -> Result<(), String> {
        let url = Self::validate_url(url)?;
        let service = service.unwrap_or(self.base.config().default_service);

        let bypass_url = service.build_url(&url);

        println!(
            "\n{} {}",
            "🔓".bright_cyan(),
            "Removing Paywall".bright_cyan()
        );
        println!("{}", "─".repeat(60).bright_black());

        let mut list = List::new().style(BoxStyle::Minimal).key_width(15);

        list.add_item(
            KeyValue::new("Original URL", &url)
                .key_color("bright_cyan")
                .value_color("bright_blue")
                .key_width(15)
                .render(),
        );

        list.add_item(
            KeyValue::new("Service", service.name())
                .key_color("bright_cyan")
                .value_color("bright_magenta")
                .key_width(15)
                .render(),
        );

        list.add_item(
            KeyValue::new("Bypass URL", &bypass_url)
                .key_color("bright_cyan")
                .value_color("bright_green")
                .key_width(15)
                .render(),
        );

        println!("{}", list.render());

        // Copy to clipboard if enabled
        if self.base.config().copy_to_clipboard {
            if Self::copy_to_clipboard(&bypass_url).is_ok() {
                println!(
                    "   {} {}",
                    "📋".bright_green(),
                    "Bypass URL copied to clipboard!".bright_green()
                );
            }
        }

        // Open in browser if enabled
        if self.base.config().auto_open_browser {
            Self::open_in_browser(&bypass_url)?;
            println!(
                "   {} {}",
                "🌐".bright_green(),
                "Opened in browser!".bright_green()
            );
        }

        // Save to history
        self.add_to_history(&url, service.name());

        println!("{}", "─".repeat(60).bright_black());
        Ok(())
    }

    fn remove_paywall_clipboard(&mut self) -> Result<(), String> {
        let url = self.get_clipboard_url().ok_or_else(|| {
            "No valid URL found in clipboard. Copy a URL first.".to_string()
        })?;

        println!(
            "{} Found URL in clipboard: {}",
            "ℹ️ ".bright_cyan(),
            url.bright_blue()
        );

        self.remove_paywall(&url, None)
    }

    fn choose_service(&mut self, url: Option<&str>) -> Result<(), String> {
        let url = if let Some(u) = url {
            Self::validate_url(u)?
        } else {
            self.get_clipboard_url().ok_or_else(|| {
                "No URL provided and no valid URL found in clipboard.".to_string()
            })?
        };

        let theme = LlaDialoguerTheme::default();

        let services = PaywallService::all();
        let items: Vec<String> = services
            .iter()
            .map(|s| {
                format!(
                    "{} {} - {}",
                    match s {
                        PaywallService::TwelveFt => "🏗️ ",
                        PaywallService::ArchiveIs => "📚",
                        PaywallService::RemovePaywall => "🗞️ ",
                        PaywallService::Freedium => "📝",
                        PaywallService::GoogleCache => "🔍",
                    },
                    s.name(),
                    s.best_for()
                )
            })
            .collect();

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Choose a service", "🔓".bright_cyan()))
            .items(&items)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        self.remove_paywall(&url, Some(services[selection]))
    }

    fn try_all_services(&mut self, url: Option<&str>) -> Result<(), String> {
        let url = if let Some(u) = url {
            Self::validate_url(u)?
        } else {
            self.get_clipboard_url().ok_or_else(|| {
                "No URL provided and no valid URL found in clipboard.".to_string()
            })?
        };

        println!(
            "\n{} {}",
            "🔓".bright_cyan(),
            "Opening URL with all services".bright_cyan()
        );
        println!("{}", "─".repeat(60).bright_black());
        println!(
            "   {} Original: {}",
            "📎".bright_cyan(),
            url.bright_blue()
        );
        println!("{}", "─".repeat(60).bright_black());

        for service in PaywallService::all() {
            let bypass_url = service.build_url(&url);
            println!(
                "   {} {}: {}",
                "→".bright_green(),
                service.name().bright_magenta(),
                bypass_url.bright_black()
            );
        }

        println!("{}", "─".repeat(60).bright_black());

        // Add to history
        self.add_to_history(&url, "all services");

        Ok(())
    }

    fn list_services(&self) -> Result<(), String> {
        println!(
            "\n{} {}",
            "🔓".bright_cyan(),
            "Available Paywall Removal Services".bright_cyan()
        );
        println!("{}", "─".repeat(70).bright_black());

        for service in PaywallService::all() {
            let is_default = service == self.base.config().default_service;
            let default_badge = if is_default {
                " [default]".bright_yellow().to_string()
            } else {
                String::new()
            };

            println!(
                "\n   {} {}{}",
                "•".bright_cyan(),
                service.name().bright_white(),
                default_badge
            );
            println!("     {}", service.description().bright_black());
            println!(
                "     {} {}",
                "Best for:".bright_cyan(),
                service.best_for().bright_black()
            );
            println!(
                "     {} {}",
                "URL:".bright_cyan(),
                service.base_url().bright_blue()
            );
        }

        println!("\n{}", "─".repeat(70).bright_black());
        Ok(())
    }

    fn show_history(&self) -> Result<(), String> {
        let history = &self.base.config().usage_history;

        if history.is_empty() {
            println!(
                "{} {}",
                "ℹ️ ".bright_cyan(),
                "No usage history available".bright_cyan()
            );
            return Ok(());
        }

        println!(
            "\n{} {}",
            "📜".bright_cyan(),
            "Paywall Removal History".bright_cyan()
        );
        println!("{}", "─".repeat(80).bright_black());

        for entry in history.iter().take(20) {
            println!(
                "   {} {} via {} - {}",
                "•".bright_cyan(),
                entry.url.bright_blue(),
                entry.service.bright_magenta(),
                entry.timestamp.bright_black()
            );
        }

        println!("{}", "─".repeat(80).bright_black());
        Ok(())
    }

    fn clear_history(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let confirm = dialoguer::Confirm::with_theme(&theme)
            .with_prompt("Are you sure you want to clear all history?")
            .default(false)
            .interact()
            .map_err(|e| format!("Failed to get confirmation: {}", e))?;

        if confirm {
            self.base.config_mut().usage_history.clear();
            self.base.save_config()?;
            println!(
                "{} {}",
                "✓".bright_green(),
                "History cleared!".bright_green()
            );
        } else {
            println!(
                "{} {}",
                "ℹ️ ".bright_cyan(),
                "Operation cancelled".bright_cyan()
            );
        }

        Ok(())
    }

    fn set_default_service(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let services = PaywallService::all();
        let current_default = self.base.config().default_service;
        let default_index = services.iter().position(|&s| s == current_default).unwrap_or(0);

        let items: Vec<String> = services
            .iter()
            .map(|s| format!("{} - {}", s.name(), s.description()))
            .collect();

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Set default service", "⚙️ ".bright_cyan()))
            .items(&items)
            .default(default_index)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        self.base.config_mut().default_service = services[selection];
        self.base.save_config()?;

        println!(
            "{} Default service set to: {}",
            "✓".bright_green(),
            services[selection].name().bright_magenta()
        );

        Ok(())
    }

    fn configure_preferences(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let options = vec![
            format!(
                "Auto-open in browser: {}",
                if self.base.config().auto_open_browser {
                    "✓ Enabled".bright_green()
                } else {
                    "✗ Disabled".bright_red()
                }
            ),
            format!(
                "Copy to clipboard: {}",
                if self.base.config().copy_to_clipboard {
                    "✓ Enabled".bright_green()
                } else {
                    "✗ Disabled".bright_red()
                }
            ),
            format!(
                "Remember history: {}",
                if self.base.config().remember_history {
                    "✓ Enabled".bright_green()
                } else {
                    "✗ Disabled".bright_red()
                }
            ),
            format!(
                "Default service: {}",
                self.base.config().default_service.name().bright_magenta()
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
                let config = self.base.config_mut();
                config.auto_open_browser = !config.auto_open_browser;
                self.base.save_config()?;
                println!(
                    "{} Auto-open in browser: {}",
                    "✓".bright_green(),
                    if self.base.config().auto_open_browser {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
            }
            1 => {
                let config = self.base.config_mut();
                config.copy_to_clipboard = !config.copy_to_clipboard;
                self.base.save_config()?;
                println!(
                    "{} Copy to clipboard: {}",
                    "✓".bright_green(),
                    if self.base.config().copy_to_clipboard {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
            }
            2 => {
                let config = self.base.config_mut();
                config.remember_history = !config.remember_history;
                self.base.save_config()?;
                println!(
                    "{} Remember history: {}",
                    "✓".bright_green(),
                    if self.base.config().remember_history {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
            }
            3 => {
                self.set_default_service()?;
            }
            4 => {}
            _ => unreachable!(),
        }

        Ok(())
    }

    fn interactive_menu(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let options = vec![
            "🔓 Remove Paywall (from clipboard)",
            "🔍 Choose Service",
            "📋 Try All Services",
            "📜 View History",
            "📚 List Services",
            "⚙️  Preferences",
            "❓ Help",
            "← Exit",
        ];

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Remove Paywall Menu", "🔓".bright_cyan()))
            .items(&options)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        match selection {
            0 => self.remove_paywall_clipboard(),
            1 => self.choose_service(None),
            2 => self.try_all_services(None),
            3 => self.show_history(),
            4 => self.list_services(),
            5 => self.configure_preferences(),
            6 => self.show_help(),
            7 => Ok(()),
            _ => unreachable!(),
        }
    }

    fn show_help(&self) -> Result<(), String> {
        let colors = self.base.config().colors.clone();

        let mut help = HelpFormatter::new("Remove Paywall Plugin".to_string());

        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Remove paywalls from URLs using various services like 12ft.io, archive.is, etc."
                .to_string(),
            vec![],
        );

        help.add_section("Actions".to_string())
            .add_command(
                "remove <url>".to_string(),
                "Remove paywall from a URL using default service".to_string(),
                vec!["lla plugin remove_paywall remove https://example.com/article".to_string()],
            )
            .add_command(
                "clipboard".to_string(),
                "Remove paywall from URL in clipboard".to_string(),
                vec!["lla plugin remove_paywall clipboard".to_string()],
            )
            .add_command(
                "choose [url]".to_string(),
                "Choose which service to use".to_string(),
                vec!["lla plugin remove_paywall choose".to_string()],
            )
            .add_command(
                "try-all [url]".to_string(),
                "Generate links for all services".to_string(),
                vec!["lla plugin remove_paywall try-all".to_string()],
            )
            .add_command(
                "services".to_string(),
                "List available services".to_string(),
                vec!["lla plugin remove_paywall services".to_string()],
            )
            .add_command(
                "history".to_string(),
                "View usage history".to_string(),
                vec!["lla plugin remove_paywall history".to_string()],
            )
            .add_command(
                "preferences".to_string(),
                "Configure plugin preferences".to_string(),
                vec!["lla plugin remove_paywall preferences".to_string()],
            )
            .add_command(
                "menu".to_string(),
                "Interactive menu".to_string(),
                vec!["lla plugin remove_paywall menu".to_string()],
            )
            .add_command(
                "help".to_string(),
                "Show this help information".to_string(),
                vec!["lla plugin remove_paywall help".to_string()],
            );

        help.add_section("Services".to_string())
            .add_command(
                "12ft.io".to_string(),
                "Best for news sites (default)".to_string(),
                vec![],
            )
            .add_command(
                "archive.is".to_string(),
                "Archives and caches web pages".to_string(),
                vec![],
            )
            .add_command(
                "RemovePaywall.com".to_string(),
                "News articles and subscription sites".to_string(),
                vec![],
            )
            .add_command(
                "Freedium".to_string(),
                "Medium articles only".to_string(),
                vec![],
            )
            .add_command(
                "Google Cache".to_string(),
                "Recently indexed pages".to_string(),
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

impl Plugin for RemovePaywallPlugin {
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
                    PluginRequest::FormatField(_entry, _format) => {
                        PluginResponse::FormattedField(None)
                    }
                    PluginRequest::PerformAction(action, args) => {
                        let result = match action.as_str() {
                            "remove" => {
                                if let Some(url) = args.first() {
                                    self.remove_paywall(url, None)
                                } else {
                                    self.remove_paywall_clipboard()
                                }
                            }
                            "clipboard" => self.remove_paywall_clipboard(),
                            "choose" => {
                                let url = args.first().map(|s| s.as_str());
                                self.choose_service(url)
                            }
                            "try-all" => {
                                let url = args.first().map(|s| s.as_str());
                                self.try_all_services(url)
                            }
                            "services" | "list" => self.list_services(),
                            "history" => self.show_history(),
                            "clear-history" => self.clear_history(),
                            "preferences" | "config" => self.configure_preferences(),
                            "set-default" => self.set_default_service(),
                            "menu" => self.interactive_menu(),
                            "help" => self.show_help(),
                            // Service-specific shortcuts
                            "12ft" => {
                                let url = args.first().map(|s| s.as_str());
                                if let Some(u) = url {
                                    self.remove_paywall(u, Some(PaywallService::TwelveFt))
                                } else {
                                    let clipboard_url = self.get_clipboard_url();
                                    if let Some(u) = clipboard_url {
                                        self.remove_paywall(&u, Some(PaywallService::TwelveFt))
                                    } else {
                                        Err("No URL provided".to_string())
                                    }
                                }
                            }
                            "archive" => {
                                let url = args.first().map(|s| s.as_str());
                                if let Some(u) = url {
                                    self.remove_paywall(u, Some(PaywallService::ArchiveIs))
                                } else {
                                    let clipboard_url = self.get_clipboard_url();
                                    if let Some(u) = clipboard_url {
                                        self.remove_paywall(&u, Some(PaywallService::ArchiveIs))
                                    } else {
                                        Err("No URL provided".to_string())
                                    }
                                }
                            }
                            "freedium" => {
                                let url = args.first().map(|s| s.as_str());
                                if let Some(u) = url {
                                    self.remove_paywall(u, Some(PaywallService::Freedium))
                                } else {
                                    let clipboard_url = self.get_clipboard_url();
                                    if let Some(u) = clipboard_url {
                                        self.remove_paywall(&u, Some(PaywallService::Freedium))
                                    } else {
                                        Err("No URL provided".to_string())
                                    }
                                }
                            }
                            _ => Err(format!(
                                "Unknown action: '{}'\n\n\
                                Available actions:\n  \
                                • remove     - Remove paywall from URL\n  \
                                • clipboard  - Remove paywall from clipboard URL\n  \
                                • choose     - Choose service interactively\n  \
                                • try-all    - Generate links for all services\n  \
                                • services   - List available services\n  \
                                • history    - View usage history\n  \
                                • 12ft       - Use 12ft.io service\n  \
                                • archive    - Use archive.is service\n  \
                                • freedium   - Use Freedium for Medium\n  \
                                • menu       - Interactive menu\n  \
                                • help       - Show help\n\n\
                                Example: lla plugin remove_paywall clipboard",
                                action
                            )),
                        };
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::GetAvailableActions => {
                        use lla_plugin_interface::ActionInfo;
                        PluginResponse::AvailableActions(vec![
                            ActionInfo {
                                name: "remove".to_string(),
                                usage: "remove <url>".to_string(),
                                description: "Remove paywall from URL".to_string(),
                                examples: vec![
                                    "lla plugin remove_paywall remove https://example.com".to_string()
                                ],
                            },
                            ActionInfo {
                                name: "clipboard".to_string(),
                                usage: "clipboard".to_string(),
                                description: "Remove paywall from clipboard URL".to_string(),
                                examples: vec!["lla plugin remove_paywall clipboard".to_string()],
                            },
                            ActionInfo {
                                name: "choose".to_string(),
                                usage: "choose [url]".to_string(),
                                description: "Choose service interactively".to_string(),
                                examples: vec!["lla plugin remove_paywall choose".to_string()],
                            },
                            ActionInfo {
                                name: "try-all".to_string(),
                                usage: "try-all [url]".to_string(),
                                description: "Generate links for all services".to_string(),
                                examples: vec!["lla plugin remove_paywall try-all".to_string()],
                            },
                            ActionInfo {
                                name: "services".to_string(),
                                usage: "services".to_string(),
                                description: "List available services".to_string(),
                                examples: vec!["lla plugin remove_paywall services".to_string()],
                            },
                            ActionInfo {
                                name: "history".to_string(),
                                usage: "history".to_string(),
                                description: "View usage history".to_string(),
                                examples: vec!["lla plugin remove_paywall history".to_string()],
                            },
                            ActionInfo {
                                name: "12ft".to_string(),
                                usage: "12ft [url]".to_string(),
                                description: "Use 12ft.io service".to_string(),
                                examples: vec!["lla plugin remove_paywall 12ft".to_string()],
                            },
                            ActionInfo {
                                name: "archive".to_string(),
                                usage: "archive [url]".to_string(),
                                description: "Use archive.is service".to_string(),
                                examples: vec!["lla plugin remove_paywall archive".to_string()],
                            },
                            ActionInfo {
                                name: "freedium".to_string(),
                                usage: "freedium [url]".to_string(),
                                description: "Use Freedium for Medium articles".to_string(),
                                examples: vec!["lla plugin remove_paywall freedium".to_string()],
                            },
                            ActionInfo {
                                name: "menu".to_string(),
                                usage: "menu".to_string(),
                                description: "Interactive menu".to_string(),
                                examples: vec!["lla plugin remove_paywall menu".to_string()],
                            },
                            ActionInfo {
                                name: "help".to_string(),
                                usage: "help".to_string(),
                                description: "Show help information".to_string(),
                                examples: vec!["lla plugin remove_paywall help".to_string()],
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

impl Default for RemovePaywallPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for RemovePaywallPlugin {
    type Config = RemovePaywallConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for RemovePaywallPlugin {}

lla_plugin_interface::declare_plugin!(RemovePaywallPlugin);

