use arboard::Clipboard;
use colored::Colorize;
use dialoguer::{MultiSelect, Select};
use itertools::Itertools;
use lazy_static::lazy_static;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{Style, ThemeSet},
    parsing::SyntaxSet,
    util::as_24_bit_terminal_escaped,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeywordMatch {
    keyword: String,
    line_number: usize,
    line: String,
    context_before: Vec<String>,
    context_after: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub keywords: Vec<String>,
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub context_lines: usize,
    pub max_matches: usize,
    pub file_extensions: Vec<String>,
    #[serde(default = "default_colors")]
    pub colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("keyword".to_string(), "bright_red".to_string());
    colors.insert("line_number".to_string(), "bright_yellow".to_string());
    colors.insert("context".to_string(), "bright_black".to_string());
    colors.insert("file".to_string(), "bright_blue".to_string());
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_cyan".to_string());
    colors
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            keywords: Vec::new(),
            case_sensitive: false,
            use_regex: false,
            context_lines: 2,
            max_matches: 5,
            file_extensions: vec![
                "txt".to_string(),
                "md".to_string(),
                "rs".to_string(),
                "py".to_string(),
                "js".to_string(),
                "java".to_string(),
                "c".to_string(),
                "cpp".to_string(),
                "h".to_string(),
                "hpp".to_string(),
                "go".to_string(),
                "rb".to_string(),
                "php".to_string(),
                "html".to_string(),
                "css".to_string(),
                "json".to_string(),
                "yaml".to_string(),
                "yml".to_string(),
                "toml".to_string(),
                "ini".to_string(),
                "conf".to_string(),
            ],
            colors: default_colors(),
        }
    }
}

impl PluginConfig for SearchConfig {}

pub struct KeywordSearchPlugin {
    base: BasePlugin<SearchConfig>,
}

