use arboard::Clipboard;
use base64::Engine as _;
use chrono::{TimeZone, Utc};
use colored::Colorize;
use dialoguer::{MultiSelect, Select};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use lazy_static::lazy_static;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use ring::digest;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    ops::Deref,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{Style, ThemeSet},
    parsing::SyntaxSet,
    util::{as_24_bit_terminal_escaped, LinesWithEndings},
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
    #[serde(default = "default_syntax_themes")]
    syntax_themes: HashMap<String, String>,
    #[serde(default = "default_max_preview_lines")]
    max_preview_lines: usize,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("error".to_string(), "bright_red".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors.insert("language".to_string(), "bright_cyan".to_string());
    colors.insert("tag".to_string(), "bright_magenta".to_string());
    colors
}

fn default_syntax_themes() -> HashMap<String, String> {
    let mut themes = HashMap::new();
    themes.insert("default".to_string(), "Solarized (dark)".to_string());
    themes
}

fn default_max_preview_lines() -> usize {
    10
}

impl Default for SnippetConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
            syntax_themes: default_syntax_themes(),
            max_preview_lines: default_max_preview_lines(),
        }
    }
}

impl PluginConfig for SnippetConfig {}

#[derive(Clone, Serialize, Deserialize)]
struct CodeSnippet {
    id: String,
    name: String,
    content: String,
    language: String,
    tags: HashSet<String>,
    created_at: u64,
    modified_at: u64,
    version: u32,
    context_before: Option<String>,
    context_after: Option<String>,
    hash: String,
    source_file: String,
    category: Option<String>,
}

impl CodeSnippet {
    fn new(name: String, content: String, language: String, source_file: String) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let hash = Self::compute_hash(&content);
        let id = Uuid::new_v4().to_string();

        Self {
            id,
            name,
            content,
            language,
            tags: HashSet::new(),
            created_at: timestamp,
            modified_at: timestamp,
            version: 1,
            context_before: None,
            context_after: None,
            hash,
            source_file,
            category: None,
        }
    }

    fn compute_hash(content: &str) -> String {
        let hash = digest::digest(&digest::SHA256, content.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(hash.as_ref())
    }
}

lazy_static! {
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "extract",
            "extract <file_path> <snippet_name> <start_line> <end_line> [context_lines]",
            "Extract a code snippet from a file",
            vec!["lla plugin --name code_snippet_extractor --action extract --args \"file.rs\" \"my_func\" 10 20"],
            |args| CodeSnippetExtractorPlugin::extract_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "list",
            "list [file_path]",
            "List all snippets, optionally filtered by file",
            vec![
                "lla plugin --name code_snippet_extractor --action list",
                "lla plugin --name code_snippet_extractor --action list --args \"file.rs\""
            ],
            |args| CodeSnippetExtractorPlugin::list_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "get",
            "get <snippet_id>",
            "Get a specific snippet by ID",
            vec!["lla plugin --name code_snippet_extractor --action get --args \"abc123\""],
            |args| CodeSnippetExtractorPlugin::get_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "search",
            "search <query>",
            "Search through all snippets",
            vec!["lla plugin --name code_snippet_extractor --action search --args \"function\""],
            |args| CodeSnippetExtractorPlugin::search_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "add-tags",
            "add-tags <snippet_id> <tag1> [tag2...]",
            "Add tags to a snippet",
            vec!["lla plugin --name code_snippet_extractor --action add-tags --args \"abc123\" \"rust\" \"function\""],
            |args| CodeSnippetExtractorPlugin::add_tags_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "remove-tags",
            "remove-tags <snippet_id> <tag1> [tag2...]",
            "Remove tags from a snippet",
            vec!["lla plugin --name code_snippet_extractor --action remove-tags --args \"abc123\" \"rust\""],
            |args| CodeSnippetExtractorPlugin::remove_tags_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name code_snippet_extractor --action help"],
            |_| CodeSnippetExtractorPlugin::help_action()
        );

        lla_plugin_utils::define_action!(
            registry,
            "list-categories",
            "list-categories",
            "List all available categories",
            vec!["lla plugin --name code_snippet_extractor --action list-categories"],
            |_| CodeSnippetExtractorPlugin::list_categories_action()
        );

        lla_plugin_utils::define_action!(
            registry,
            "list-by-category",
            "list-by-category <category>",
            "List all snippets in a category",
            vec!["lla plugin --name code_snippet_extractor --action list-by-category --args \"algorithms\""],
            |args| CodeSnippetExtractorPlugin::list_by_category_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "set-category",
            "set-category <snippet_id> <category>",
            "Set or change the category of a snippet",
            vec!["lla plugin --name code_snippet_extractor --action set-category --args \"abc123\" \"algorithms\""],
            |args| CodeSnippetExtractorPlugin::set_category_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "export",
            "export <file_path> [snippet_ids...]",
            "Export snippets to a JSON file",
            vec![
                "lla plugin --name code_snippet_extractor --action export --args \"snippets.json\"",
                "lla plugin --name code_snippet_extractor --action export --args \"snippets.json\" \"abc123\" \"def456\""
            ],
            |args| CodeSnippetExtractorPlugin::export_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "import",
            "import <file_path>",
            "Import snippets from a JSON file",
            vec![
                "lla plugin --name code_snippet_extractor --action import --args \"snippets.json\""
            ],
            |args| CodeSnippetExtractorPlugin::import_action(args)
        );

        registry
    });
}

