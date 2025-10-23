use colored::Colorize;
use dialoguer::Confirm;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlushHistoryEntry {
    pub timestamp: String,
    pub os: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlushDnsConfig {
    #[serde(default = "default_true")]
    pub confirm_before_flush: bool,
    #[serde(default = "default_true")]
    pub show_verbose_output: bool,
    #[serde(default)]
    pub flush_history: Vec<FlushHistoryEntry>,
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
    colors
}

impl Default for FlushDnsConfig {
    fn default() -> Self {
        Self {
            confirm_before_flush: true,
            show_verbose_output: true,
            flush_history: Vec::new(),
            max_history_size: 50,
            colors: default_colors(),
        }
    }
}

impl PluginConfig for FlushDnsConfig {}

pub struct FlushDnsPlugin {
    base: BasePlugin<FlushDnsConfig>,
}

impl FlushDnsPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[FlushDnsPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn add_to_history(&mut self, success: bool) {
        let config = self.base.config_mut();

        let os = if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "linux") {
            "Linux"
        } else if cfg!(target_os = "windows") {
            "Windows"
        } else {
            "Unknown"
        };

        config.flush_history.insert(
            0,
            FlushHistoryEntry {
                timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                os: os.to_string(),
                success,
            },
        );

        // Trim history if needed
        if config.flush_history.len() > config.max_history_size {
            config.flush_history.truncate(config.max_history_size);
        }

        if let Err(e) = self.base.save_config() {
            eprintln!("Failed to save flush history: {}", e);
        }
    }

    fn get_os_name(&self) -> &str {
        if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "linux") {
            "Linux"
        } else if cfg!(target_os = "windows") {
            "Windows"
        } else {
            "Unknown"
        }
    }

    fn flush_dns_cache(&mut self) -> Result<(), String> {
        let confirm_before_flush = self.base.config().confirm_before_flush;
        let show_verbose_output = self.base.config().show_verbose_output;

        // Show confirmation if enabled
        if confirm_before_flush {
            let theme = LlaDialoguerTheme::default();
            let confirm = Confirm::with_theme(&theme)
                .with_prompt(format!(
                    "{} Are you sure you want to flush the DNS cache?",
                    "⚠️ ".bright_yellow()
                ))
                .default(true)
                .interact()
                .map_err(|e| format!("Failed to get confirmation: {}", e))?;

            if !confirm {
                println!("{} Operation cancelled", "Info:".bright_blue());
                return Ok(());
            }
        }

        println!(
            "{} Flushing DNS cache on {}...",
            "Info:".bright_cyan(),
            self.get_os_name().bright_yellow()
        );

        let result = self.execute_flush_command();

        match result {
            Ok(output) => {
                self.add_to_history(true);
                println!(
                    "{} DNS cache flushed successfully!",
                    "Success:".bright_green()
                );

                if show_verbose_output && !output.is_empty() {
                    println!("\n{} Output:", "Info:".bright_cyan());
                    println!("{}", output.bright_black());
                }
                Ok(())
            }
            Err(e) => {
                self.add_to_history(false);
                Err(format!("Failed to flush DNS cache: {}", e))
            }
        }
    }

    fn execute_flush_command(&self) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        {
            // macOS command
            let output = std::process::Command::new("sudo")
                .args(&["dscacheutil", "-flushcache"])
                .output()
                .map_err(|e| format!("Failed to execute command: {}", e))?;

            // Also flush mDNSResponder
            let _ = std::process::Command::new("sudo")
                .args(&["killall", "-HUP", "mDNSResponder"])
                .output();

            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).to_string())
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Linux command - try systemd-resolve first, then nscd
            let result = std::process::Command::new("sudo")
                .args(&["systemd-resolve", "--flush-caches"])
                .output();

            if let Ok(output) = result {
                if output.status.success() {
                    return Ok(String::from_utf8_lossy(&output.stdout).to_string());
                }
            }

            // Try nscd as fallback
            let result = std::process::Command::new("sudo")
                .args(&["systemctl", "restart", "nscd"])
                .output();

            if let Ok(output) = result {
                if output.status.success() {
                    return Ok(String::from_utf8_lossy(&output.stdout).to_string());
                }
            }

            Err("Failed to flush DNS. Try: sudo systemd-resolve --flush-caches or sudo systemctl restart nscd".to_string())
        }

        #[cfg(target_os = "windows")]
        {
            // Windows command
            let output = std::process::Command::new("ipconfig")
                .args(&["/flushdns"])
                .output()
                .map_err(|e| format!("Failed to execute command: {}", e))?;

            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).to_string())
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Err("DNS cache flushing is not supported on this operating system".to_string())
        }
    }

    fn view_history(&self) -> Result<(), String> {
        let history = &self.base.config().flush_history;

        if history.is_empty() {
            println!("{} No flush history available", "Info:".bright_cyan());
            return Ok(());
        }

        println!("\n{} DNS Flush History:", "📜".bright_cyan());
        println!("─{}─", "─".repeat(70));

        for (i, entry) in history.iter().enumerate().take(20) {
            let status = if entry.success {
                "✓".bright_green()
            } else {
                "✗".bright_red()
            };

            println!(
                " {} {} {} {}",
                status,
                entry.timestamp.bright_black(),
                format!("[{}]", entry.os).bright_yellow(),
                if entry.success {
                    "Success".bright_green()
                } else {
                    "Failed".bright_red()
                }
            );

            if i == 19 && history.len() > 20 {
                println!(
                    " {} {} more entries...",
                    "...".bright_black(),
                    (history.len() - 20).to_string().bright_yellow()
                );
            }
        }

        println!("─{}─\n", "─".repeat(70));

        // Show statistics
        let total = history.len();
        let successful = history.iter().filter(|e| e.success).count();
        let failed = total - successful;

        println!("{} Statistics:", "📊".bright_cyan());
        println!(" • Total flushes: {}", total.to_string().bright_yellow());
        println!(" • Successful: {}", successful.to_string().bright_green());
        if failed > 0 {
            println!(" • Failed: {}", failed.to_string().bright_red());
        }

        Ok(())
    }

    fn clear_history(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let confirm = Confirm::with_theme(&theme)
            .with_prompt("Are you sure you want to clear the flush history?")
            .default(false)
            .interact()
            .map_err(|e| format!("Failed to get confirmation: {}", e))?;

        if confirm {
            self.base.config_mut().flush_history.clear();
            self.base.save_config()?;
            println!("{} Flush history cleared!", "Success:".bright_green());
        } else {
            println!("{} Operation cancelled", "Info:".bright_blue());
        }

        Ok(())
    }

    fn configure_preferences(&mut self) -> Result<(), String> {
        use dialoguer::{Input, Select};
        let theme = LlaDialoguerTheme::default();

        let options = vec![
            format!(
                "Confirm before flush: {}",
                if self.base.config().confirm_before_flush {
                    "✓ Enabled".bright_green()
                } else {
                    "✗ Disabled".bright_red()
                }
            ),
            format!(
                "Show verbose output: {}",
                if self.base.config().show_verbose_output {
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
                    config.confirm_before_flush = !config.confirm_before_flush;
                    config.confirm_before_flush
                };
                self.base.save_config()?;
                println!(
                    "{} Confirm before flush: {}",
                    "Success:".bright_green(),
                    if new_value { "enabled" } else { "disabled" }
                );
            }
            1 => {
                let new_value = {
                    let config = self.base.config_mut();
                    config.show_verbose_output = !config.show_verbose_output;
                    config.show_verbose_output
                };
                self.base.save_config()?;
                println!(
                    "{} Verbose output: {}",
                    "Success:".bright_green(),
                    if new_value { "enabled" } else { "disabled" }
                );
            }
            2 => {
                let input: usize = Input::with_theme(&theme)
                    .with_prompt("Enter max history size")
                    .default(self.base.config().max_history_size)
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;

                let config = self.base.config_mut();
                config.max_history_size = input;

                if config.flush_history.len() > config.max_history_size {
                    config.flush_history.truncate(config.max_history_size);
                }

                self.base.save_config()?;
                println!(
                    "{} Max history size set to: {}",
                    "Success:".bright_green(),
                    input
                );
            }
            3 => {
                // Back - do nothing
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    fn show_help(&self) -> Result<(), String> {
        let confirm_before_flush = self.base.config().confirm_before_flush;
        let show_verbose = self.base.config().show_verbose_output;
        let colors = self.base.config().colors.clone();

        let mut help = HelpFormatter::new("Flush DNS Plugin".to_string());

        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            format!(
                "Flush DNS cache on {} with history tracking and configurable options.",
                self.get_os_name()
            ),
            vec![],
        );

        help.add_section("Actions".to_string())
            .add_command(
                "flush".to_string(),
                "Flush the DNS cache".to_string(),
                vec!["flush".to_string()],
            )
            .add_command(
                "history".to_string(),
                "View flush history".to_string(),
                vec!["history".to_string()],
            )
            .add_command(
                "clear-history".to_string(),
                "Clear flush history".to_string(),
                vec!["clear-history".to_string()],
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

        help.add_section("Preferences".to_string())
            .add_command(
                "Confirm Before Flush".to_string(),
                format!(
                    "Currently: {}",
                    if confirm_before_flush {
                        "✓ Enabled".bright_green().to_string()
                    } else {
                        "✗ Disabled".bright_red().to_string()
                    }
                ),
                vec![],
            )
            .add_command(
                "Show Verbose Output".to_string(),
                format!(
                    "Currently: {}",
                    if show_verbose {
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

impl Plugin for FlushDnsPlugin {
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
                            "flush" => self.flush_dns_cache(),
                            "history" => self.view_history(),
                            "clear-history" => self.clear_history(),
                            "preferences" => self.configure_preferences(),
                            "help" => self.show_help(),
                            _ => Err(format!("Unknown action: {}", action)),
                        };
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::GetAvailableActions => {
                        use lla_plugin_interface::ActionInfo;
                        PluginResponse::AvailableActions(vec![
                            ActionInfo { name: "flush".to_string(), usage: "flush".to_string(), description: "Flush DNS cache".to_string(), examples: vec!["lla plugin flush_dns flush".to_string()] },
                            ActionInfo { name: "history".to_string(), usage: "history".to_string(), description: "View flush history".to_string(), examples: vec!["lla plugin flush_dns history".to_string()] },
                            ActionInfo { name: "clear-history".to_string(), usage: "clear-history".to_string(), description: "Clear flush history".to_string(), examples: vec!["lla plugin flush_dns clear-history".to_string()] },
                            ActionInfo { name: "preferences".to_string(), usage: "preferences".to_string(), description: "Configure preferences".to_string(), examples: vec!["lla plugin flush_dns preferences".to_string()] },
                            ActionInfo { name: "help".to_string(), usage: "help".to_string(), description: "Show help information".to_string(), examples: vec!["lla plugin flush_dns help".to_string()] },
                        ])
                    }
                };
                self.encode_response(response)
            }
            Err(e) => self.encode_error(&e),
        }
    }
}

impl Default for FlushDnsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for FlushDnsPlugin {
    type Config = FlushDnsConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for FlushDnsPlugin {}

lla_plugin_interface::declare_plugin!(FlushDnsPlugin);
