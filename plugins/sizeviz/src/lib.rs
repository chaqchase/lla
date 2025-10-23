use colored::Colorize;
use lazy_static::lazy_static;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, Spinner},
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{cmp, collections::HashMap};

lazy_static! {
    static ref SPINNER: RwLock<Spinner> = RwLock::new(Spinner::new());
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name sizeviz --action help"],
            |_| {
                let mut help = HelpFormatter::new("Size Visualizer Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Visualizes file sizes with bars and percentage indicators.".to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string()).add_command(
                    "help".to_string(),
                    "Show this help information".to_string(),
                    vec!["lla plugin --name sizeviz --action help".to_string()],
                );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic size visualization".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show detailed size visualization with percentage".to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&SizeConfig::default().colors))
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
pub struct SizeConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("tiny".to_string(), "bright_green".to_string());
    colors.insert("small".to_string(), "bright_cyan".to_string());
    colors.insert("medium".to_string(), "bright_yellow".to_string());
    colors.insert("large".to_string(), "bright_red".to_string());
    colors.insert("huge".to_string(), "bright_magenta".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("size".to_string(), "bright_yellow".to_string());
    colors.insert("percentage".to_string(), "bright_magenta".to_string());
    colors
}

impl Default for SizeConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for SizeConfig {}

pub struct FileSizeVisualizerPlugin {
    base: BasePlugin<SizeConfig>,
}

impl FileSizeVisualizerPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[FileSizeVisualizerPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn format_size(size: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;

        match size {
            0 => "0 B".to_string(),
            s if s < KB => format!("{} B", s),
            s if s < MB => format!("{:.1} KB", s as f64 / KB as f64),
            s if s < GB => format!("{:.1} MB", s as f64 / MB as f64),
            s if s < TB => format!("{:.2} GB", s as f64 / GB as f64),
            s => format!("{:.2} TB", s as f64 / TB as f64),
        }
    }

    fn size_to_bar(size: u64, max_size: u64, max_width: usize) -> String {
        let normalized_size = (size as f64 / max_size as f64 * max_width as f64).ceil() as usize;
        let width = cmp::min(normalized_size, max_width);

        let full_blocks = "█".repeat(width);
        let remainder = (normalized_size as f64 * 8.0) as usize % 8;
        let partial_block = match remainder {
            1 => "▏",
            2 => "▎",
            3 => "▍",
            4 => "▌",
            5 => "▋",
            6 => "▊",
            7 => "▉",
            _ => "",
        };

        let spaces = "░".repeat(max_width.saturating_sub(width));
        format!("{}{}{}", full_blocks, partial_block, spaces)
    }

    fn get_size_color(&self, size: u64) -> String {
        let colors = &self.base.config().colors;
        const KB: u64 = 1024;
        const KB_1: u64 = KB + 1;
        const KB_10: u64 = KB * 10;
        const KB_10_1: u64 = KB_10 + 1;
        const MB: u64 = KB * 1024;
        const MB_1: u64 = MB + 1;
        const MB_10: u64 = MB * 10;
        const MB_10_1: u64 = MB_10 + 1;
        const MB_100: u64 = MB * 100;
        const MB_100_1: u64 = MB_100 + 1;
        const GB: u64 = MB * 1024;

        match size {
            0..=KB => colors
                .get("tiny")
                .unwrap_or(&"white".to_string())
                .to_string(),
            KB_1..=KB_10 => colors
                .get("small")
                .unwrap_or(&"white".to_string())
                .to_string(),
            KB_10_1..=MB => colors
                .get("small")
                .unwrap_or(&"white".to_string())
                .to_string(),
            MB_1..=MB_10 => colors
                .get("medium")
                .unwrap_or(&"white".to_string())
                .to_string(),
            MB_10_1..=MB_100 => colors
                .get("large")
                .unwrap_or(&"white".to_string())
                .to_string(),
            MB_100_1..=GB => colors
                .get("large")
                .unwrap_or(&"white".to_string())
                .to_string(),
            _ => colors
                .get("huge")
                .unwrap_or(&"white".to_string())
                .to_string(),
        }
    }

    fn get_percentage(size: u64, total_size: u64) -> f64 {
        if total_size == 0 {
            0.0
        } else {
            (size as f64 / total_size as f64) * 100.0
        }
    }

    fn format_size_info(
        &self,
        entry: &lla_plugin_interface::DecoratedEntry,
        format: &str,
    ) -> Option<String> {
        entry
            .custom_fields
            .get("size")
            .and_then(|size_str| size_str.parse::<u64>().ok())
            .map(|size| {
                let max_size = 1_073_741_824;
                let result = match format {
                    "long" => {
                        let bar = Self::size_to_bar(size, max_size, 20);
                        let bar_color = self.get_size_color(size);
                        let percentage = Self::get_percentage(size, max_size);

                        format!(
                            "\n{}\n{}\n{}\n{}",
                            format!(
                                "┌─ {} ─{}",
                                "Size".bright_blue(),
                                "─".repeat(40).bright_black()
                            ),
                            format!(
                                "│ {} {}",
                                bar.color(match bar_color.as_str() {
                                    "bright_green" => colored::Color::BrightGreen,
                                    "bright_cyan" => colored::Color::BrightCyan,
                                    "bright_yellow" => colored::Color::BrightYellow,
                                    "bright_red" => colored::Color::BrightRed,
                                    "bright_magenta" => colored::Color::BrightMagenta,
                                    _ => colored::Color::White,
                                }),
                                Self::format_size(size).bright_yellow()
                            ),
                            format!(
                                "│ {}% of reference (1GB)",
                                format!("{:.1}", percentage).bright_magenta()
                            ),
                            format!("└{}", "─".repeat(50).bright_black())
                        )
                    }
                    "default" => {
                        let bar = Self::size_to_bar(size, max_size, 10);
                        let bar_color = self.get_size_color(size);
                        format!(
                            "{} {}",
                            bar.color(match bar_color.as_str() {
                                "bright_green" => colored::Color::BrightGreen,
                                "bright_cyan" => colored::Color::BrightCyan,
                                "bright_yellow" => colored::Color::BrightYellow,
                                "bright_red" => colored::Color::BrightRed,
                                "bright_magenta" => colored::Color::BrightMagenta,
                                _ => colored::Color::White,
                            }),
                            Self::format_size(size).bright_yellow()
                        )
                    }
                    _ => return None,
                };
                Some(result)
            })
            .flatten()
    }
}

impl Plugin for FileSizeVisualizerPlugin {
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
                        spinner.set_status("Calculating size...".to_string());

                        if entry.path.is_file() {
                            let size = entry.metadata.size;
                            entry
                                .custom_fields
                                .insert("size".to_string(), size.to_string());
                        }

                        spinner.finish();
                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::FormatField(entry, format) => {
                        let field = self.format_size_info(&entry, &format);
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

impl Default for FileSizeVisualizerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for FileSizeVisualizerPlugin {
    type Config = SizeConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for FileSizeVisualizerPlugin {}

lla_plugin_interface::declare_plugin!(FileSizeVisualizerPlugin);