impl KeywordSearchPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[KeywordSearchPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn highlight_match(&self, line: &str, keyword: &str) -> String {
        lazy_static! {
            static ref PS: SyntaxSet = SyntaxSet::load_defaults_newlines();
            static ref TS: ThemeSet = ThemeSet::load_defaults();
        }

        let syntax = PS.find_syntax_plain_text();
        let mut h = HighlightLines::new(syntax, &TS.themes["base16-ocean.dark"]);
        let ranges: Vec<(Style, &str)> = h.highlight_line(line, &PS).unwrap_or_default();
        let highlighted = as_24_bit_terminal_escaped(&ranges[..], false);
        let mut result = highlighted.clone();

        if let Some(pattern) = RegexBuilder::new(&regex::escape(keyword))
            .case_insensitive(true)
            .build()
            .ok()
        {
            for mat in pattern.find_iter(&highlighted) {
                let matched_text = &highlighted[mat.start()..mat.end()];
                result.replace_range(
                    mat.start()..mat.end(),
                    &matched_text.bright_red().to_string(),
                );
            }
        }

        result
    }

    fn render_match(&self, m: &KeywordMatch, file_path: &str) -> String {
        let mut output = String::new();
        let separator_width = 98;
        let line_number_width = 4;
        let prefix_width = line_number_width + 3;
        let max_line_width = separator_width - prefix_width;

        output.push_str(&format!("─{}─\n", "─".repeat(separator_width)));
        let truncated_path = if file_path.len() > max_line_width {
            format!("{}...", &file_path[..max_line_width - 3])
        } else {
            file_path.to_string()
        };
        output.push_str(&format!(" 📂 {}\n", truncated_path.bright_blue()));
        output.push_str(&format!("─{}─\n", "─".repeat(separator_width)));

        let format_line = |line: &str, is_match: bool| -> String {
            let stripped = strip_ansi_escapes::strip(line.as_bytes());
            let visible_len = String::from_utf8_lossy(&stripped).chars().count();

            if visible_len > max_line_width {
                let mut truncated = String::new();
                let mut current_len = 0;
                let mut chars = line.chars();

                while let Some(c) = chars.next() {
                    if !c.is_ascii_control() {
                        current_len += 1;
                    }
                    truncated.push(c);
                    if current_len >= max_line_width - 3 {
                        break;
                    }
                }

                if is_match {
                    format!("{}...", truncated)
                } else {
                    format!("{}...", truncated).bright_black().to_string()
                }
            } else if is_match {
                line.to_string()
            } else {
                line.bright_black().to_string()
            }
        };

        for (i, line) in m.context_before.iter().enumerate() {
            let line_num = m.line_number - (m.context_before.len() - i);
            output.push_str(&format!(
                " {:>4} │ {}\n",
                format!("{}", line_num).bright_black(),
                format_line(line, false)
            ));
        }

        let highlighted = self.highlight_match(&m.line, &m.keyword);
        output.push_str(&format!(
            " {:>4} │ {}\n",
            format!("{}", m.line_number).bright_yellow(),
            format_line(&highlighted, true)
        ));

        for (i, line) in m.context_after.iter().enumerate() {
            let line_num = m.line_number + i + 1;
            output.push_str(&format!(
                " {:>4} │ {}\n",
                format!("{}", line_num).bright_black(),
                format_line(line, false)
            ));
        }

        output.push_str(&format!("─{}─\n", "─".repeat(separator_width)));
        output
    }

    fn search_file(&self, path: &std::path::Path) -> Option<Vec<KeywordMatch>> {
        let config = self.base.config();

        if let Some(ext) = path.extension() {
            if !config
                .file_extensions
                .contains(&ext.to_string_lossy().to_string())
            {
                return None;
            }
        }

        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

        let mut matches = Vec::new();
        let patterns: Vec<_> = config
            .keywords
            .iter()
            .map(|k| {
                RegexBuilder::new(&regex::escape(k))
                    .case_insensitive(!config.case_sensitive)
                    .build()
                    .ok()
            })
            .collect();

        for (index, line) in lines.iter().enumerate() {
            for (pattern_index, pattern) in patterns.iter().enumerate() {
                if let Some(pattern) = pattern {
                    if pattern.is_match(line) {
                        let context_start = index.saturating_sub(config.context_lines);
                        let context_end = (index + config.context_lines + 1).min(lines.len());

                        matches.push(KeywordMatch {
                            keyword: config.keywords[pattern_index].clone(),
                            line_number: index + 1,
                            line: line.clone(),
                            context_before: lines[context_start..index].to_vec(),
                            context_after: lines[index + 1..context_end].to_vec(),
                        });

                        if matches.len() >= config.max_matches {
                            return Some(matches);
                        }
                    }
                }
            }
        }

        if matches.is_empty() {
            None
        } else {
            Some(matches)
        }
    }

    fn interactive_search(
        &self,
        matches: Vec<KeywordMatch>,
        file_path: &str,
    ) -> Result<(), String> {
        let items: Vec<String> = matches
            .iter()
            .map(|m| {
                format!(
                    "{} Line {}: {} {}",
                    "►".bright_cyan(),
                    m.line_number.to_string().bright_yellow(),
                    self.highlight_match(&m.line, &m.keyword),
                    format!("[{}]", m.keyword).bright_magenta()
                )
            })
            .collect();

        let theme = LlaDialoguerTheme::default();
        let selection = MultiSelect::with_theme(&theme)
            .with_prompt(format!("{} Select matches to process", "🔍".bright_cyan()))
            .items(&items)
            .defaults(&vec![true; items.len()])
            .interact()
            .map_err(|e| format!("Failed to show selector: {}", e))?;

        if selection.is_empty() {
            println!("{} No matches selected", "Info:".bright_blue());
            return Ok(());
        }

        let selected_matches: Vec<&KeywordMatch> = selection.iter().map(|&i| &matches[i]).collect();

        let actions = vec![
            "📝 View detailed matches",
            "📋 Copy to clipboard",
            "💾 Save to file",
            "📊 Show statistics",
            "🔍 Filter matches",
            "📈 Advanced analysis",
        ];

        let theme = LlaDialoguerTheme::default();
        let action_selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Choose action", "⚡".bright_cyan()))
            .items(&actions)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show action menu: {}", e))?;

        match action_selection {
            0 => {
                println!(
                    "\n{} Showing {} selected matches:",
                    "Info:".bright_blue(),
                    selected_matches.len()
                );
                for m in selected_matches {
                    println!("\n{}", self.render_match(m, file_path));
                }
            }
            1 => {
                let content = selected_matches
                    .iter()
                    .map(|m| {
                        format!(
                            "File: {}\nLine {}: {}\nKeyword: {}\nContext:\n{}\n",
                            file_path,
                            m.line_number,
                            m.line,
                            m.keyword,
                            m.context_before
                                .iter()
                                .chain(std::iter::once(&m.line))
                                .chain(m.context_after.iter())
                                .enumerate()
                                .map(|(i, line)| format!(
                                    "  {}: {}",
                                    (m.line_number - m.context_before.len() + i),
                                    line
                                ))
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n---\n");

                match Clipboard::new() {
                    Ok(mut clipboard) => {
                        if let Err(e) = clipboard.set_text(&content) {
                            println!(
                                "{} Failed to copy to clipboard: {}",
                                "Error:".bright_red(),
                                e
                            );
                        } else {
                            println!(
                                "{} {} matches copied to clipboard with full context!",
                                "Success:".bright_green(),
                                selected_matches.len()
                            );
                        }
                    }
                    Err(e) => println!(
                        "{} Failed to access clipboard: {}",
                        "Error:".bright_red(),
                        e
                    ),
                }
            }
            2 => {
                let default_filename = format!(
                    "keyword_matches_{}.txt",
                    chrono::Local::now().format("%Y%m%d_%H%M%S")
                );
                let theme = LlaDialoguerTheme::default();
                let input = dialoguer::Input::<String>::with_theme(&theme)
                    .with_prompt("Enter file path to save")
                    .with_initial_text(&default_filename)
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;

                let content = selected_matches
                    .iter()
                    .map(|m| {
                        format!(
                            "Match Details:\n  File: {}\n  Line: {}\n  Keyword: {}\n  Content: {}\n  Context:\n{}",
                            file_path,
                            m.line_number,
                            m.keyword,
                            m.line,
                            m.context_before
                                .iter()
                                .chain(std::iter::once(&m.line))
                                .chain(m.context_after.iter())
                                .enumerate()
                                .map(|(i, line)| format!(
                                    "    {}: {}",
                                    (m.line_number - m.context_before.len() + i),
                                    line
                                ))
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");

                fs::write(&input, content).map_err(|e| format!("Failed to write file: {}", e))?;
                println!(
                    "{} {} matches saved to {} with full context",
                    "Success:".bright_green(),
                    selected_matches.len(),
                    input.bright_blue()
                );
            }
            3 => {
                let total_matches = selected_matches.len();
                let unique_keywords: std::collections::HashSet<_> =
                    selected_matches.iter().map(|m| &m.keyword).collect();
                let avg_context_lines = selected_matches
                    .iter()
                    .map(|m| m.context_before.len() + m.context_after.len())
                    .sum::<usize>() as f64
                    / total_matches as f64;

                println!("\n{} Match Statistics:", "📊".bright_cyan());
                println!("─{}─", "─".repeat(50));
                println!(
                    " • Total matches: {}",
                    total_matches.to_string().bright_yellow()
                );
                println!(
                    " • Unique keywords: {}",
                    unique_keywords.len().to_string().bright_yellow()
                );
                println!(
                    " • Average context lines: {:.1}",
                    avg_context_lines.to_string().bright_yellow()
                );
                println!(" • File: {}", file_path.bright_blue());

                println!("\n{} Keyword Frequency:", "📈".bright_cyan());
                let mut keyword_freq: HashMap<&String, usize> = HashMap::new();
                for m in selected_matches.iter() {
                    *keyword_freq.entry(&m.keyword).or_insert(0) += 1;
                }
                for (keyword, count) in keyword_freq.iter() {
                    println!(
                        " • {}: {}",
                        keyword.bright_magenta(),
                        count.to_string().bright_yellow()
                    );
                }
                println!("─{}─\n", "─".repeat(50));
            }
            4 => {
                let keywords: Vec<_> = selected_matches
                    .iter()
                    .map(|m| &m.keyword)
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();

                let theme = LlaDialoguerTheme::default();
                let keyword_selection = MultiSelect::with_theme(&theme)
                    .with_prompt("Filter by keywords")
                    .items(&keywords)
                    .interact()
                    .map_err(|e| format!("Failed to show keyword selector: {}", e))?;

                if keyword_selection.is_empty() {
                    println!("{} No keywords selected", "Info:".bright_blue());
                    return Ok(());
                }

                let filtered_matches: Vec<_> = selected_matches
                    .into_iter()
                    .filter(|m| keyword_selection.iter().any(|&i| keywords[i] == &m.keyword))
                    .collect();

                println!(
                    "\n{} Showing {} filtered matches:",
                    "Info:".bright_blue(),
                    filtered_matches.len()
                );
                for m in filtered_matches {
                    println!("\n{}", self.render_match(m, file_path));
                }
            }
            5 => {
                println!("\n{} Advanced Analysis:", "📈".bright_cyan());
                println!("─{}─", "─".repeat(50));

                let mut line_dist: HashMap<usize, usize> = HashMap::new();
                for m in selected_matches.iter() {
                    let bucket = (m.line_number / 10) * 10;
                    *line_dist.entry(bucket).or_insert(0) += 1;
                }
                println!("\n{} Line Distribution:", "📊".bright_blue());
                for (bucket, count) in line_dist.iter().sorted_by_key(|k| k.0) {
                    println!(
                        " • Lines {}-{}: {}",
                        bucket,
                        bucket + 9,
                        "█".repeat(*count).bright_yellow()
                    );
                }

                println!("\n{} Keyword Patterns:", "🔍".bright_blue());
                let mut patterns: HashMap<(&String, &String), usize> = HashMap::new();
                for window in selected_matches.windows(2) {
                    if let [a, b] = window {
                        *patterns.entry((&a.keyword, &b.keyword)).or_insert(0) += 1;
                    }
                }
                for ((k1, k2), count) in patterns.iter().filter(|(_, &c)| c > 1) {
                    println!(
                        " • {} → {}: {} occurrences",
                        k1.bright_magenta(),
                        k2.bright_magenta(),
                        count.to_string().bright_yellow()
                    );
                }
                println!("─{}─\n", "─".repeat(50));
            }
            _ => unreachable!(),
        }

        Ok(())
    }
}

impl Plugin for KeywordSearchPlugin {
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
                        if let Some(matches) = entry
                            .path
                            .is_file()
                            .then(|| self.search_file(&entry.path))
                            .flatten()
                        {
                            entry.custom_fields.insert(
                                "keyword_matches".to_string(),
                                toml::to_string(&matches).unwrap_or_default(),
                            );
                        }
                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::FormatField(entry, _format) => {
                        let field = entry
                            .custom_fields
                            .get("keyword_matches")
                            .and_then(|toml_str| toml::from_str::<Vec<KeywordMatch>>(toml_str).ok())
                            .map(|matches| {
                                matches
                                    .iter()
                                    .map(|m| self.render_match(m, &entry.path.to_string_lossy()))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            });
                        PluginResponse::FormattedField(field)
                    }
                    PluginRequest::PerformAction(action, _args) => {
                        let response = match action.as_str() {
                            "search" => {
                                let result = (|| {
                                    let config = self.base.config();
                                    if config.keywords.is_empty() {
                                        let theme = LlaDialoguerTheme::default();
                                        let input = dialoguer::Input::<String>::with_theme(&theme)
                                            .with_prompt("Enter keywords (space-separated)")
                                            .interact_text()
                                            .map_err(|e| {
                                                format!("Failed to get keywords: {}", e)
                                            })?;

                                        let keywords: Vec<String> = input
                                            .split_whitespace()
                                            .map(|s| s.to_string())
                                            .collect();

                                        if keywords.is_empty() {
                                            return Err("No keywords provided".to_string());
                                        }

                                        self.base.config_mut().keywords = keywords;
                                    }

                                    let mut files: Vec<String> = Vec::new();
                                    for entry in std::fs::read_dir(".")
                                        .map_err(|e| format!("Failed to read directory: {}", e))?
                                    {
                                        let entry = entry
                                            .map_err(|e| format!("Failed to read entry: {}", e))?;
                                        let path = entry.path();
                                        if path.is_file() {
                                            if let Some(ext) = path.extension() {
                                                if self
                                                    .base
                                                    .config()
                                                    .file_extensions
                                                    .contains(&ext.to_string_lossy().to_string())
                                                {
                                                    files.push(path.to_string_lossy().to_string());
                                                }
                                            }
                                        }
                                    }

                                    if files.is_empty() {
                                        return Err(
                                            "No supported files found in current directory"
                                                .to_string(),
                                        );
                                    }

                                    let files =
                                        files.iter().map(|p| p.to_string()).collect::<Vec<_>>();
                                    let theme = LlaDialoguerTheme::default();
                                    let selection = MultiSelect::with_theme(&theme)
                                        .with_prompt("Select files to search")
                                        .items(&files)
                                        .interact()
                                        .map_err(|e| {
                                            format!("Failed to show file selector: {}", e)
                                        })?;

                                    if selection.is_empty() {
                                        return Err("No files selected".to_string());
                                    }

                                    let mut all_matches = Vec::new();
                                    for &idx in &selection {
                                        let path = std::path::Path::new(&files[idx]);
                                        if let Some(matches) = self.search_file(path) {
                                            all_matches.extend(matches);
                                        }
                                    }

                                    if all_matches.is_empty() {
                                        println!(
                                            "{} No matches found in selected files",
                                            "Info:".bright_blue()
                                        );
                                        Ok(())
                                    } else {
                                        self.interactive_search(all_matches, "Selected Files")
                                    }
                                })();
                                PluginResponse::ActionResult(result)
                            }
                            "help" => {
                                let result = {
                                    let mut help =
                                        HelpFormatter::new("Keyword Search Plugin".to_string());
                                    help.add_section("Description".to_string())
                                        .add_command(
                                            "".to_string(),
                                            "Search for keywords in files with interactive selection and actions.".to_string(),
                                            vec![],
                                        );

                                    help.add_section("Actions".to_string())
                                        .add_command(
                                            "search".to_string(),
                                            "Search for keywords in files".to_string(),
                                            vec!["search".to_string()],
                                        )
                                        .add_command(
                                            "help".to_string(),
                                            "Show this help information".to_string(),
                                            vec!["help".to_string()],
                                        );

                                    println!(
                                        "{}",
                                        BoxComponent::new(help.render(&self.base.config().colors))
                                            .style(BoxStyle::Minimal)
                                            .padding(1)
                                            .render()
                                    );
                                    Ok(())
                                };
                                PluginResponse::ActionResult(result)
                            }
                            _ => PluginResponse::ActionResult(Err(format!(
                                "Unknown action: {}",
                                action
                            ))),
                        };
                        response
                    }
                    PluginRequest::GetAvailableActions => {
                        use lla_plugin_interface::ActionInfo;
                        PluginResponse::AvailableActions(vec![
                            ActionInfo { name: "search".to_string(), usage: "search".to_string(), description: "Search for keywords in files".to_string(), examples: vec!["lla plugin keyword_search search".to_string()] },
                            ActionInfo { name: "help".to_string(), usage: "help".to_string(), description: "Show help information".to_string(), examples: vec!["lla plugin keyword_search help".to_string()] },
                        ])
                    }
                };
                self.encode_response(response)
            }
            Err(e) => self.encode_error(&e),
        }
    }
}

impl Default for KeywordSearchPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for KeywordSearchPlugin {
    type Config = SearchConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for KeywordSearchPlugin {}

lla_plugin_interface::declare_plugin!(KeywordSearchPlugin);
