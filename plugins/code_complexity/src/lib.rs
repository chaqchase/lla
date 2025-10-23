use lazy_static::lazy_static;
use lla_plugin_interface::{DecoratedEntry, Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::{
        components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List},
        TextBlock,
    },
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::PathBuf,
};

lazy_static! {
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "set-thresholds",
            "set-thresholds <low> <medium> <high> <very-high>",
            "Set complexity thresholds",
            vec!["lla plugin --name code_complexity --action set-thresholds --args 10 20 30 40"],
            |args| {
                if args.len() != 4 {
                    return Err(
                        "Usage: set-thresholds <low> <medium> <high> <very-high>".to_string()
                    );
                }
                if let (Ok(low), Ok(medium), Ok(high), Ok(very_high)) = (
                    args[0].parse::<f32>(),
                    args[1].parse::<f32>(),
                    args[2].parse::<f32>(),
                    args[3].parse::<f32>(),
                ) {
                    let mut state = PLUGIN_STATE.write();
                    state.config.thresholds = ComplexityThresholds {
                        low,
                        medium,
                        high,
                        very_high,
                    };
                    state.save_config();
                    println!(
                        "{}",
                        TextBlock::new("Updated complexity thresholds")
                            .color("bright_green")
                            .build()
                    );
                    Ok(())
                } else {
                    Err("Invalid threshold values".to_string())
                }
            }
        );

        lla_plugin_utils::define_action!(
            registry,
            "show-report",
            "show-report",
            "Show detailed complexity report",
            vec!["lla plugin --name code_complexity --action show-report"],
            |_| {
                let state = PLUGIN_STATE.read();
                println!("{}", state.generate_report());
                Ok(())
            }
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name code_complexity --action help"],
            |_| {
                let mut help = HelpFormatter::new("Code Complexity Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Analyzes code complexity using various metrics".to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string())
                    .add_command(
                        "set-thresholds".to_string(),
                        "Set complexity thresholds".to_string(),
                        vec![
                            "lla plugin --name code_complexity --action set-thresholds --args 10 20 30 40"
                                .to_string(),
                        ],
                    )
                    .add_command(
                        "show-report".to_string(),
                        "Show detailed complexity report".to_string(),
                        vec!["lla plugin --name code_complexity --action show-report".to_string()],
                    )
                    .add_command(
                        "help".to_string(),
                        "Show this help information".to_string(),
                        vec!["lla plugin --name code_complexity --action help".to_string()],
                    );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic complexity metrics".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show detailed complexity metrics and suggestions".to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&ComplexityConfig::default().colors))
                        .style(BoxStyle::Minimal)
                        .padding(2)
                        .render()
                );
                Ok(())
            }
        );

        registry
    });
    static ref PLUGIN_STATE: RwLock<PluginState> = RwLock::new(PluginState::new());
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LanguageRules {
    extensions: Vec<String>,
    function_patterns: Vec<String>,
    class_patterns: Vec<String>,
    branch_patterns: Vec<String>,
    loop_patterns: Vec<String>,
    comment_patterns: Vec<String>,
    max_line_length: usize,
    max_function_lines: usize,
}

