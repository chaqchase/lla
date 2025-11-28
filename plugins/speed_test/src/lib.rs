use colored::Colorize;
use dialoguer::Select;
use indicatif::{ProgressBar, ProgressStyle};
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, LlaDialoguerTheme},
    BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const TEST_URLS: &[(&str, &str)] = &[
    ("Cloudflare", "https://speed.cloudflare.com/__down?bytes=10000000"),
    ("Google", "https://www.google.com/images/branding/googlelogo/2x/googlelogo_color_272x92dp.png"),
    ("GitHub", "https://github.githubassets.com/images/modules/logos_page/GitHub-Mark.png"),
];

const PING_URLS: &[(&str, &str)] = &[
    ("Cloudflare DNS", "https://1.1.1.1/"),
    ("Google DNS", "https://8.8.8.8/"),
    ("OpenDNS", "https://208.67.222.222/"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTestResult {
    pub timestamp: String,
    pub download_mbps: f64,
    pub latency_ms: u64,
    pub server: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTestConfig {
    #[serde(default = "default_true")]
    pub remember_history: bool,
    #[serde(default)]
    pub test_history: Vec<SpeedTestResult>,
    #[serde(default = "default_max_history")]
    pub max_history_size: usize,
    #[serde(default = "default_colors")]
    pub colors: HashMap<String, String>,
    #[serde(default = "default_test_size")]
    pub test_size_mb: usize,
}

fn default_true() -> bool {
    true
}

fn default_max_history() -> usize {
    50
}

fn default_test_size() -> usize {
    10
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_cyan".to_string());
    colors.insert("warning".to_string(), "bright_yellow".to_string());
    colors.insert("error".to_string(), "bright_red".to_string());
    colors.insert("speed_fast".to_string(), "bright_green".to_string());
    colors.insert("speed_medium".to_string(), "bright_yellow".to_string());
    colors.insert("speed_slow".to_string(), "bright_red".to_string());
    colors
}

impl Default for SpeedTestConfig {
    fn default() -> Self {
        Self {
            remember_history: true,
            test_history: Vec::new(),
            max_history_size: 50,
            colors: default_colors(),
            test_size_mb: 10,
        }
    }
}

impl PluginConfig for SpeedTestConfig {}

pub struct SpeedTestPlugin {
    base: BasePlugin<SpeedTestConfig>,
    http: Client,
}

impl SpeedTestPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| Client::new());
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
            http: client,
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[SpeedTestPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn add_to_history(&mut self, result: SpeedTestResult) {
        let config = self.base.config_mut();

        if !config.remember_history {
            return;
        }

        config.test_history.insert(0, result);

        if config.test_history.len() > config.max_history_size {
            config.test_history.truncate(config.max_history_size);
        }

        if let Err(e) = self.base.save_config() {
            eprintln!("Failed to save test history: {}", e);
        }
    }

    fn test_latency(&self, url: &str) -> Result<u64, String> {
        let start = Instant::now();
        self.http
            .head(url)
            .send()
            .map_err(|e| format!("Failed to ping: {}", e))?;
        Ok(start.elapsed().as_millis() as u64)
    }

    fn test_download(&self, url: &str, server_name: &str) -> Result<(f64, u64), String> {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message(format!("Testing download speed from {}...", server_name));
        pb.enable_steady_tick(Duration::from_millis(100));

        // First, measure latency
        let latency = self.test_latency(url).unwrap_or(0);

        // Then test download speed
        let start = Instant::now();
        let response = self
            .http
            .get(url)
            .send()
            .map_err(|e| format!("Failed to download: {}", e))?;

        let bytes = response
            .bytes()
            .map_err(|e| format!("Failed to read response: {}", e))?;

        let elapsed = start.elapsed();
        let bytes_downloaded = bytes.len() as f64;
        let seconds = elapsed.as_secs_f64();

        // Calculate speed in Mbps (megabits per second)
        let mbps = (bytes_downloaded * 8.0) / (seconds * 1_000_000.0);

        pb.finish_and_clear();

        Ok((mbps, latency))
    }

    fn run_speed_test(&mut self) -> Result<(), String> {
        println!(
            "\n{} {}",
            "🚀".bright_cyan(),
            "Running Internet Speed Test...".bright_cyan()
        );
        println!("{}", "─".repeat(50).bright_black());

        // Test latency first
        println!(
            "\n{} {}",
            "📡".bright_yellow(),
            "Testing latency...".bright_yellow()
        );

        let mut best_latency = u64::MAX;
        let mut best_latency_server = String::new();

        for (name, url) in PING_URLS {
            match self.test_latency(url) {
                Ok(ms) => {
                    let color = if ms < 50 {
                        "bright_green"
                    } else if ms < 100 {
                        "bright_yellow"
                    } else {
                        "bright_red"
                    };
                    let ms_str = format!("{}ms", ms);
                    let colored_ms = match color {
                        "bright_green" => ms_str.bright_green(),
                        "bright_yellow" => ms_str.bright_yellow(),
                        _ => ms_str.bright_red(),
                    };
                    println!("   {} {}: {}", "•".bright_cyan(), name, colored_ms);
                    if ms < best_latency {
                        best_latency = ms;
                        best_latency_server = name.to_string();
                    }
                }
                Err(e) => {
                    println!(
                        "   {} {}: {}",
                        "•".bright_red(),
                        name,
                        format!("Failed - {}", e).bright_red()
                    );
                }
            }
        }

        // Test download speed
        println!(
            "\n{} {}",
            "⬇️ ".bright_green(),
            "Testing download speed...".bright_green()
        );

        let mut best_speed = 0.0f64;
        let mut best_server = String::new();
        let mut test_latency = best_latency;

        for (name, url) in TEST_URLS {
            match self.test_download(url, name) {
                Ok((mbps, latency)) => {
                    let color = if mbps > 50.0 {
                        "bright_green"
                    } else if mbps > 10.0 {
                        "bright_yellow"
                    } else {
                        "bright_red"
                    };
                    let speed_str = format!("{:.2} Mbps", mbps);
                    let colored_speed = match color {
                        "bright_green" => speed_str.bright_green(),
                        "bright_yellow" => speed_str.bright_yellow(),
                        _ => speed_str.bright_red(),
                    };
                    println!("   {} {}: {}", "•".bright_cyan(), name, colored_speed);
                    if mbps > best_speed {
                        best_speed = mbps;
                        best_server = name.to_string();
                        test_latency = latency;
                    }
                }
                Err(e) => {
                    println!(
                        "   {} {}: {}",
                        "•".bright_red(),
                        name,
                        format!("Failed - {}", e).bright_red()
                    );
                }
            }
        }

        // Summary
        println!("\n{}", "─".repeat(50).bright_black());
        println!("{} {}", "📊".bright_cyan(), "Results Summary".bright_cyan());
        println!("{}", "─".repeat(50).bright_black());

        let mut list = List::new().style(BoxStyle::Minimal).key_width(20);

        let speed_color = if best_speed > 50.0 {
            "bright_green"
        } else if best_speed > 10.0 {
            "bright_yellow"
        } else {
            "bright_red"
        };

        let latency_color = if best_latency < 50 {
            "bright_green"
        } else if best_latency < 100 {
            "bright_yellow"
        } else {
            "bright_red"
        };

        list.add_item(
            KeyValue::new("Download Speed", format!("{:.2} Mbps", best_speed))
                .key_color("bright_cyan")
                .value_color(speed_color)
                .key_width(20)
                .render(),
        );

        list.add_item(
            KeyValue::new("Best Server", &best_server)
                .key_color("bright_cyan")
                .value_color("bright_white")
                .key_width(20)
                .render(),
        );

        list.add_item(
            KeyValue::new("Latency", format!("{}ms", best_latency))
                .key_color("bright_cyan")
                .value_color(latency_color)
                .key_width(20)
                .render(),
        );

        list.add_item(
            KeyValue::new("Fastest DNS", &best_latency_server)
                .key_color("bright_cyan")
                .value_color("bright_white")
                .key_width(20)
                .render(),
        );

        println!("{}", list.render());

        // Speed rating
        let rating = if best_speed > 100.0 {
            ("🚀 Excellent!", "bright_green")
        } else if best_speed > 50.0 {
            ("✨ Very Good", "bright_green")
        } else if best_speed > 25.0 {
            ("👍 Good", "bright_yellow")
        } else if best_speed > 10.0 {
            ("🐢 Moderate", "bright_yellow")
        } else {
            ("🐌 Slow", "bright_red")
        };

        let rating_str = match rating.1 {
            "bright_green" => rating.0.bright_green(),
            "bright_yellow" => rating.0.bright_yellow(),
            _ => rating.0.bright_red(),
        };
        println!("\n   {} {}", "Rating:".bright_cyan(), rating_str);

        // Save to history
        let result = SpeedTestResult {
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            download_mbps: best_speed,
            latency_ms: test_latency,
            server: best_server,
        };
        self.add_to_history(result);

        println!("\n{} {}", "✓".bright_green(), "Test completed!".bright_green());

        Ok(())
    }

    fn quick_test(&mut self) -> Result<(), String> {
        println!(
            "\n{} {}",
            "⚡".bright_yellow(),
            "Running Quick Speed Test...".bright_yellow()
        );

        // Just test one server
        let (name, url) = TEST_URLS[0];
        match self.test_download(url, name) {
            Ok((mbps, latency)) => {
                let speed_color = if mbps > 50.0 {
                    "bright_green"
                } else if mbps > 10.0 {
                    "bright_yellow"
                } else {
                    "bright_red"
                };

                let speed_str = format!("{:.2} Mbps", mbps);
                let colored_speed = match speed_color {
                    "bright_green" => speed_str.bright_green(),
                    "bright_yellow" => speed_str.bright_yellow(),
                    _ => speed_str.bright_red(),
                };

                println!(
                    "\n   {} Download: {} | Latency: {}ms",
                    "📊".bright_cyan(),
                    colored_speed,
                    latency
                );

                let result = SpeedTestResult {
                    timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    download_mbps: mbps,
                    latency_ms: latency,
                    server: name.to_string(),
                };
                self.add_to_history(result);

                Ok(())
            }
            Err(e) => Err(format!("Test failed: {}", e)),
        }
    }

    fn show_history(&self) -> Result<(), String> {
        let history = &self.base.config().test_history;

        if history.is_empty() {
            println!(
                "{} {}",
                "ℹ️ ".bright_cyan(),
                "No speed test history available".bright_cyan()
            );
            return Ok(());
        }

        println!(
            "\n{} {}",
            "📜".bright_cyan(),
            "Speed Test History".bright_cyan()
        );
        println!("{}", "─".repeat(70).bright_black());
        println!(
            "{:^20} {:^15} {:^12} {:^15}",
            "Timestamp".bright_white(),
            "Download".bright_white(),
            "Latency".bright_white(),
            "Server".bright_white()
        );
        println!("{}", "─".repeat(70).bright_black());

        for result in history.iter().take(20) {
            let speed_str = format!("{:.2} Mbps", result.download_mbps);
            let speed_colored = if result.download_mbps > 50.0 {
                speed_str.bright_green()
            } else if result.download_mbps > 10.0 {
                speed_str.bright_yellow()
            } else {
                speed_str.bright_red()
            };

            let latency_str = format!("{}ms", result.latency_ms);
            let latency_colored = if result.latency_ms < 50 {
                latency_str.bright_green()
            } else if result.latency_ms < 100 {
                latency_str.bright_yellow()
            } else {
                latency_str.bright_red()
            };

            println!(
                "{:^20} {:^15} {:^12} {:^15}",
                result.timestamp.bright_black(),
                speed_colored,
                latency_colored,
                result.server.bright_cyan()
            );
        }

        println!("{}", "─".repeat(70).bright_black());

        // Statistics
        if history.len() > 1 {
            let avg_speed: f64 = history.iter().map(|r| r.download_mbps).sum::<f64>() / history.len() as f64;
            let avg_latency: f64 = history.iter().map(|r| r.latency_ms as f64).sum::<f64>() / history.len() as f64;
            let max_speed = history.iter().map(|r| r.download_mbps).fold(0.0f64, f64::max);

            println!(
                "\n{} Average: {:.2} Mbps | Best: {:.2} Mbps | Avg Latency: {:.0}ms",
                "📈".bright_cyan(),
                avg_speed,
                max_speed,
                avg_latency
            );
        }

        Ok(())
    }

    fn clear_history(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let confirm = dialoguer::Confirm::with_theme(&theme)
            .with_prompt("Are you sure you want to clear all speed test history?")
            .default(false)
            .interact()
            .map_err(|e| format!("Failed to get confirmation: {}", e))?;

        if confirm {
            self.base.config_mut().test_history.clear();
            self.base.save_config()?;
            println!(
                "{} {}",
                "✓".bright_green(),
                "Speed test history cleared!".bright_green()
            );
        } else {
            println!("{} {}", "ℹ️ ".bright_cyan(), "Operation cancelled".bright_cyan());
        }

        Ok(())
    }

    fn show_help(&self) -> Result<(), String> {
        let colors = self.base.config().colors.clone();

        let mut help = HelpFormatter::new("Speed Test Plugin".to_string());

        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Test your internet connection speed including download speed and latency.".to_string(),
            vec![],
        );

        help.add_section("Actions".to_string())
            .add_command(
                "test".to_string(),
                "Run a full speed test (multiple servers)".to_string(),
                vec!["lla plugin speed_test test".to_string()],
            )
            .add_command(
                "quick".to_string(),
                "Run a quick speed test (single server)".to_string(),
                vec!["lla plugin speed_test quick".to_string()],
            )
            .add_command(
                "history".to_string(),
                "Show speed test history".to_string(),
                vec!["lla plugin speed_test history".to_string()],
            )
            .add_command(
                "clear-history".to_string(),
                "Clear speed test history".to_string(),
                vec!["lla plugin speed_test clear-history".to_string()],
            )
            .add_command(
                "help".to_string(),
                "Show this help information".to_string(),
                vec!["lla plugin speed_test help".to_string()],
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

    fn interactive_menu(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let options = vec![
            "🚀 Run Full Speed Test",
            "⚡ Quick Test",
            "📜 View History",
            "🗑️  Clear History",
            "❓ Help",
            "← Exit",
        ];

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Speed Test Menu", "📡".bright_cyan()))
            .items(&options)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        match selection {
            0 => self.run_speed_test(),
            1 => self.quick_test(),
            2 => self.show_history(),
            3 => self.clear_history(),
            4 => self.show_help(),
            5 => Ok(()),
            _ => unreachable!(),
        }
    }
}

impl Plugin for SpeedTestPlugin {
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
                    PluginRequest::PerformAction(action, _args) => {
                        let result = match action.as_str() {
                            "test" => self.run_speed_test(),
                            "quick" => self.quick_test(),
                            "history" => self.show_history(),
                            "clear-history" => self.clear_history(),
                            "menu" => self.interactive_menu(),
                            "help" => self.show_help(),
                            _ => Err(format!(
                                "Unknown action: '{}'\n\n\
                                Available actions:\n  \
                                • test          - Run full speed test\n  \
                                • quick         - Run quick speed test\n  \
                                • history       - Show test history\n  \
                                • clear-history - Clear test history\n  \
                                • menu          - Interactive menu\n  \
                                • help          - Show help\n\n\
                                Example: lla plugin speed_test test",
                                action
                            )),
                        };
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::GetAvailableActions => {
                        use lla_plugin_interface::ActionInfo;
                        PluginResponse::AvailableActions(vec![
                            ActionInfo {
                                name: "test".to_string(),
                                usage: "test".to_string(),
                                description: "Run a full speed test".to_string(),
                                examples: vec!["lla plugin speed_test test".to_string()],
                            },
                            ActionInfo {
                                name: "quick".to_string(),
                                usage: "quick".to_string(),
                                description: "Run a quick speed test".to_string(),
                                examples: vec!["lla plugin speed_test quick".to_string()],
                            },
                            ActionInfo {
                                name: "history".to_string(),
                                usage: "history".to_string(),
                                description: "Show speed test history".to_string(),
                                examples: vec!["lla plugin speed_test history".to_string()],
                            },
                            ActionInfo {
                                name: "clear-history".to_string(),
                                usage: "clear-history".to_string(),
                                description: "Clear speed test history".to_string(),
                                examples: vec!["lla plugin speed_test clear-history".to_string()],
                            },
                            ActionInfo {
                                name: "menu".to_string(),
                                usage: "menu".to_string(),
                                description: "Interactive menu".to_string(),
                                examples: vec!["lla plugin speed_test menu".to_string()],
                            },
                            ActionInfo {
                                name: "help".to_string(),
                                usage: "help".to_string(),
                                description: "Show help information".to_string(),
                                examples: vec!["lla plugin speed_test help".to_string()],
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

impl Default for SpeedTestPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for SpeedTestPlugin {
    type Config = SpeedTestConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for SpeedTestPlugin {}

lla_plugin_interface::declare_plugin!(SpeedTestPlugin);

