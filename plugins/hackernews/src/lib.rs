use arboard::Clipboard;
use colored::Colorize;
use dialoguer::Select;
use indicatif::{ProgressBar, ProgressStyle};
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

const HN_API_BASE: &str = "https://hacker-news.firebaseio.com/v0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HNTopic {
    Top,
    Best,
    New,
    Ask,
    Show,
    Jobs,
}

impl HNTopic {
    fn api_endpoint(&self) -> String {
        match self {
            HNTopic::Top => format!("{}/topstories.json", HN_API_BASE),
            HNTopic::Best => format!("{}/beststories.json", HN_API_BASE),
            HNTopic::New => format!("{}/newstories.json", HN_API_BASE),
            HNTopic::Ask => format!("{}/askstories.json", HN_API_BASE),
            HNTopic::Show => format!("{}/showstories.json", HN_API_BASE),
            HNTopic::Jobs => format!("{}/jobstories.json", HN_API_BASE),
        }
    }

    fn rss_path(&self) -> &'static str {
        match self {
            HNTopic::Top => "frontpage",
            HNTopic::Best => "best",
            HNTopic::New => "newest",
            HNTopic::Ask => "ask",
            HNTopic::Show => "show",
            HNTopic::Jobs => "jobs",
        }
    }

    fn name(&self) -> &'static str {
        match self {
            HNTopic::Top => "Top Stories",
            HNTopic::Best => "Best Stories",
            HNTopic::New => "New Stories",
            HNTopic::Ask => "Ask HN",
            HNTopic::Show => "Show HN",
            HNTopic::Jobs => "Jobs",
        }
    }

    fn emoji(&self) -> &'static str {
        match self {
            HNTopic::Top => "🔥",
            HNTopic::Best => "⭐",
            HNTopic::New => "🆕",
            HNTopic::Ask => "❓",
            HNTopic::Show => "🎨",
            HNTopic::Jobs => "💼",
        }
    }

    fn all() -> Vec<HNTopic> {
        vec![
            HNTopic::Top,
            HNTopic::Best,
            HNTopic::New,
            HNTopic::Ask,
            HNTopic::Show,
            HNTopic::Jobs,
        ]
    }
}