impl Default for LanguageRules {
    fn default() -> Self {
        Self {
            extensions: vec!["rs".to_string()],
            function_patterns: vec!["fn ".to_string()],
            class_patterns: vec![
                "struct ".to_string(),
                "impl ".to_string(),
                "trait ".to_string(),
            ],
            branch_patterns: vec!["if ".to_string(), "match ".to_string(), "else".to_string()],
            loop_patterns: vec!["for ".to_string(), "while ".to_string(), "loop".to_string()],
            comment_patterns: vec!["//".to_string(), "/*".to_string()],
            max_line_length: 100,
            max_function_lines: 50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityConfig {
    #[serde(default = "default_languages")]
    languages: HashMap<String, LanguageRules>,
    #[serde(default = "default_thresholds")]
    thresholds: ComplexityThresholds,
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("low".to_string(), "bright_green".to_string());
    colors.insert("medium".to_string(), "bright_yellow".to_string());
    colors.insert("high".to_string(), "bright_red".to_string());
    colors.insert("very_high".to_string(), "red".to_string());
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

fn default_thresholds() -> ComplexityThresholds {
    ComplexityThresholds {
        low: 10.0,
        medium: 20.0,
        high: 30.0,
        very_high: 40.0,
    }
}

fn default_languages() -> HashMap<String, LanguageRules> {
    let mut languages = HashMap::new();
    languages.insert("Rust".to_string(), LanguageRules::default());
    languages
}

impl Default for ComplexityConfig {
    fn default() -> Self {
        Self {
            languages: default_languages(),
            thresholds: default_thresholds(),
            colors: default_colors(),
        }
    }
}

impl PluginConfig for ComplexityConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComplexityThresholds {
    low: f32,
    medium: f32,
    high: f32,
    very_high: f32,
}

impl Default for ComplexityThresholds {
    fn default() -> Self {
        Self {
            low: 10.0,
            medium: 20.0,
            high: 30.0,
            very_high: 40.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComplexityMetrics {
    lines: usize,
    functions: usize,
    classes: usize,
    branches: usize,
    loops: usize,
    comments: usize,
    long_lines: usize,
    long_functions: Vec<(String, usize)>,
    cyclomatic_complexity: usize,
    cognitive_complexity: usize,
    maintainability_index: f32,
}

impl Default for ComplexityMetrics {
    fn default() -> Self {
        Self {
            lines: 0,
            functions: 0,
            classes: 0,
            branches: 0,
            loops: 0,
            comments: 0,
            long_lines: 0,
            long_functions: Vec::new(),
            cyclomatic_complexity: 0,
            cognitive_complexity: 0,
            maintainability_index: 100.0,
        }
    }
}

struct PluginState {
    config: ComplexityConfig,
    config_path: PathBuf,
    stats: HashMap<String, Vec<(PathBuf, ComplexityMetrics)>>,
}

impl PluginState {
    fn new() -> Self {
        let config_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("lla")
            .join("code_complexity.toml");

        let config = Self::load_config(&config_path).unwrap_or_default();

        Self {
            config,
            config_path,
            stats: HashMap::new(),
        }
    }

    fn load_config(path: &PathBuf) -> Option<ComplexityConfig> {
        fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
    }

    fn save_config(&self) {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        if let Ok(content) = toml::to_string_pretty(&self.config) {
            fs::write(&self.config_path, content).ok();
        }
    }

    fn analyze_file(&self, path: &std::path::Path) -> Option<ComplexityMetrics> {
        let extension = path.extension()?.to_str()?;
        let language = self
            .config
            .languages
            .iter()
            .find(|(_, rules)| rules.extensions.iter().any(|ext| ext == extension))?;
        let rules = language.1;

        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);
        let mut metrics = ComplexityMetrics::default();
        let mut current_function = String::new();
        let mut current_function_lines = 0;
        let mut nesting_level = 0;

        for line in reader.lines().map_while(Result::ok) {
            metrics.lines += 1;
            let trimmed = line.trim();

            if trimmed.is_empty() {
                continue;
            }

            if line.len() > rules.max_line_length {
                metrics.long_lines += 1;
            }

            if rules
                .comment_patterns
                .iter()
                .any(|p| trimmed.starts_with(p))
            {
                metrics.comments += 1;
                continue;
            }

            if rules.function_patterns.iter().any(|p| trimmed.contains(p)) {
                if !current_function.is_empty() && current_function_lines > rules.max_function_lines
                {
                    metrics
                        .long_functions
                        .push((current_function.clone(), current_function_lines));
                }
                current_function = trimmed.to_string();
                current_function_lines = 0;
                metrics.functions += 1;
            }
            current_function_lines += 1;

            if rules.class_patterns.iter().any(|p| trimmed.contains(p)) {
                metrics.classes += 1;
            }

            if rules.branch_patterns.iter().any(|p| trimmed.contains(p)) {
                metrics.branches += 1;
                metrics.cyclomatic_complexity += 1;
                metrics.cognitive_complexity += nesting_level + 1;
            }

            if rules.loop_patterns.iter().any(|p| trimmed.contains(p)) {
                metrics.loops += 1;
                metrics.cyclomatic_complexity += 1;
                metrics.cognitive_complexity += nesting_level + 1;
            }

            if trimmed.ends_with('{') {
                nesting_level += 1;
            } else if trimmed.starts_with('}') {
                nesting_level = nesting_level.saturating_sub(1);
            }
        }

        let volume = (metrics.lines as f32 * (metrics.functions + metrics.classes) as f32).ln();
        let cyclomatic = metrics.cyclomatic_complexity as f32;
        let lines = metrics.lines as f32;
        metrics.maintainability_index =
            (171.0 - 5.2 * volume - 0.23 * cyclomatic - 16.2 * lines.ln()).max(0.0) * 100.0 / 171.0;

        Some(metrics)
    }

    fn get_complexity_color(&self, metrics: &ComplexityMetrics) -> String {
        let score = metrics.cyclomatic_complexity as f32 * 0.4
            + metrics.cognitive_complexity as f32 * 0.3
            + (100.0 - metrics.maintainability_index) * 0.3;

        if score < self.config.thresholds.low {
            self.config
                .colors
                .get("low")
                .unwrap_or(&"bright_green".to_string())
                .clone()
        } else if score < self.config.thresholds.medium {
            self.config
                .colors
                .get("medium")
                .unwrap_or(&"bright_yellow".to_string())
                .clone()
        } else if score < self.config.thresholds.high {
            self.config
                .colors
                .get("high")
                .unwrap_or(&"bright_red".to_string())
                .clone()
        } else {
            self.config
                .colors
                .get("very_high")
                .unwrap_or(&"red".to_string())
                .clone()
        }
    }

    fn format_metrics(&self, metrics: &ComplexityMetrics, detailed: bool) -> String {
        let color = self.get_complexity_color(metrics);
        let mut list = List::new();

        list.add_item(
            KeyValue::new(
                "Complexity",
                &format!(
                    "{} (MI: {:.1})",
                    metrics.cyclomatic_complexity, metrics.maintainability_index
                ),
            )
            .key_color(color)
            .key_width(15)
            .render(),
        );

        if detailed {
            list.add_item(
                KeyValue::new("Lines", metrics.lines.to_string())
                    .key_width(15)
                    .render(),
            );
            list.add_item(
                KeyValue::new("Functions", metrics.functions.to_string())
                    .key_width(15)
                    .render(),
            );
            list.add_item(
                KeyValue::new("Classes", metrics.classes.to_string())
                    .key_width(15)
                    .render(),
            );
            list.add_item(
                KeyValue::new("Branches", metrics.branches.to_string())
                    .key_width(15)
                    .render(),
            );
            list.add_item(
                KeyValue::new("Loops", metrics.loops.to_string())
                    .key_width(15)
                    .render(),
            );
            list.add_item(
                KeyValue::new("Comments", metrics.comments.to_string())
                    .key_width(15)
                    .render(),
            );
            list.add_item(
                KeyValue::new("Long lines", metrics.long_lines.to_string())
                    .key_width(15)
                    .render(),
            );

            if !metrics.long_functions.is_empty() {
                list.add_item("Long functions:".to_string());
                for (name, lines) in &metrics.long_functions {
                    list.add_item(format!("  {} ({} lines)", name, lines));
                }
            }

            if metrics.maintainability_index < 65.0 {
                list.add_item("\nSuggestions:".to_string());
                if metrics.long_functions.len() > 2 {
                    list.add_item("  - Consider breaking down long functions".to_string());
                }
                if metrics.comments < metrics.lines / 10 {
                    list.add_item("  - Add more documentation".to_string());
                }
                if metrics.cyclomatic_complexity > 10 {
                    list.add_item("  - Reduce nested conditionals".to_string());
                }
                if metrics.cognitive_complexity > 15 {
                    list.add_item("  - Simplify complex logic".to_string());
                }
            }
        }

        BoxComponent::new(list.render())
            .style(BoxStyle::Minimal)
            .padding(1)
            .render()
    }

    fn generate_report(&self) -> String {
        let mut list = List::new();
        list.add_item(
            TextBlock::new("Code Complexity Report")
                .color("bright_blue")
                .build(),
        );

        for (language, files) in &self.stats {
            list.add_item(TextBlock::new(language).color("bright_cyan").build());

            let mut total_metrics = ComplexityMetrics::default();
            let mut file_count = 0;

            for (path, metrics) in files {
                list.add_item(
                    KeyValue::new(
                        &format!("  {}", path.display()),
                        &format!(
                            "{} (MI: {:.1})",
                            metrics.cyclomatic_complexity, metrics.maintainability_index
                        ),
                    )
                    .key_color(self.get_complexity_color(&metrics))
                    .render(),
                );

                total_metrics.lines += metrics.lines;
                total_metrics.functions += metrics.functions;
                total_metrics.classes += metrics.classes;
                total_metrics.branches += metrics.branches;
                total_metrics.loops += metrics.loops;
                total_metrics.comments += metrics.comments;
                total_metrics.cyclomatic_complexity += metrics.cyclomatic_complexity;
                total_metrics.cognitive_complexity += metrics.cognitive_complexity;
                file_count += 1;
            }

            if file_count > 0 {
                list.add_item("\nAverage metrics:".to_string());
                list.add_item(format!(
                    "  Lines per file: {:.1}",
                    total_metrics.lines as f32 / file_count as f32
                ));
                list.add_item(format!(
                    "  Cyclomatic complexity: {:.1}",
                    total_metrics.cyclomatic_complexity as f32 / file_count as f32
                ));
                list.add_item(format!(
                    "  Maintainability index: {:.1}\n",
                    total_metrics.maintainability_index / file_count as f32
                ));
            }
        }

        BoxComponent::new(list.render())
            .style(BoxStyle::Minimal)
            .padding(1)
            .render()
    }
}

pub struct CodeComplexityEstimatorPlugin {
    base: BasePlugin<ComplexityConfig>,
}

impl CodeComplexityEstimatorPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!(
                "[CodeComplexityEstimatorPlugin] Failed to save config: {}",
                e
            );
        }
        plugin
    }

    fn format_file_info(&self, entry: &DecoratedEntry, format: &str) -> Option<String> {
        entry
            .custom_fields
            .get("complexity_metrics")
            .and_then(|toml_str| toml::from_str::<ComplexityMetrics>(toml_str).ok())
            .map(|metrics| {
                PLUGIN_STATE
                    .read()
                    .format_metrics(&metrics, format == "long")
            })
    }
}

impl Plugin for CodeComplexityEstimatorPlugin {
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
                        if entry.path.is_file() {
                            let metrics = PLUGIN_STATE.read().analyze_file(&entry.path);
                            if let Some(metrics) = metrics {
                                entry.custom_fields.insert(
                                    "complexity_metrics".to_string(),
                                    toml::to_string(&metrics).unwrap_or_default(),
                                );

                                if let Some(ext) = entry.path.extension().and_then(|e| e.to_str()) {
                                    let lang = {
                                        let state = PLUGIN_STATE.read();
                                        state
                                            .config
                                            .languages
                                            .iter()
                                            .find(|(_, rules)| {
                                                rules.extensions.iter().any(|e| e == ext)
                                            })
                                            .map(|(lang, _)| lang.clone())
                                    };

                                    if let Some(lang) = lang {
                                        PLUGIN_STATE
                                            .write()
                                            .stats
                                            .entry(lang)
                                            .or_default()
                                            .push((entry.path.clone(), metrics));
                                    }
                                }
                            }
                        }
                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::FormatField(entry, format) => {
                        let field = self.format_file_info(&entry, &format);
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

impl Default for CodeComplexityEstimatorPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for CodeComplexityEstimatorPlugin {
    type Config = ComplexityConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for CodeComplexityEstimatorPlugin {}

lla_plugin_interface::declare_plugin!(CodeComplexityEstimatorPlugin);