pub struct CodeSnippetExtractorPlugin {
    base: BasePlugin<SnippetConfig>,
    snippets: HashMap<String, CodeSnippet>,
}

impl CodeSnippetExtractorPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
            snippets: Self::load_snippets(&Self::get_snippets_path()),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[CodeSnippetExtractorPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn get_snippets_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("lla")
            .join("code_snippets.toml")
    }

    fn load_snippets(path: &PathBuf) -> HashMap<String, CodeSnippet> {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(snippets) = toml::from_str(&content) {
                return snippets;
            }
        }
        HashMap::new()
    }

    fn save_snippets(&self) -> Result<(), String> {
        let snippet_file = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("lla")
            .join("code_snippets.toml");

        if let Some(parent) = snippet_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(&self.snippets)
            .map_err(|e| format!("Failed to serialize snippets: {}", e))?;
        std::fs::write(&snippet_file, content).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn detect_language(file_path: &str) -> String {
        match file_path.split('.').last() {
            Some("rs") => "rust",
            Some("py") => "python",
            Some("js") => "javascript",
            Some("ts") => "typescript",
            Some("go") => "go",
            Some("c") => "c",
            Some("cpp") | Some("cc") | Some("cxx") => "cpp",
            Some("java") => "java",
            Some("rb") => "ruby",
            Some("php") => "php",
            Some("sh") => "shell",
            Some("html") => "html",
            Some("css") => "css",
            Some("md") => "markdown",
            Some("json") => "json",
            Some("yaml") | Some("yml") => "yaml",
            Some("xml") => "xml",
            Some("sql") => "sql",
            _ => "text",
        }
        .to_string()
    }

    fn extract_snippet(
        &mut self,
        file_path: &str,
        name: &str,
        start_line: usize,
        end_line: usize,
        context_lines: Option<usize>,
    ) -> Result<String, String> {
        let file = File::open(file_path).map_err(|e| format!("Failed to open file: {}", e))?;
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

        if start_line > end_line || end_line > lines.len() {
            return Err("Invalid line range".to_string());
        }

        let context_lines = context_lines.unwrap_or(3);
        let context_before = if start_line > context_lines {
            Some(lines[start_line - context_lines - 1..start_line - 1].join("\n"))
        } else {
            Some(lines[0..start_line - 1].join("\n"))
        };

        let context_after = if end_line + context_lines <= lines.len() {
            Some(lines[end_line..end_line + context_lines].join("\n"))
        } else {
            Some(lines[end_line..].join("\n"))
        };

        let content = lines[start_line - 1..end_line].join("\n");
        let language = Self::detect_language(file_path);
        let mut snippet =
            CodeSnippet::new(name.to_string(), content, language, file_path.to_string());
        snippet.context_before = context_before;
        snippet.context_after = context_after;

        let id = snippet.id.clone();
        self.snippets.insert(id.clone(), snippet);
        self.save_snippets()?;
        Ok(id)
    }

    fn get_snippet(&self, id: &str) -> Option<&CodeSnippet> {
        self.snippets.get(id)
    }

    fn list_snippets(&self) -> Vec<&CodeSnippet> {
        self.snippets.values().collect()
    }

    fn list_snippets_by_file(&self, file_path: &str) -> Vec<&CodeSnippet> {
        self.snippets
            .values()
            .filter(|s| s.source_file == file_path)
            .collect()
    }

    fn search_snippets(&self, query: &str) -> Vec<&CodeSnippet> {
        let matcher = SkimMatcherV2::default();
        let query = query.to_lowercase();
        let mut matches: Vec<(&CodeSnippet, i64)> = self
            .snippets
            .values()
            .filter_map(|s| {
                let name_score = matcher
                    .fuzzy_match(&s.name.to_lowercase(), &query)
                    .unwrap_or(0);
                let content_score = matcher
                    .fuzzy_match(&s.content.to_lowercase(), &query)
                    .unwrap_or(0);
                let tags_score = s
                    .tags
                    .iter()
                    .filter_map(|t| matcher.fuzzy_match(&t.to_lowercase(), &query))
                    .max()
                    .unwrap_or(0);
                let source_score = matcher
                    .fuzzy_match(&s.source_file.to_lowercase(), &query)
                    .unwrap_or(0);

                let total_score = name_score + content_score + tags_score + source_score;
                if total_score > 0 {
                    Some((s, total_score))
                } else {
                    None
                }
            })
            .collect();

        matches.sort_by(|a, b| b.1.cmp(&a.1));
        matches.into_iter().map(|(snippet, _)| snippet).collect()
    }

    fn add_tags(&mut self, id: &str, tags: &[String]) -> Result<(), String> {
        let snippet = self.snippets.get_mut(id).ok_or("Snippet not found")?;
        snippet.tags.extend(tags.iter().cloned());
        self.save_snippets()?;
        Ok(())
    }

    fn remove_tags(&mut self, id: &str, tags: &[String]) -> Result<(), String> {
        let snippet = self.snippets.get_mut(id).ok_or("Snippet not found")?;
        for tag in tags {
            snippet.tags.remove(tag);
        }
        self.save_snippets()?;
        Ok(())
    }

    fn format_timestamp(&self, timestamp: u64) -> String {
        if let Some(datetime) = Utc.timestamp_opt(timestamp as i64, 0).single() {
            datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
        } else {
            "Invalid timestamp".to_string()
        }
    }

    fn count_lines(text: &str) -> usize {
        text.lines().count()
    }

    fn truncate_str(&self, s: &str, max_width: usize) -> String {
        if console::measure_text_width(s) <= max_width {
            s.to_string()
        } else {
            let mut width = 0;
            let mut result = String::new();
            let mut chars = s.chars();

            while let Some(c) = chars.next() {
                let char_width = console::measure_text_width(&c.to_string());
                if width + char_width + 3 > max_width {
                    result.push_str("...");
                    break;
                }
                width += char_width;
                result.push(c);
            }
            result
        }
    }

    fn render_snippet(&self, snippet: &CodeSnippet) -> String {
        let colors = &self.base.config().colors;
        let mut output = String::new();
        let max_width = 100;
        let content_width = max_width - 8;

        output.push_str(&format!("─{}─\n", "─".bright_black().repeat(max_width - 2)));
        output.push_str(&format!(
            " {} \n",
            self.truncate_str(&snippet.name.bright_white().to_string(), max_width - 3)
        ));

        let metadata = format!(
            " {} {}  •  {} {}  •  {} {}",
            "ID:".bright_yellow(),
            snippet.id.bright_magenta(),
            "Language:".bright_yellow(),
            snippet.language.bright_cyan(),
            "Version:".bright_yellow(),
            format!("v{}", snippet.version).bright_white()
        );
        output.push_str(&format!(
            "{}\n",
            self.truncate_str(&metadata, max_width - 1)
        ));

        output.push_str(&format!("─{}─\n", "─".bright_black().repeat(max_width - 2)));
        let source_info = format!(
            " 📂 {}  {}",
            "Source:".bright_yellow(),
            snippet.source_file.bright_blue()
        );
        output.push_str(&format!(
            "{}\n",
            self.truncate_str(&source_info, max_width - 1)
        ));

        let tag_list = if snippet.tags.is_empty() {
            "No tags".dimmed().to_string()
        } else {
            snippet
                .tags
                .iter()
                .map(|t| {
                    format!(
                        "#{}",
                        t.color(colors.get("tag").unwrap_or(&"white".to_string()).as_str())
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
        };

        let tags_info = format!(" 🏷️  {}  {}", "Tags:".bright_yellow(), tag_list);
        output.push_str(&format!(
            "{}\n",
            self.truncate_str(&tags_info, max_width - 1)
        ));

        let category_info = format!(
            " 📁 {}  {}",
            "Category:".bright_yellow(),
            snippet
                .category
                .as_ref()
                .map(|c| c.bright_cyan().to_string())
                .unwrap_or_else(|| "None".dimmed().to_string())
        );
        output.push_str(&format!(
            "{}\n",
            self.truncate_str(&category_info, max_width - 1)
        ));

        let timestamps = format!(
            " 🕒 Created: {}  •  Modified: {}",
            self.format_timestamp(snippet.created_at).bright_white(),
            self.format_timestamp(snippet.modified_at).bright_white()
        );
        output.push_str(&format!(
            "{}\n",
            self.truncate_str(&timestamps, max_width - 1)
        ));

        if let Some(ctx) = &snippet.context_before {
            let ctx_lines = Self::count_lines(ctx);
            output.push_str(&format!("─{}─\n", "─".bright_black().repeat(max_width - 2)));
            output.push_str(&format!(
                " {} {}\n",
                "◀ Context".bright_blue(),
                format!("({} lines)", ctx_lines).bright_black()
            ));

            for (i, line) in ctx.lines().enumerate() {
                let line_num = format!(" {:>4} │ ", -(ctx_lines as i32 - i as i32)).bright_black();
                let truncated_line =
                    self.truncate_str(&line.bright_black().to_string(), content_width);
                output.push_str(&format!("{}{}\n", line_num, truncated_line));
            }
        }

        let content_lines = snippet.content.lines().count();
        output.push_str(&format!("─{}─\n", "─".bright_black().repeat(max_width - 2)));
        output.push_str(&format!(
            " {} {}\n",
            "▶ Code".bright_green(),
            format!("({} lines)", content_lines).bright_black()
        ));

        let highlighted = self.highlight_code(&snippet.content, &snippet.language);
        for (i, line) in highlighted.lines().enumerate() {
            let line_num = format!(" {:>4} │ ", i + 1).bright_black();
            let truncated_line = self.truncate_str(line, content_width);
            output.push_str(&format!("{}{}\n", line_num, truncated_line));
        }

        if let Some(ctx) = &snippet.context_after {
            let ctx_lines = Self::count_lines(ctx);
            output.push_str(&format!("─{}─\n", "─".bright_black().repeat(max_width - 2)));
            output.push_str(&format!(
                " {} {}\n",
                "▼ Context".bright_blue(),
                format!("({} lines)", ctx_lines).bright_black()
            ));

            for (i, line) in ctx.lines().enumerate() {
                let line_num = format!(" {:>4} │ ", content_lines + i + 1).bright_black();
                let truncated_line =
                    self.truncate_str(&line.bright_black().to_string(), content_width);
                output.push_str(&format!("{}{}\n", line_num, truncated_line));
            }
        }

        output.push_str(&format!("─{}─\n", "─".bright_black().repeat(max_width - 2)));
        let stats = format!(
            " 📊 {} characters in {} lines",
            snippet.content.len(),
            content_lines
        );
        output.push_str(&format!("{}\n", self.truncate_str(&stats, max_width - 1)));

        output
    }

    fn extract_action(args: &[String]) -> Result<(), String> {
        if args.len() < 4 || args.len() > 5 {
            return Err(
                "Usage: extract <file_path> <snippet_name> <start_line> <end_line> [context_lines]"
                    .to_string(),
            );
        }

        let start_line = args[2]
            .parse::<usize>()
            .map_err(|_| "Invalid start line".to_string())?;
        let end_line = args[3]
            .parse::<usize>()
            .map_err(|_| "Invalid end line".to_string())?;
        let context_lines = args.get(4).and_then(|s| s.parse().ok());

        let mut plugin = Self::new();
        let snippet_id =
            plugin.extract_snippet(&args[0], &args[1], start_line, end_line, context_lines)?;

        println!(
            "{} extracted snippet '{}' from {} (lines {}-{}) with ID: {}",
            "Successfully".bright_green(),
            args[1].bright_yellow(),
            args[0].bright_blue(),
            start_line.to_string().bright_cyan(),
            end_line.to_string().bright_cyan(),
            snippet_id.bright_magenta()
        );
        Ok(())
    }

    fn list_action(args: &[String]) -> Result<(), String> {
        let plugin = Self::new();
        let snippets = if let Some(file_path) = args.get(0) {
            plugin.list_snippets_by_file(file_path)
        } else {
            plugin.list_snippets()
        };

        if snippets.is_empty() {
            println!("{} No snippets found", "Info:".bright_blue(),);
        } else {
            println!("{} snippets:", "Found".bright_green());
            for snippet in snippets {
                println!(
                    "  {} [{}] {} [{}] {}",
                    "→".bright_cyan(),
                    snippet.id.bright_magenta(),
                    snippet.name.bright_yellow(),
                    snippet.language.bright_blue(),
                    snippet
                        .tags
                        .iter()
                        .map(|t| format!("#{}", t.bright_magenta()))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
        }
        Ok(())
    }

    fn get_action(args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Usage: get <snippet_id>".to_string());
        }

        let plugin = Self::new();
        let snippet = plugin
            .get_snippet(&args[0])
            .ok_or("Snippet not found".to_string())?;

        println!("{}", plugin.render_snippet(snippet));
        Ok(())
    }

    fn search_action(args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Usage: search <query>".to_string());
        }

        let plugin = Self::new();
        let results = plugin.search_snippets(&args[0]);

        if results.is_empty() {
            println!(
                "{} No matching snippets found for query: {}",
                "Info:".bright_blue(),
                args[0].bright_yellow()
            );
            return Ok(());
        }

        println!(
            "{} snippets for query: {}",
            "Found".bright_green(),
            args[0].bright_yellow()
        );

        let selection_items: Vec<String> = results
            .iter()
            .map(|snippet| {
                format!(
                    "[{}] {} [{}] {} {} from {}",
                    snippet.id.bright_magenta(),
                    snippet.name.bright_yellow(),
                    snippet.language.bright_blue(),
                    snippet
                        .category
                        .as_ref()
                        .map(|c| format!("({})", c.bright_cyan()))
                        .unwrap_or_default(),
                    snippet
                        .tags
                        .iter()
                        .map(|t| format!("#{}", t.bright_magenta()))
                        .collect::<Vec<_>>()
                        .join(" "),
                    snippet.source_file.bright_blue()
                )
            })
            .collect();

        let theme = LlaDialoguerTheme::default();
        let selection = MultiSelect::with_theme(&theme)
            .with_prompt(
                "Select snippets (Space to select, Enter to confirm, Arrow keys to navigate)",
            )
            .items(&selection_items)
            .defaults(&vec![false; selection_items.len()])
            .interact()
            .map_err(|e| format!("Failed to show selector: {}", e))?;

        if selection.is_empty() {
            return Ok(());
        }

        let selected_snippets: Vec<&CodeSnippet> = selection.iter().map(|&i| results[i]).collect();

        let actions = vec![
            "View snippets",
            "Copy to clipboard",
            "Add tags",
            "Remove tags",
            "Set category",
        ];

        let action_selection = Select::with_theme(&LlaDialoguerTheme::default())
            .with_prompt("Choose action")
            .items(&actions)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show action menu: {}", e))?;

        match action_selection {
            0 => {
                for snippet in selected_snippets {
                    println!("\n{}", plugin.render_snippet(snippet));
                }
            }
            1 => {
                let content = selected_snippets
                    .iter()
                    .map(|s| s.content.clone())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                plugin.copy_to_clipboard(&content)?;
                println!(
                    "{} Snippets copied to clipboard!",
                    "Success:".bright_green()
                );
            }
            2 => {
                let input = dialoguer::Input::<String>::with_theme(&LlaDialoguerTheme::default())
                    .with_prompt("Enter tags (space-separated)")
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;
                let tags: Vec<String> = input.split_whitespace().map(String::from).collect();
                let ids: Vec<String> = selected_snippets.iter().map(|s| s.id.clone()).collect();
                let mut plugin = Self::new();
                plugin.batch_add_tags(&ids, &tags)?;
                println!(
                    "{} Added tags to {} snippets",
                    "Success:".bright_green(),
                    ids.len()
                );
            }
            3 => {
                let input = dialoguer::Input::<String>::with_theme(&LlaDialoguerTheme::default())
                    .with_prompt("Enter tags to remove (space-separated)")
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;
                let tags: Vec<String> = input.split_whitespace().map(String::from).collect();
                let ids: Vec<String> = selected_snippets.iter().map(|s| s.id.clone()).collect();
                let mut plugin = Self::new();
                plugin.batch_remove_tags(&ids, &tags)?;
                println!(
                    "{} Removed tags from {} snippets",
                    "Success:".bright_green(),
                    ids.len()
                );
            }
            4 => {
                let categories: Vec<String> = plugin.list_categories().into_iter().collect();
                let mut category_items = vec!["(None)".to_string(), "(New category)".to_string()];
                category_items.extend(categories);

                let category_selection = Select::with_theme(&LlaDialoguerTheme::default())
                    .with_prompt("Choose category")
                    .items(&category_items)
                    .default(0)
                    .interact()
                    .map_err(|e| format!("Failed to show category menu: {}", e))?;

                let category = match category_selection {
                    0 => None,
                    1 => {
                        let input =
                            dialoguer::Input::<String>::with_theme(&LlaDialoguerTheme::default())
                                .with_prompt("Enter new category name")
                                .interact_text()
                                .map_err(|e| format!("Failed to get input: {}", e))?;
                        Some(input)
                    }
                    i => Some(category_items[i].clone()),
                };

                let ids: Vec<String> = selected_snippets.iter().map(|s| s.id.clone()).collect();
                let mut plugin = Self::new();
                plugin.batch_set_category(&ids, category.clone())?;
                println!(
                    "{} Set category {} for {} snippets",
                    "Success:".bright_green(),
                    category.unwrap_or_else(|| "None".to_string()),
                    ids.len()
                );
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    fn add_tags_action(args: &[String]) -> Result<(), String> {
        if args.len() < 2 {
            return Err("Usage: add-tags <snippet_id> <tag1> [tag2...]".to_string());
        }

        let mut plugin = Self::new();
        let tags: Vec<String> = args[1..].iter().map(|s| s.to_string()).collect();
        plugin.add_tags(&args[0], &tags)?;

        println!(
            "{} tags {} to snippet",
            "Added".bright_green(),
            tags.iter()
                .map(|t| format!("#{}", t.bright_magenta()))
                .collect::<Vec<_>>()
                .join(" ")
        );
        Ok(())
    }

    fn remove_tags_action(args: &[String]) -> Result<(), String> {
        if args.len() < 2 {
            return Err("Usage: remove-tags <snippet_id> <tag1> [tag2...]".to_string());
        }

        let mut plugin = Self::new();
        let tags: Vec<String> = args[1..].iter().map(|s| s.to_string()).collect();
        plugin.remove_tags(&args[0], &tags)?;

        println!(
            "{} tags {} from snippet",
            "Removed".bright_green(),
            tags.iter()
                .map(|t| format!("#{}", t.bright_magenta()))
                .collect::<Vec<_>>()
                .join(" ")
        );
        Ok(())
    }

    fn help_action() -> Result<(), String> {
        let mut help = HelpFormatter::new("Code Snippet Extractor".to_string());
        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Extract, manage, and search code snippets with tagging support".to_string(),
            vec![],
        );

        help.add_section("Basic Commands".to_string())
            .add_command(
                "extract".to_string(),
                "Extract a code snippet from a file".to_string(),
                vec!["extract file.rs my_func 10 20".to_string()],
            )
            .add_command(
                "list".to_string(),
                "List all snippets, optionally filtered by file".to_string(),
                vec!["list".to_string(), "list file.rs".to_string()],
            )
            .add_command(
                "get".to_string(),
                "Get a specific snippet by ID".to_string(),
                vec!["get abc123".to_string()],
            );

        help.add_section("Search & Organization".to_string())
            .add_command(
                "search".to_string(),
                "Search through all snippets (with multi-select and batch operations)".to_string(),
                vec!["search function".to_string()],
            )
            .add_command(
                "add-tags".to_string(),
                "Add tags to a snippet".to_string(),
                vec!["add-tags abc123 rust function".to_string()],
            )
            .add_command(
                "remove-tags".to_string(),
                "Remove tags from a snippet".to_string(),
                vec!["remove-tags abc123 rust".to_string()],
            );

        help.add_section("Category Management".to_string())
            .add_command(
                "list-categories".to_string(),
                "List all available categories".to_string(),
                vec!["list-categories".to_string()],
            )
            .add_command(
                "list-by-category".to_string(),
                "List all snippets in a category".to_string(),
                vec!["list-by-category algorithms".to_string()],
            )
            .add_command(
                "set-category".to_string(),
                "Set or change the category of a snippet".to_string(),
                vec!["set-category abc123 algorithms".to_string()],
            );

        help.add_section("Import & Export".to_string())
            .add_command(
                "export".to_string(),
                "Export snippets to a JSON file".to_string(),
                vec![
                    "export snippets.json".to_string(),
                    "export snippets.json abc123 def456".to_string(),
                ],
            )
            .add_command(
                "import".to_string(),
                "Import snippets from a JSON file".to_string(),
                vec!["import snippets.json".to_string()],
            );

        println!(
            "{}",
            BoxComponent::new(help.render(&SnippetConfig::default().colors))
                .style(BoxStyle::Minimal)
                .padding(1)
                .render()
        );
        Ok(())
    }

    fn copy_to_clipboard(&self, content: &str) -> Result<(), String> {
        let mut clipboard =
            Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;
        clipboard
            .set_text(content)
            .map_err(|e| format!("Failed to copy to clipboard: {}", e))
    }

    fn set_category(&mut self, id: &str, category: Option<String>) -> Result<(), String> {
        let snippet = self.snippets.get_mut(id).ok_or("Snippet not found")?;
        snippet.category = category;
        self.save_snippets()?;
        Ok(())
    }

    fn list_categories(&self) -> HashSet<String> {
        self.snippets
            .values()
            .filter_map(|s| s.category.clone())
            .collect()
    }

    fn list_snippets_by_category(&self, category: &str) -> Vec<&CodeSnippet> {
        self.snippets
            .values()
            .filter(|s| s.category.as_deref() == Some(category))
            .collect()
    }

    fn batch_add_tags(&mut self, ids: &[String], tags: &[String]) -> Result<(), String> {
        for id in ids {
            self.add_tags(id, tags)?;
        }
        Ok(())
    }

    fn batch_remove_tags(&mut self, ids: &[String], tags: &[String]) -> Result<(), String> {
        for id in ids {
            self.remove_tags(id, tags)?;
        }
        Ok(())
    }

    fn batch_set_category(
        &mut self,
        ids: &[String],
        category: Option<String>,
    ) -> Result<(), String> {
        for id in ids {
            self.set_category(id, category.clone())?;
        }
        Ok(())
    }

    fn list_categories_action() -> Result<(), String> {
        let plugin = Self::new();
        let categories = plugin.list_categories();

        if categories.is_empty() {
            println!("{} No categories found", "Info:".bright_blue());
        } else {
            println!("{} categories:", "Found".bright_green());
            for category in categories {
                let snippets = plugin.list_snippets_by_category(&category);
                println!(
                    "  {} {} ({} snippets)",
                    "→".bright_cyan(),
                    category.bright_yellow(),
                    snippets.len().to_string().bright_blue()
                );
            }
        }
        Ok(())
    }

    fn list_by_category_action(args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Usage: list-by-category <category>".to_string());
        }

        let plugin = Self::new();
        let snippets = plugin.list_snippets_by_category(&args[0]);

        if snippets.is_empty() {
            println!(
                "{} No snippets found in category: {}",
                "Info:".bright_blue(),
                args[0].bright_yellow()
            );
        } else {
            println!(
                "{} snippets in category {}:",
                "Found".bright_green(),
                args[0].bright_yellow()
            );
            for snippet in snippets {
                println!(
                    "  {} [{}] {} [{}] {}",
                    "→".bright_cyan(),
                    snippet.id.bright_magenta(),
                    snippet.name.bright_yellow(),
                    snippet.language.bright_blue(),
                    snippet
                        .tags
                        .iter()
                        .map(|t| format!("#{}", t.bright_magenta()))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
        }
        Ok(())
    }

    fn set_category_action(args: &[String]) -> Result<(), String> {
        if args.len() != 2 {
            return Err("Usage: set-category <snippet_id> <category>".to_string());
        }

        let mut plugin = Self::new();
        let category = if args[1].to_lowercase() == "none" {
            None
        } else {
            Some(args[1].clone())
        };

        plugin.set_category(&args[0], category.clone())?;
        println!(
            "{} Set category {} for snippet",
            "Success:".bright_green(),
            category
                .unwrap_or_else(|| "None".to_string())
                .bright_yellow()
        );
        Ok(())
    }

    fn highlight_code(&self, code: &str, language: &str) -> String {
        lazy_static! {
            static ref PS: SyntaxSet = SyntaxSet::load_defaults_newlines();
            static ref TS: ThemeSet = ThemeSet::load_defaults();
        }

        let syntax = PS
            .find_syntax_by_token(language)
            .unwrap_or_else(|| PS.find_syntax_plain_text());
        let mut h = HighlightLines::new(syntax, &TS.themes["base16-ocean.dark"]);

        let mut highlighted = String::new();
        for line in LinesWithEndings::from(code) {
            let ranges: Vec<(Style, &str)> = h.highlight_line(line, &PS).unwrap_or_default();
            let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
            highlighted.push_str(&escaped);
        }
        highlighted
    }

    fn export_snippets(&self, path: &str, ids: &[String]) -> Result<(), String> {
        let snippets: Vec<&CodeSnippet> =
            ids.iter().filter_map(|id| self.snippets.get(id)).collect();

        let content = serde_json::to_string_pretty(&snippets)
            .map_err(|e| format!("Failed to serialize snippets: {}", e))?;

        std::fs::write(path, content).map_err(|e| format!("Failed to write file: {}", e))?;
        Ok(())
    }

    fn import_snippets(&mut self, path: &str) -> Result<Vec<String>, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

        let imported_snippets: Vec<CodeSnippet> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse snippets: {}", e))?;

        let mut imported_ids = Vec::new();
        for mut snippet in imported_snippets {
            snippet.id = Uuid::new_v4().to_string();
            imported_ids.push(snippet.id.clone());
            self.snippets.insert(snippet.id.clone(), snippet);
        }

        self.save_snippets()?;
        Ok(imported_ids)
    }

    fn export_action(args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Usage: export <file_path> [snippet_ids...]".to_string());
        }

        let plugin = Self::new();
        let export_path = &args[0];
        let snippet_ids = if args.len() > 1 {
            args[1..].to_vec()
        } else {
            plugin.snippets.keys().cloned().collect()
        };

        plugin.export_snippets(export_path, &snippet_ids)?;
        println!(
            "{} Exported {} snippets to {}",
            "Success:".bright_green(),
            snippet_ids.len(),
            export_path.bright_blue()
        );
        Ok(())
    }

    fn import_action(args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Usage: import <file_path>".to_string());
        }

        let mut plugin = Self::new();
        let import_path = &args[0];
        let imported_ids = plugin.import_snippets(import_path)?;

        println!(
            "{} Imported {} snippets from {}",
            "Success:".bright_green(),
            imported_ids.len(),
            import_path.bright_blue()
        );

        println!("\n{} Imported snippets:", "Info:".bright_blue());
        for id in &imported_ids {
            if let Some(snippet) = plugin.get_snippet(id) {
                println!(
                    "  {} [{}] {} [{}] {}",
                    "→".bright_cyan(),
                    snippet.id.bright_magenta(),
                    snippet.name.bright_yellow(),
                    snippet.language.bright_blue(),
                    snippet
                        .tags
                        .iter()
                        .map(|t| format!("#{}", t.bright_magenta()))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
        }

        Ok(())
    }
}

impl Deref for CodeSnippetExtractorPlugin {
    type Target = SnippetConfig;

    fn deref(&self) -> &Self::Target {
        self.base.config()
    }
}

impl Plugin for CodeSnippetExtractorPlugin {
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
                        if let Some(file_path) = entry.path.to_str() {
                            let snippet_count = self.list_snippets_by_file(file_path).len();
                            if snippet_count > 0 {
                                entry.custom_fields.insert(
                                    "snippet_count".to_string(),
                                    format!("[{} snippets]", snippet_count),
                                );
                            }
                        }
                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::FormatField(entry, format) => {
                        let field = if format == "snippet_count" {
                            entry.custom_fields.get("snippet_count").cloned()
                        } else {
                            None
                        };
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

impl ConfigurablePlugin for CodeSnippetExtractorPlugin {
    type Config = SnippetConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for CodeSnippetExtractorPlugin {}

lla_plugin_interface::declare_plugin!(CodeSnippetExtractorPlugin);

impl Default for CodeSnippetExtractorPlugin {
    fn default() -> Self {
        Self::new()
    }
}
