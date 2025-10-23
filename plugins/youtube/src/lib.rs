use arboard::Clipboard;
use colored::Colorize;
use dialoguer::{Input, MultiSelect, Select};
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    ui::interactive_suggest,
    BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHistoryEntry {
    pub query: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YouTubeConfig {
    #[serde(default = "default_true")]
    pub remember_search_history: bool,
    #[serde(default = "default_true")]
    pub use_clipboard_fallback: bool,
    #[serde(default)]
    pub search_history: Vec<SearchHistoryEntry>,
    #[serde(default = "default_max_history")]
    pub max_history_size: usize,
    #[serde(default = "default_colors")]
    pub colors: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

fn default_max_history() -> usize {
    100
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_cyan".to_string());
    colors.insert("warning".to_string(), "bright_yellow".to_string());
    colors.insert("error".to_string(), "bright_red".to_string());
    colors.insert("prompt".to_string(), "bright_blue".to_string());
    colors.insert("link".to_string(), "bright_magenta".to_string());
    colors
}

impl Default for YouTubeConfig {
    fn default() -> Self {
        Self {
            remember_search_history: true,
            use_clipboard_fallback: true,
            search_history: Vec::new(),
            max_history_size: 100,
            colors: default_colors(),
        }
    }
}

impl PluginConfig for YouTubeConfig {}

pub struct YouTubePlugin {
    base: BasePlugin<YouTubeConfig>,
    http: Client,
}

impl YouTubePlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap_or_else(|_| Client::new());
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
            http: client,
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[YouTubePlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn add_to_history(&mut self, query: &str) {
        let config = self.base.config_mut();

        if !config.remember_search_history {
            return;
        }

        // Remove duplicate if exists
        config.search_history.retain(|e| e.query != query);

        // Add new entry at the beginning
        config.search_history.insert(
            0,
            SearchHistoryEntry {
                query: query.to_string(),
                timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            },
        );

        // Trim history if needed
        if config.search_history.len() > config.max_history_size {
            config.search_history.truncate(config.max_history_size);
        }

        if let Err(e) = self.base.save_config() {
            eprintln!("Failed to save search history: {}", e);
        }
    }

    fn open_youtube_search(&self, query: &str) -> Result<(), String> {
        let encoded_query =
            url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
        let search_url = format!(
            "https://www.youtube.com/results?search_query={}",
            encoded_query
        );

        #[cfg(target_os = "macos")]
        let open_command = "open";
        #[cfg(target_os = "linux")]
        let open_command = "xdg-open";
        #[cfg(target_os = "windows")]
        let open_command = "start";

        std::process::Command::new(open_command)
            .arg(&search_url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;

        Ok(())
    }

    fn fetch_youtube_suggestions(&self, query: &str) -> Result<Vec<String>, String> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let encoded_query =
            url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
        let url = format!(
            "http://suggestqueries.google.com/complete/search?client=firefox&ds=yt&q={}",
            encoded_query
        );

        let response = self.http
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to fetch suggestions: {}", e))?;

        let json: Value = response
            .json()
            .map_err(|e| format!("Failed to parse suggestions: {}", e))?;

        if let Some(suggestions) = json.get(1).and_then(|v| v.as_array()) {
            let results: Vec<String> = suggestions
                .iter()
                .filter_map(|s| s.as_str().map(|s| s.to_string()))
                .take(10)
                .collect();
            Ok(results)
        } else {
            Ok(Vec::new())
        }
    }

    fn perform_search(&mut self, text: Option<String>) -> Result<(), String> {
        println!(
            "\n{} {}",
            "💡".bright_yellow(),
            "Type to see live suggestions. Use arrows to pick or Enter to search."
                .bright_cyan()
        );

        let initial = text.as_deref();
        let query = interactive_suggest(
            "Search YouTube:",
            initial,
            |q| self.fetch_youtube_suggestions(q),
        )?;

        if query.trim().is_empty() {
            return Err("Search query cannot be empty".to_string());
        }

        println!(
            "{} Searching YouTube for: {}",
            "Info:".bright_cyan(),
            query.bright_yellow()
        );
        self.open_youtube_search(&query)?;
        self.add_to_history(&query);
        println!("{} Done!", "Success:".bright_green());
        Ok(())
    }

    fn search_selected_text(&mut self) -> Result<(), String> {
        // Get text from clipboard
        let clipboard_text = if self.base.config().use_clipboard_fallback {
            match Clipboard::new() {
                Ok(mut clipboard) => clipboard.get_text().ok(),
                Err(_) => None,
            }
        } else {
            None
        };

        if let Some(text) = clipboard_text {
            println!(
                "{} Using text from clipboard: {}",
                "Info:".bright_cyan(),
                text.bright_yellow()
            );
            self.perform_search(Some(text))
        } else {
            Err("No text available in clipboard. Copy some text first.".to_string())
        }
    }

    fn manage_history(&mut self) -> Result<(), String> {
        let history = self.base.config().search_history.clone();

        if history.is_empty() {
            println!("{} No search history available", "Info:".bright_cyan());
            return Ok(());
        }

        let theme = LlaDialoguerTheme::default();

        let items: Vec<String> = history
            .iter()
            .map(|entry| {
                format!(
                    "{} {} {}",
                    "🎬".bright_cyan(),
                    entry.query,
                    format!("({})", entry.timestamp).bright_black()
                )
            })
            .collect();

        let actions = vec![
            "🔍 Search selected entry",
            "📋 Copy to clipboard",
            "🗑️  Delete selected entries",
            "🧹 Clear all history",
            "📊 Show statistics",
        ];

        let action_selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Choose action", "⚡".bright_cyan()))
            .items(&actions)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show action menu: {}", e))?;

        match action_selection {
            0 => {
                // Search selected entry
                let selection = Select::with_theme(&theme)
                    .with_prompt(format!("{} Select entry to search", "🔍".bright_cyan()))
                    .items(&items)
                    .default(0)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                let query = &history[selection].query;
                self.open_youtube_search(query)?;
                self.add_to_history(query);

                println!("{} Search completed!", "Success:".bright_green());
            }
            1 => {
                // Copy to clipboard
                let selection = MultiSelect::with_theme(&theme)
                    .with_prompt(format!("{} Select entries to copy", "📋".bright_cyan()))
                    .items(&items)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                if selection.is_empty() {
                    println!("{} No entries selected", "Info:".bright_blue());
                    return Ok(());
                }

                let queries: Vec<String> = selection
                    .iter()
                    .map(|&i| history[i].query.clone())
                    .collect();
                let content = queries.join("\n");

                match Clipboard::new() {
                    Ok(mut clipboard) => {
                        clipboard
                            .set_text(&content)
                            .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;
                        println!(
                            "{} {} entries copied to clipboard!",
                            "Success:".bright_green(),
                            selection.len()
                        );
                    }
                    Err(e) => return Err(format!("Failed to access clipboard: {}", e)),
                }
            }
            2 => {
                // Delete selected entries
                let selection = MultiSelect::with_theme(&theme)
                    .with_prompt(format!("{} Select entries to delete", "🗑️ ".bright_cyan()))
                    .items(&items)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                if selection.is_empty() {
                    println!("{} No entries selected", "Info:".bright_blue());
                    return Ok(());
                }

                let config = self.base.config_mut();
                let mut indices = selection;
                indices.sort_unstable_by(|a, b| b.cmp(a)); // Sort in reverse to delete safely
                for &i in &indices {
                    config.search_history.remove(i);
                }

                self.base.save_config()?;
                println!(
                    "{} {} entries deleted!",
                    "Success:".bright_green(),
                    indices.len()
                );
            }
            3 => {
                // Clear all history
                let confirm: bool = dialoguer::Confirm::with_theme(&theme)
                    .with_prompt("Are you sure you want to clear all search history?")
                    .default(false)
                    .interact()
                    .map_err(|e| format!("Failed to get confirmation: {}", e))?;

                if confirm {
                    self.base.config_mut().search_history.clear();
                    self.base.save_config()?;
                    println!("{} All search history cleared!", "Success:".bright_green());
                } else {
                    println!("{} Operation cancelled", "Info:".bright_blue());
                }
            }
            4 => {
                // Show statistics
                let total = history.len();
                let unique_queries: std::collections::HashSet<_> =
                    history.iter().map(|e| &e.query).collect();

                println!("\n{} Search History Statistics:", "📊".bright_cyan());
                println!("─{}─", "─".repeat(50));
                println!(" • Total searches: {}", total.to_string().bright_yellow());
                println!(
                    " • Unique queries: {}",
                    unique_queries.len().to_string().bright_yellow()
                );

                if let Some(oldest) = history.last() {
                    println!(" • Oldest search: {}", oldest.timestamp.bright_black());
                }
                if let Some(newest) = history.first() {
                    println!(" • Most recent: {}", newest.timestamp.bright_black());
                }

                // Top 5 most frequent queries
                let mut freq: HashMap<&String, usize> = HashMap::new();
                for entry in history.iter() {
                    *freq.entry(&entry.query).or_insert(0) += 1;
                }

                let mut freq_vec: Vec<_> = freq.into_iter().collect();
                freq_vec.sort_by(|a, b| b.1.cmp(&a.1));

                if !freq_vec.is_empty() {
                    println!("\n{} Top 5 searches:", "🔥".bright_yellow());
                    for (query, count) in freq_vec.iter().take(5) {
                        println!(
                            " • {} ({}x)",
                            query.bright_magenta(),
                            count.to_string().bright_yellow()
                        );
                    }
                }
                println!("─{}─\n", "─".repeat(50));
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    fn configure_preferences(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let options = vec![
            format!(
                "Remember Search History: {}",
                if self.base.config().remember_search_history {
                    "✓ Enabled".bright_green()
                } else {
                    "✗ Disabled".bright_red()
                }
            ),
            format!(
                "Use Clipboard Fallback: {}",
                if self.base.config().use_clipboard_fallback {
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
                    config.remember_search_history = !config.remember_search_history;
                    config.remember_search_history
                };
                self.base.save_config()?;
                println!(
                    "{} Remember search history: {}",
                    "Success:".bright_green(),
                    if new_value { "enabled" } else { "disabled" }
                );
            }
            1 => {
                let new_value = {
                    let config = self.base.config_mut();
                    config.use_clipboard_fallback = !config.use_clipboard_fallback;
                    config.use_clipboard_fallback
                };
                self.base.save_config()?;
                println!(
                    "{} Clipboard fallback: {}",
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

                // Trim history if needed
                if config.search_history.len() > config.max_history_size {
                    config.search_history.truncate(config.max_history_size);
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
        let remember_history = self.base.config().remember_search_history;
        let use_clipboard = self.base.config().use_clipboard_fallback;
        let colors = self.base.config().colors.clone();

        let mut help = HelpFormatter::new("YouTube Search Plugin".to_string());

        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Search YouTube with live autosuggestions and history tracking.".to_string(),
            vec![],
        );

        help.add_section("Actions".to_string())
            .add_command(
                "search".to_string(),
                "Search YouTube (live suggestions; arrows to select)".to_string(),
                vec!["search".to_string()],
            )
            .add_command(
                "search-selected".to_string(),
                "Search YouTube with clipboard text".to_string(),
                vec!["search-selected".to_string()],
            )
            .add_command(
                "history".to_string(),
                "Manage search history".to_string(),
                vec!["history".to_string()],
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
                "Remember Search History".to_string(),
                format!(
                    "Currently: {}",
                    if remember_history {
                        "✓ Enabled".bright_green().to_string()
                    } else {
                        "✗ Disabled".bright_red().to_string()
                    }
                ),
                vec![],
            )
            .add_command(
                "Use Clipboard Fallback".to_string(),
                format!(
                    "Currently: {}",
                    if use_clipboard {
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

impl Plugin for YouTubePlugin {
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
                            "search" => self.perform_search(None),
                            "search-selected" => self.search_selected_text(),
                            "history" => self.manage_history(),
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

impl Default for YouTubePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for YouTubePlugin {
    type Config = YouTubeConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for YouTubePlugin {}

lla_plugin_interface::declare_plugin!(YouTubePlugin);