impl Default for HNTopic {
    fn default() -> Self {
        HNTopic::Top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HNItem {
    pub id: u64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub by: Option<String>,
    #[serde(default)]
    pub score: Option<u32>,
    #[serde(default)]
    pub descendants: Option<u32>,
    #[serde(default)]
    pub time: Option<u64>,
    #[serde(rename = "type", default)]
    pub item_type: Option<String>,
}

impl HNItem {
    fn hn_url(&self) -> String {
        format!("https://news.ycombinator.com/item?id={}", self.id)
    }

    fn display_url(&self) -> String {
        self.url.clone().unwrap_or_else(|| self.hn_url())
    }

    fn domain(&self) -> Option<String> {
        self.url.as_ref().and_then(|u| {
            // Simple domain extraction without url crate
            let u = u.trim_start_matches("https://").trim_start_matches("http://");
            u.split('/').next().map(|s| s.to_string())
        })
    }

    fn time_ago(&self) -> String {
        if let Some(timestamp) = self.time {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let diff = now.saturating_sub(timestamp);

            if diff < 60 {
                format!("{}s ago", diff)
            } else if diff < 3600 {
                format!("{}m ago", diff / 60)
            } else if diff < 86400 {
                format!("{}h ago", diff / 3600)
            } else {
                format!("{}d ago", diff / 86400)
            }
        } else {
            "unknown".to_string()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub items: Vec<HNItem>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HackerNewsConfig {
    #[serde(default)]
    pub default_topic: HNTopic,
    #[serde(default = "default_story_count")]
    pub story_count: usize,
    #[serde(default = "default_cache_duration")]
    pub cache_duration_secs: u64,
    #[serde(default)]
    pub cached_stories: HashMap<String, CacheEntry>,
    #[serde(default = "default_colors")]
    pub colors: HashMap<String, String>,
}

fn default_story_count() -> usize {
    30
}

fn default_cache_duration() -> u64 {
    300 // 5 minutes
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_cyan".to_string());
    colors.insert("warning".to_string(), "bright_yellow".to_string());
    colors.insert("error".to_string(), "bright_red".to_string());
    colors.insert("title".to_string(), "bright_white".to_string());
    colors.insert("score".to_string(), "bright_yellow".to_string());
    colors.insert("comments".to_string(), "bright_cyan".to_string());
    colors.insert("domain".to_string(), "bright_blue".to_string());
    colors.insert("time".to_string(), "bright_black".to_string());
    colors.insert("author".to_string(), "bright_magenta".to_string());
    colors
}

impl Default for HackerNewsConfig {
    fn default() -> Self {
        Self {
            default_topic: HNTopic::default(),
            story_count: 30,
            cache_duration_secs: 300,
            cached_stories: HashMap::new(),
            colors: default_colors(),
        }
    }
}

impl PluginConfig for HackerNewsConfig {}

pub struct HackerNewsPlugin {
    base: BasePlugin<HackerNewsConfig>,
    http: Client,
}

impl HackerNewsPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("lla-hackernews-plugin/0.5.0")
            .build()
            .unwrap_or_else(|_| Client::new());

        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
            http: client,
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[HackerNewsPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn get_cached_stories(&self, topic: HNTopic) -> Option<Vec<HNItem>> {
        let cache_key = topic.rss_path().to_string();
        let config = self.base.config();

        if let Some(entry) = config.cached_stories.get(&cache_key) {
            let age = Self::current_timestamp().saturating_sub(entry.timestamp);
            if age < config.cache_duration_secs {
                return Some(entry.items.clone());
            }
        }
        None
    }

    fn cache_stories(&mut self, topic: HNTopic, items: Vec<HNItem>) {
        let cache_key = topic.rss_path().to_string();
        let entry = CacheEntry {
            items,
            timestamp: Self::current_timestamp(),
        };
        self.base.config_mut().cached_stories.insert(cache_key, entry);
        let _ = self.base.save_config();
    }

    fn fetch_story_ids(&self, topic: HNTopic) -> Result<Vec<u64>, String> {
        let url = topic.api_endpoint();
        let response = self
            .http
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to fetch story IDs: {}", e))?;

        let ids: Vec<u64> = response
            .json()
            .map_err(|e| format!("Failed to parse story IDs: {}", e))?;

        Ok(ids)
    }

    fn fetch_item(&self, id: u64) -> Result<HNItem, String> {
        let url = format!("{}/item/{}.json", HN_API_BASE, id);
        let response = self
            .http
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to fetch item {}: {}", id, e))?;

        let item: HNItem = response
            .json()
            .map_err(|e| format!("Failed to parse item {}: {}", id, e))?;

        Ok(item)
    }

    fn fetch_stories(&mut self, topic: HNTopic) -> Result<Vec<HNItem>, String> {
        // Check cache first
        if let Some(cached) = self.get_cached_stories(topic) {
            return Ok(cached);
        }

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message(format!("Fetching {} from Hacker News...", topic.name()));
        pb.enable_steady_tick(Duration::from_millis(100));

        let story_count = self.base.config().story_count;
        let ids = self.fetch_story_ids(topic)?;
        let ids: Vec<u64> = ids.into_iter().take(story_count).collect();

        let mut items = Vec::with_capacity(ids.len());
        for (i, id) in ids.iter().enumerate() {
            pb.set_message(format!(
                "Fetching {} ({}/{})...",
                topic.name(),
                i + 1,
                ids.len()
            ));

            if let Ok(item) = self.fetch_item(*id) {
                items.push(item);
            }
        }

        pb.finish_and_clear();

        // Cache the results
        self.cache_stories(topic, items.clone());

        Ok(items)
    }

    fn display_stories(&mut self, topic: HNTopic) -> Result<(), String> {
        let items = self.fetch_stories(topic)?;

        println!(
            "\n{} {} {}",
            topic.emoji(),
            topic.name().bright_cyan(),
            format!("({} stories)", items.len()).bright_black()
        );
        println!("{}", "─".repeat(80).bright_black());

        for (i, item) in items.iter().enumerate() {
            let title = item.title.as_deref().unwrap_or("(no title)");
            let score = item.score.unwrap_or(0);
            let comments = item.descendants.unwrap_or(0);
            let author = item.by.as_deref().unwrap_or("unknown");
            let domain = item.domain().unwrap_or_else(|| "news.ycombinator.com".to_string());
            let time_ago = item.time_ago();

            // Rank number
            let rank = format!("{:>3}.", i + 1);

            // Score badge
            let score_str = format!("▲{}", score);
            let score_colored = if score > 100 {
                score_str.bright_yellow()
            } else if score > 50 {
                score_str.yellow()
            } else {
                score_str.bright_black()
            };

            // Comments badge
            let comments_str = format!("💬{}", comments);
            let comments_colored = if comments > 50 {
                comments_str.bright_cyan()
            } else {
                comments_str.bright_black()
            };

            println!(
                "{} {} {}",
                rank.bright_black(),
                title.bright_white(),
                format!("({})", domain).bright_blue()
            );
            println!(
                "     {} {} {} by {} {}",
                score_colored,
                comments_colored,
                "•".bright_black(),
                author.bright_magenta(),
                time_ago.bright_black()
            );
        }

        println!("{}", "─".repeat(80).bright_black());
        println!(
            "   {} {}",
            "ℹ️ ".bright_cyan(),
            "Use 'open <number>' to open a story in your browser".bright_black()
        );

        Ok(())
    }

    fn open_story(&mut self, topic: HNTopic, index: usize) -> Result<(), String> {
        let items = self.fetch_stories(topic)?;

        if index == 0 || index > items.len() {
            return Err(format!(
                "Invalid story number. Please choose between 1 and {}",
                items.len()
            ));
        }

        let item = &items[index - 1];
        let url = item.display_url();

        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(target_os = "windows")]
        let cmd = "start";

        std::process::Command::new(cmd)
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;

        println!(
            "{} Opened: {}",
            "✓".bright_green(),
            item.title.as_deref().unwrap_or("story").bright_yellow()
        );

        Ok(())
    }

    fn open_comments(&mut self, topic: HNTopic, index: usize) -> Result<(), String> {
        let items = self.fetch_stories(topic)?;

        if index == 0 || index > items.len() {
            return Err(format!(
                "Invalid story number. Please choose between 1 and {}",
                items.len()
            ));
        }

        let item = &items[index - 1];
        let url = item.hn_url();

        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(target_os = "windows")]
        let cmd = "start";

        std::process::Command::new(cmd)
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;

        println!(
            "{} Opened comments for: {}",
            "✓".bright_green(),
            item.title.as_deref().unwrap_or("story").bright_yellow()
        );

        Ok(())
    }

    fn copy_url(&mut self, topic: HNTopic, index: usize) -> Result<(), String> {
        let items = self.fetch_stories(topic)?;

        if index == 0 || index > items.len() {
            return Err(format!(
                "Invalid story number. Please choose between 1 and {}",
                items.len()
            ));
        }

        let item = &items[index - 1];
        let url = item.display_url();

        match Clipboard::new() {
            Ok(mut clipboard) => {
                clipboard
                    .set_text(&url)
                    .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;
                println!(
                    "{} URL copied: {}",
                    "✓".bright_green(),
                    url.bright_blue()
                );
                Ok(())
            }
            Err(e) => Err(format!("Failed to access clipboard: {}", e)),
        }
    }

    fn interactive_browse(&mut self, topic: HNTopic) -> Result<(), String> {
        let items = self.fetch_stories(topic)?;
        let theme = LlaDialoguerTheme::default();

        loop {
            println!(
                "\n{} {} {}",
                topic.emoji(),
                topic.name().bright_cyan(),
                format!("({} stories)", items.len()).bright_black()
            );
            println!("{}", "─".repeat(80).bright_black());

            let display_items: Vec<String> = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let title = item.title.as_deref().unwrap_or("(no title)");
                    let score = item.score.unwrap_or(0);
                    let comments = item.descendants.unwrap_or(0);
                    let domain = item.domain().unwrap_or_else(|| "hn".to_string());

                    format!(
                        "{:>3}. {} ({}) [▲{} 💬{}]",
                        i + 1,
                        title,
                        domain,
                        score,
                        comments
                    )
                })
                .collect();

            let mut options = display_items.clone();
            options.push("─── Actions ───".to_string());
            options.push("🔄 Refresh".to_string());
            options.push("📋 Change Topic".to_string());
            options.push("← Back".to_string());

            let selection = Select::with_theme(&theme)
                .with_prompt("Select a story")
                .items(&options)
                .default(0)
                .max_length(20)
                .interact()
                .map_err(|e| format!("Failed to show menu: {}", e))?;

            if selection < items.len() {
                // Story selected - show sub-menu
                let story_options = vec![
                    "🔗 Open article",
                    "💬 Open comments",
                    "📋 Copy URL",
                    "← Back",
                ];

                let story_action = Select::with_theme(&theme)
                    .with_prompt(format!(
                        "{}",
                        items[selection]
                            .title
                            .as_deref()
                            .unwrap_or("Story")
                            .bright_white()
                    ))
                    .items(&story_options)
                    .default(0)
                    .interact()
                    .map_err(|e| format!("Failed to show story menu: {}", e))?;

                match story_action {
                    0 => {
                        let url = items[selection].display_url();
                        #[cfg(target_os = "macos")]
                        let cmd = "open";
                        #[cfg(target_os = "linux")]
                        let cmd = "xdg-open";
                        #[cfg(target_os = "windows")]
                        let cmd = "start";

                        let _ = std::process::Command::new(cmd).arg(&url).spawn();
                    }
                    1 => {
                        let url = items[selection].hn_url();
                        #[cfg(target_os = "macos")]
                        let cmd = "open";
                        #[cfg(target_os = "linux")]
                        let cmd = "xdg-open";
                        #[cfg(target_os = "windows")]
                        let cmd = "start";

                        let _ = std::process::Command::new(cmd).arg(&url).spawn();
                    }
                    2 => {
                        let url = items[selection].display_url();
                        if let Ok(mut clipboard) = Clipboard::new() {
                            let _ = clipboard.set_text(&url);
                            println!(
                                "{} URL copied!",
                                "✓".bright_green()
                            );
                        }
                    }
                    3 => continue,
                    _ => {}
                }
            } else {
                let action_index = selection - items.len() - 1; // -1 for separator
                match action_index {
                    0 => {
                        // Refresh - clear cache
                        let cache_key = topic.rss_path().to_string();
                        self.base.config_mut().cached_stories.remove(&cache_key);
                        return self.interactive_browse(topic);
                    }
                    1 => {
                        // Change topic
                        return self.interactive_menu();
                    }
                    2 => {
                        // Back
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    }

    fn interactive_menu(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let topics = HNTopic::all();
        let options: Vec<String> = topics
            .iter()
            .map(|t| format!("{} {}", t.emoji(), t.name()))
            .chain(std::iter::once("← Exit".to_string()))
            .collect();

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Hacker News", "📰".bright_cyan()))
            .items(&options)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        if selection < topics.len() {
            self.interactive_browse(topics[selection])
        } else {
            Ok(())
        }
    }

    fn clear_cache(&mut self) -> Result<(), String> {
        self.base.config_mut().cached_stories.clear();
        self.base.save_config()?;
        println!(
            "{} {}",
            "✓".bright_green(),
            "Cache cleared!".bright_green()
        );
        Ok(())
    }

    fn show_help(&self) -> Result<(), String> {
        let colors = self.base.config().colors.clone();

        let mut help = HelpFormatter::new("Hacker News Plugin".to_string());

        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Browse Hacker News stories - top, best, new, ask, show, and jobs.".to_string(),
            vec![],
        );

        help.add_section("Actions".to_string())
            .add_command(
                "top".to_string(),
                "Show top stories".to_string(),
                vec!["lla plugin hackernews top".to_string()],
            )
            .add_command(
                "best".to_string(),
                "Show best stories".to_string(),
                vec!["lla plugin hackernews best".to_string()],
            )
            .add_command(
                "new".to_string(),
                "Show new stories".to_string(),
                vec!["lla plugin hackernews new".to_string()],
            )
            .add_command(
                "ask".to_string(),
                "Show Ask HN stories".to_string(),
                vec!["lla plugin hackernews ask".to_string()],
            )
            .add_command(
                "show".to_string(),
                "Show Show HN stories".to_string(),
                vec!["lla plugin hackernews show".to_string()],
            )
            .add_command(
                "jobs".to_string(),
                "Show job postings".to_string(),
                vec!["lla plugin hackernews jobs".to_string()],
            )
            .add_command(
                "open <n>".to_string(),
                "Open story #n in browser".to_string(),
                vec!["lla plugin hackernews open 1".to_string()],
            )
            .add_command(
                "comments <n>".to_string(),
                "Open comments for story #n".to_string(),
                vec!["lla plugin hackernews comments 1".to_string()],
            )
            .add_command(
                "copy <n>".to_string(),
                "Copy URL of story #n".to_string(),
                vec!["lla plugin hackernews copy 1".to_string()],
            )
            .add_command(
                "browse".to_string(),
                "Interactive story browser".to_string(),
                vec!["lla plugin hackernews browse".to_string()],
            )
            .add_command(
                "menu".to_string(),
                "Interactive menu".to_string(),
                vec!["lla plugin hackernews menu".to_string()],
            )
            .add_command(
                "clear-cache".to_string(),
                "Clear cached stories".to_string(),
                vec!["lla plugin hackernews clear-cache".to_string()],
            )
            .add_command(
                "help".to_string(),
                "Show this help information".to_string(),
                vec!["lla plugin hackernews help".to_string()],
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

impl Plugin for HackerNewsPlugin {
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
                        let default_topic = self.base.config().default_topic;

                        let result = match action.as_str() {
                            "top" => self.display_stories(HNTopic::Top),
                            "best" => self.display_stories(HNTopic::Best),
                            "new" | "newest" => self.display_stories(HNTopic::New),
                            "ask" => self.display_stories(HNTopic::Ask),
                            "show" => self.display_stories(HNTopic::Show),
                            "jobs" => self.display_stories(HNTopic::Jobs),
                            "open" => {
                                if let Some(index_str) = args.first() {
                                    if let Ok(index) = index_str.parse::<usize>() {
                                        self.open_story(default_topic, index)
                                    } else {
                                        Err("Invalid story number".to_string())
                                    }
                                } else {
                                    Err("Please specify a story number".to_string())
                                }
                            }
                            "comments" => {
                                if let Some(index_str) = args.first() {
                                    if let Ok(index) = index_str.parse::<usize>() {
                                        self.open_comments(default_topic, index)
                                    } else {
                                        Err("Invalid story number".to_string())
                                    }
                                } else {
                                    Err("Please specify a story number".to_string())
                                }
                            }
                            "copy" => {
                                if let Some(index_str) = args.first() {
                                    if let Ok(index) = index_str.parse::<usize>() {
                                        self.copy_url(default_topic, index)
                                    } else {
                                        Err("Invalid story number".to_string())
                                    }
                                } else {
                                    Err("Please specify a story number".to_string())
                                }
                            }
                            "browse" => {
                                let topic = if let Some(topic_str) = args.first() {
                                    match topic_str.to_lowercase().as_str() {
                                        "top" => HNTopic::Top,
                                        "best" => HNTopic::Best,
                                        "new" | "newest" => HNTopic::New,
                                        "ask" => HNTopic::Ask,
                                        "show" => HNTopic::Show,
                                        "jobs" => HNTopic::Jobs,
                                        _ => default_topic,
                                    }
                                } else {
                                    default_topic
                                };
                                self.interactive_browse(topic)
                            }
                            "menu" => self.interactive_menu(),
                            "clear-cache" | "refresh" => self.clear_cache(),
                            "help" => self.show_help(),
                            _ => Err(format!(
                                "Unknown action: '{}'\n\n\
                                Available actions:\n  \
                                • top       - Show top stories\n  \
                                • best      - Show best stories\n  \
                                • new       - Show new stories\n  \
                                • ask       - Show Ask HN\n  \
                                • show      - Show Show HN\n  \
                                • jobs      - Show job postings\n  \
                                • open <n>  - Open story #n\n  \
                                • comments  - Open comments for story #n\n  \
                                • copy <n>  - Copy URL of story #n\n  \
                                • browse    - Interactive browser\n  \
                                • menu      - Interactive menu\n  \
                                • help      - Show help\n\n\
                                Example: lla plugin hackernews top",
                                action
                            )),
                        };
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::GetAvailableActions => {
                        use lla_plugin_interface::ActionInfo;
                        PluginResponse::AvailableActions(vec![
                            ActionInfo {
                                name: "top".to_string(),
                                usage: "top".to_string(),
                                description: "Show top stories".to_string(),
                                examples: vec!["lla plugin hackernews top".to_string()],
                            },
                            ActionInfo {
                                name: "best".to_string(),
                                usage: "best".to_string(),
                                description: "Show best stories".to_string(),
                                examples: vec!["lla plugin hackernews best".to_string()],
                            },
                            ActionInfo {
                                name: "new".to_string(),
                                usage: "new".to_string(),
                                description: "Show new stories".to_string(),
                                examples: vec!["lla plugin hackernews new".to_string()],
                            },
                            ActionInfo {
                                name: "ask".to_string(),
                                usage: "ask".to_string(),
                                description: "Show Ask HN".to_string(),
                                examples: vec!["lla plugin hackernews ask".to_string()],
                            },
                            ActionInfo {
                                name: "show".to_string(),
                                usage: "show".to_string(),
                                description: "Show Show HN".to_string(),
                                examples: vec!["lla plugin hackernews show".to_string()],
                            },
                            ActionInfo {
                                name: "jobs".to_string(),
                                usage: "jobs".to_string(),
                                description: "Show job postings".to_string(),
                                examples: vec!["lla plugin hackernews jobs".to_string()],
                            },
                            ActionInfo {
                                name: "open".to_string(),
                                usage: "open <number>".to_string(),
                                description: "Open story in browser".to_string(),
                                examples: vec!["lla plugin hackernews open 1".to_string()],
                            },
                            ActionInfo {
                                name: "comments".to_string(),
                                usage: "comments <number>".to_string(),
                                description: "Open story comments".to_string(),
                                examples: vec!["lla plugin hackernews comments 1".to_string()],
                            },
                            ActionInfo {
                                name: "copy".to_string(),
                                usage: "copy <number>".to_string(),
                                description: "Copy story URL".to_string(),
                                examples: vec!["lla plugin hackernews copy 1".to_string()],
                            },
                            ActionInfo {
                                name: "browse".to_string(),
                                usage: "browse [topic]".to_string(),
                                description: "Interactive browser".to_string(),
                                examples: vec!["lla plugin hackernews browse".to_string()],
                            },
                            ActionInfo {
                                name: "menu".to_string(),
                                usage: "menu".to_string(),
                                description: "Interactive menu".to_string(),
                                examples: vec!["lla plugin hackernews menu".to_string()],
                            },
                            ActionInfo {
                                name: "help".to_string(),
                                usage: "help".to_string(),
                                description: "Show help information".to_string(),
                                examples: vec!["lla plugin hackernews help".to_string()],
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

impl Default for HackerNewsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for HackerNewsPlugin {
    type Config = HackerNewsConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for HackerNewsPlugin {}

lla_plugin_interface::declare_plugin!(HackerNewsPlugin);

