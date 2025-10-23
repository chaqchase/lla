use crate::commands::args::ConfigAction;
use crate::error::{ConfigErrorKind, LlaError, Result};
use crate::theme::{load_theme, Theme};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TreeFormatterConfig {
    #[serde(default)]
    pub max_lines: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GridFormatterConfig {
    #[serde(default)]
    pub ignore_width: bool,
    #[serde(default = "default_grid_max_width")]
    pub max_width: usize,
}

impl Default for GridFormatterConfig {
    fn default() -> Self {
        Self {
            ignore_width: false,
            max_width: default_grid_max_width(),
        }
    }
}

fn default_grid_max_width() -> usize {
    200
}

impl Default for TreeFormatterConfig {
    fn default() -> Self {
        Self {
            max_lines: Some(20_000),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SizeMapConfig {}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FormatterConfig {
    #[serde(default)]
    pub tree: TreeFormatterConfig,
    #[serde(default)]
    pub grid: GridFormatterConfig,
    #[serde(default)]
    pub long: LongFormatterConfig,
    #[serde(default)]
    pub sizemap: SizeMapConfig,
}

impl Default for FormatterConfig {
    fn default() -> Self {
        Self {
            tree: TreeFormatterConfig::default(),
            grid: GridFormatterConfig::default(),
            long: LongFormatterConfig::default(),
            sizemap: SizeMapConfig::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LongFormatterConfig {
    #[serde(default)]
    pub hide_group: bool,
    #[serde(default)]
    pub relative_dates: bool,
}

impl Default for LongFormatterConfig {
    fn default() -> Self {
        Self {
            hide_group: false,
            relative_dates: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecursiveConfig {
    #[serde(default)]
    pub max_entries: Option<usize>,
}

impl Default for RecursiveConfig {
    fn default() -> Self {
        Self {
            max_entries: Some(20_000),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FuzzyConfig {
    #[serde(default = "default_ignore_patterns")]
    pub ignore_patterns: Vec<String>,
}

impl Default for FuzzyConfig {
    fn default() -> Self {
        Self {
            ignore_patterns: default_ignore_patterns(),
        }
    }
}

fn default_ignore_patterns() -> Vec<String> {
    vec![
        String::from("node_modules"),
        String::from("target"),
        String::from(".git"),
        String::from(".idea"),
        String::from(".vscode"),
    ]
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ListerConfig {
    #[serde(default)]
    pub recursive: RecursiveConfig,
    #[serde(default)]
    pub fuzzy: FuzzyConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SortConfig {
    #[serde(default)]
    pub dirs_first: bool,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub natural: bool,
}

impl Default for SortConfig {
    fn default() -> Self {
        Self {
            dirs_first: false,
            case_sensitive: false,
            natural: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct FilterConfig {
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub no_dotfiles: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub default_sort: String,
    pub default_format: String,
    pub enabled_plugins: Vec<String>,
    #[serde(deserialize_with = "deserialize_path_with_tilde")]
    pub plugins_dir: PathBuf,
    #[serde(default, deserialize_with = "deserialize_paths_with_tilde")]
    pub exclude_paths: Vec<PathBuf>,
    pub default_depth: Option<usize>,
    #[serde(default)]
    pub show_icons: bool,
    #[serde(default)]
    pub include_dirs: bool,
    #[serde(default)]
    pub sort: SortConfig,
    #[serde(default)]
    pub filter: FilterConfig,
    #[serde(default)]
    pub formatters: FormatterConfig,
    #[serde(default)]
    pub listers: ListerConfig,
    #[serde(default)]
    pub shortcuts: HashMap<String, ShortcutCommand>,
    #[serde(default)]
    pub plugin_aliases: HashMap<String, String>,
    #[serde(default = "default_theme_name")]
    pub theme: String,
    #[serde(default = "default_permission_format")]
    pub permission_format: String,
}

fn deserialize_path_with_tilde<'de, D>(deserializer: D) -> std::result::Result<PathBuf, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let path_str = String::deserialize(deserializer)?;
    if path_str.starts_with('~') {
        let home = dirs::home_dir()
            .ok_or_else(|| serde::de::Error::custom("Could not determine home directory"))?;
        Ok(home.join(&path_str[2..]))
    } else {
        Ok(PathBuf::from(path_str))
    }
}

fn deserialize_paths_with_tilde<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let list = Vec::<String>::deserialize(deserializer)?;
    let mut result = Vec::with_capacity(list.len());
    for s in list {
        if s.starts_with('~') {
            let home = dirs::home_dir()
                .ok_or_else(|| serde::de::Error::custom("Could not determine home directory"))?;
            result.push(home.join(&s[2..]));
        } else {
            result.push(PathBuf::from(s));
        }
    }
    Ok(result)
}

fn default_theme_name() -> String {
    "default".to_string()
}

fn default_permission_format() -> String {
    "symbolic".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutCommand {
    pub plugin_name: String,
    pub action: String,
    pub description: Option<String>,
}

impl Config {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Config::default()
    }

    pub fn load(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        if path.exists() {
            let contents = fs::read_to_string(path)?;
            let config: Config = toml::from_str(&contents)?;
            config.validate()?;
            Ok(config)
        } else {
            let config = Config::default();
            config.ensure_plugins_dir()?;
            config.save(path)?;
            Ok(config)
        }
    }

    fn generate_config_content(&self) -> String {
        let plugins_dir_str = self.plugins_dir.to_string_lossy();
        let plugins_dir_display = if let Some(home) = dirs::home_dir() {
            if let Ok(relative) = self.plugins_dir.strip_prefix(&home) {
                format!("~/{}", relative.to_string_lossy())
            } else {
                plugins_dir_str.to_string()
            }
        } else {
            plugins_dir_str.to_string()
        };

        let mut content = format!(
            r#"# lla Configuration File
# This file controls the behavior and appearance of the lla command

# Default sorting method for file listings
# Possible values:
#   - "name": Sort alphabetically by filename (default)
#   - "size": Sort by file size, largest first
#   - "date": Sort by modification time, newest first
default_sort = "{}"

# Default format for displaying files
# Possible values:
#   - "default": Quick and clean directory listing
#   - "long": Detailed file information with metadata
#   - "tree": Hierarchical directory visualization
#   - "fuzzy": Interactive fuzzy search
#   - "grid": Organized grid layout for better readability
#   - "git": Git-aware view with repository status
#   - "timeline": Group files by time periods
#   - "sizemap": Visual representation of file sizes
#   - "table": Structured data display
default_format = "{}"

# Whether to show icons by default
# When true, file and directory icons will be displayed in all views
# Default: false
show_icons = {}

# Whether to include directory sizes in file listings
# When true, directory sizes will be calculated recursively
# This may impact performance for large directories
# Default: false
include_dirs = {}

# Format for displaying file permissions    
# Possible values:
#   - "symbolic": Traditional Unix-style (e.g., -rw-r--r--)
#   - "octal": Numeric mode (e.g., d644)
#   - "binary": Binary representation (e.g., 110100100)
#   - "compact": Compact representation (e.g., 644)
#   - "verbose": Verbose representation (e.g., type:file owner:rwx group:r-x others:r-x)
# Default: "symbolic"
permission_format = "{}"

# The theme to use for coloring
# Place custom themes in ~/.config/lla/themes/
# Default: "default"
theme = "{}"

# List of enabled plugins
# Each plugin provides additional functionality
# Examples:
#   - "git_status": Show Git repository information
#   - "file_hash": Calculate and display file hashes
#   - "file_tagger": Add and manage file tags
enabled_plugins = {}

# Directory where plugins are stored
# Default: ~/.config/lla/plugins
plugins_dir = "{}"

# Paths to exclude from listings (tilde is supported)
# Examples:
#   - "~/Library/Mobile Documents"  # macOS iCloud Drive (Mobile Documents)
#   - "~/Library/CloudStorage"      # macOS cloud storage providers
# Default: [] (no exclusions)
exclude_paths = {}

# Maximum depth for recursive directory traversal
# Controls how deep lla will go when showing directory contents
# Set to None for unlimited depth (may impact performance)
# Default: 3 levels deep
default_depth = {}

# Sorting configuration
[sort]
# List directories before files
# Default: false
dirs_first = {}

# Enable case-sensitive sorting
# Default: false
case_sensitive = {}

# Use natural sorting for numbers (e.g., 2.txt before 10.txt)
# Default: true
natural = {}

# Filtering configuration
[filter]
# Enable case-sensitive filtering by default
# Default: false
case_sensitive = {}

# Hide dot files and directories by default
# Default: false
no_dotfiles = {}

# Formatter-specific configurations
[formatters.tree]
# Maximum number of entries to display in tree view
# Controls memory usage and performance for large directories
# Set to 0 to show all entries (may impact performance)
# Default: 20000 entries
max_lines = {}

# Grid formatter configuration
[formatters.grid]
# Whether to ignore terminal width by default
# When true, grid view will use max_width instead of terminal width
# Default: false
ignore_width = {}

# Maximum width for grid view when ignore_width is true
# This value is used when terminal width is ignored
# Default: 200 columns
max_width = {}

# Long formatter configuration
[formatters.long]
# Hide the group column in long format
# Default: false
hide_group = {}

# Show relative dates (e.g., "2h ago") in long format
# Default: false
relative_dates = {}

# Lister-specific configurations
[listers.recursive]
# Maximum number of entries to process in recursive listing
# Controls memory usage and performance for deep directory structures
# Set to 0 to process all entries (may impact performance)
# Default: 20000 entries
max_entries = {}

# Fuzzy lister configuration
[listers.fuzzy]
# Patterns to ignore when listing files in fuzzy mode
# Can be:
#  - Simple substring match: "node_modules"
#  - Glob pattern: "glob:*.min.js"
#  - Regular expression: "regex:.*\\.pyc$"
# Default: ["node_modules", "target", ".git", ".idea", ".vscode"]
ignore_patterns = {}"#,
            self.default_sort,
            self.default_format,
            self.show_icons,
            self.include_dirs,
            self.permission_format,
            self.theme,
            serde_json::to_string(&self.enabled_plugins).unwrap(),
            plugins_dir_display,
            {
                let home = dirs::home_dir();
                let display_paths: Vec<String> = self
                    .exclude_paths
                    .iter()
                    .map(|p| {
                        if let Some(home) = &home {
                            if let Ok(relative) = p.strip_prefix(home) {
                                format!("~/{}", relative.to_string_lossy())
                            } else {
                                p.to_string_lossy().to_string()
                            }
                        } else {
                            p.to_string_lossy().to_string()
                        }
                    })
                    .collect();
                serde_json::to_string(&display_paths).unwrap()
            },
            match self.default_depth {
                Some(depth) => depth.to_string(),
                None => "null".to_string(),
            },
            self.sort.dirs_first,
            self.sort.case_sensitive,
            self.sort.natural,
            self.filter.case_sensitive,
            self.filter.no_dotfiles,
            self.formatters.tree.max_lines.unwrap_or(0),
            self.formatters.grid.ignore_width,
            self.formatters.grid.max_width,
            self.formatters.long.hide_group,
            self.formatters.long.relative_dates,
            self.listers.recursive.max_entries.unwrap_or(0),
            serde_json::to_string(&self.listers.fuzzy.ignore_patterns).unwrap(),
        );

        if !self.shortcuts.is_empty() {
            content.push_str("\n\n# Command shortcuts\n");
            content.push_str("# Define custom shortcuts for frequently used plugin commands\n");
            content.push_str("[shortcuts]\n");
            for (name, cmd) in &self.shortcuts {
                content.push_str(&format!(
                    r#"{}={{ plugin_name = "{}", action = "{}""#,
                    name, cmd.plugin_name, cmd.action
                ));
                if let Some(desc) = &cmd.description {
                    content.push_str(&format!(r#", description = "{}""#, desc));
                }
                content.push_str("}\n");
            }
        }

        content
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        self.ensure_plugins_dir()?;
        fs::write(path, self.generate_config_content())?;
        Ok(())
    }

    pub fn get_config_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".config").join("lla").join("config.toml")
    }

    pub fn ensure_plugins_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.plugins_dir).map_err(|e| {
            LlaError::Config(ConfigErrorKind::InvalidPath(format!(
                "Failed to create plugins directory: {}",
                e
            )))
        })
    }

    pub fn enable_plugin(&mut self, plugin_name: &str) -> Result<()> {
        self.ensure_plugins_dir().map_err(|e| {
            LlaError::Config(ConfigErrorKind::InvalidPath(format!(
                "Failed to create plugins directory: {}",
                e
            )))
        })?;
        if !self.enabled_plugins.contains(&plugin_name.to_string()) {
            self.enabled_plugins.push(plugin_name.to_string());
            self.save(&Self::get_config_path())?;
        }
        Ok(())
    }

    pub fn disable_plugin(&mut self, plugin_name: &str) -> Result<()> {
        self.enabled_plugins.retain(|name| name != plugin_name);
        self.save(&Self::get_config_path())?;
        Ok(())
    }

    pub fn add_shortcut(&mut self, name: String, command: ShortcutCommand) -> Result<()> {
        if name.is_empty() {
            return Err(LlaError::Config(ConfigErrorKind::ValidationError(
                "Shortcut name cannot be empty".to_string(),
            )));
        }
        if command.plugin_name.is_empty() {
            return Err(LlaError::Config(ConfigErrorKind::ValidationError(
                "Plugin name cannot be empty".to_string(),
            )));
        }
        if command.action.is_empty() {
            return Err(LlaError::Config(ConfigErrorKind::ValidationError(
                "Action cannot be empty".to_string(),
            )));
        }

        self.shortcuts.insert(name, command);
        self.save(&Self::get_config_path())?;
        Ok(())
    }

    pub fn remove_shortcut(&mut self, name: &str) -> Result<()> {
        self.shortcuts.remove(name);
        self.save(&Self::get_config_path())?;
        Ok(())
    }

    pub fn get_shortcut(&self, name: &str) -> Option<&ShortcutCommand> {
        self.shortcuts.get(name)
    }

    pub fn resolve_plugin_alias(&self, name: &str) -> String {
        self.plugin_aliases
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    pub fn validate(&self) -> Result<()> {
        if !["name", "size", "date"].contains(&self.default_sort.as_str()) {
            return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                "default_sort".to_string(),
                format!(
                    "Invalid sort value: {}. Must be one of: name, size, date",
                    self.default_sort
                ),
            )));
        }

        let valid_formats = [
            "default", "long", "tree", "grid", "git", "timeline", "sizemap", "table", "fuzzy",
        ];
        if !valid_formats.contains(&self.default_format.as_str()) {
            return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                "default_format".to_string(),
                format!(
                    "Invalid format value: {}. Must be one of: {}",
                    self.default_format,
                    valid_formats.join(", ")
                ),
            )));
        }

        if !self.plugins_dir.exists() {
            return Err(LlaError::Config(ConfigErrorKind::InvalidPath(format!(
                "Plugins directory does not exist: {}",
                self.plugins_dir.display()
            ))));
        }

        // Validate exclude_paths entries are non-empty
        for p in &self.exclude_paths {
            if p.as_os_str().is_empty() {
                return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                    "exclude_paths".to_string(),
                    "exclude_paths cannot contain empty paths".to_string(),
                )));
            }
        }

        if let Some(depth) = self.default_depth {
            if depth == 0 {
                return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                    "default_depth".to_string(),
                    "Depth must be greater than 0 or None".to_string(),
                )));
            }
        }

        if let Some(max_lines) = self.formatters.tree.max_lines {
            if max_lines > 100_000 {
                return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                    "formatters.tree.max_lines".to_string(),
                    "Tree formatter max lines should not exceed 100,000".to_string(),
                )));
            }
        }

        if let Some(max_entries) = self.listers.recursive.max_entries {
            if max_entries > 100_000 {
                return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                    "listers.recursive.max_entries".to_string(),
                    "Recursive lister max entries should not exceed 100,000".to_string(),
                )));
            }
        }

        for plugin in &self.enabled_plugins {
            let possible_names = [
                format!("lib{}.dylib", plugin),
                format!("lib{}.so", plugin),
                format!("{}.dll", plugin),
                format!("{}.dylib", plugin),
                format!("{}.so", plugin),
                plugin.clone(),
            ];

            let exists = possible_names
                .iter()
                .any(|name| self.plugins_dir.join(name).exists());

            if !exists {
                return Err(LlaError::Config(ConfigErrorKind::ValidationError(format!(
                    "Enabled plugin not found: {}",
                    plugin
                ))));
            }
        }

        for (name, cmd) in &self.shortcuts {
            if name.is_empty() {
                return Err(LlaError::Config(ConfigErrorKind::ValidationError(
                    "Shortcut name cannot be empty".to_string(),
                )));
            }
            if cmd.plugin_name.is_empty() {
                return Err(LlaError::Config(ConfigErrorKind::ValidationError(format!(
                    "Plugin name cannot be empty for shortcut: {}",
                    name
                ))));
            }
            if cmd.action.is_empty() {
                return Err(LlaError::Config(ConfigErrorKind::ValidationError(format!(
                    "Action cannot be empty for shortcut: {}",
                    name
                ))));
            }
        }

        Ok(())
    }

    pub fn set_value(&mut self, key: &str, value: &str) -> Result<()> {
        match key.split('.').collect::<Vec<_>>().as_slice() {
            ["plugins_dir"] => {
                let new_dir = PathBuf::from(value);
                fs::create_dir_all(&new_dir).map_err(|_| {
                    LlaError::Config(ConfigErrorKind::InvalidPath(format!(
                        "Failed to create directory: {}",
                        new_dir.display()
                    )))
                })?;
                self.plugins_dir = new_dir;
            }
            ["exclude_paths"] => {
                // Accept JSON array (e.g., ["~/foo","/bar"]) or a single path string
                let paths: Vec<String> = if value.trim_start().starts_with('[') {
                    serde_json::from_str(value).map_err(|_| {
                        LlaError::Config(ConfigErrorKind::InvalidValue(
                            key.to_string(),
                            "must be a JSON array of strings or a single path".to_string(),
                        ))
                    })?
                } else {
                    vec![value.to_string()]
                };

                let mut resolved = Vec::with_capacity(paths.len());
                for s in paths {
                    if s.starts_with('~') {
                        let home = dirs::home_dir().ok_or_else(|| {
                            LlaError::Config(ConfigErrorKind::InvalidPath(
                                "Could not determine home directory".to_string(),
                            ))
                        })?;
                        resolved.push(home.join(&s[2..]));
                    } else {
                        resolved.push(PathBuf::from(s));
                    }
                }

                self.exclude_paths = resolved;
            }
            ["default_sort"] => {
                if !["name", "size", "date"].contains(&value) {
                    return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be one of: name, size, date".to_string(),
                    )));
                }
                self.default_sort = value.to_string();
            }
            ["default_format"] => {
                let valid_formats = [
                    "default", "long", "tree", "grid", "git", "timeline", "sizemap", "table",
                    "fuzzy",
                ];
                if !valid_formats.contains(&value) {
                    return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        format!("must be one of: {}", valid_formats.join(", ")),
                    )));
                }
                self.default_format = value.to_string();
            }
            ["show_icons"] => {
                self.show_icons = value.parse().map_err(|_| {
                    LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be true or false".to_string(),
                    ))
                })?;
            }
            ["include_dirs"] => {
                self.include_dirs = value.parse().map_err(|_| {
                    LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be true or false".to_string(),
                    ))
                })?;
            }
            ["default_depth"] => {
                if value.to_lowercase() == "null" {
                    self.default_depth = None;
                } else {
                    let depth = value.parse().map_err(|_| {
                        LlaError::Config(ConfigErrorKind::InvalidValue(
                            key.to_string(),
                            "must be a positive number or null".to_string(),
                        ))
                    })?;
                    if depth == 0 {
                        return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                            key.to_string(),
                            "must be greater than 0 or null".to_string(),
                        )));
                    }
                    self.default_depth = Some(depth);
                }
            }
            ["sort", "dirs_first"] => {
                self.sort.dirs_first = value.parse().map_err(|_| {
                    LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be true or false".to_string(),
                    ))
                })?;
            }
            ["sort", "case_sensitive"] => {
                self.sort.case_sensitive = value.parse().map_err(|_| {
                    LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be true or false".to_string(),
                    ))
                })?;
            }
            ["sort", "natural"] => {
                self.sort.natural = value.parse().map_err(|_| {
                    LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be true or false".to_string(),
                    ))
                })?;
            }
            ["filter", "case_sensitive"] => {
                self.filter.case_sensitive = value.parse().map_err(|_| {
                    LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be true or false".to_string(),
                    ))
                })?;
            }
            ["filter", "no_dotfiles"] => {
                self.filter.no_dotfiles = value.parse().map_err(|_| {
                    LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be true or false".to_string(),
                    ))
                })?;
            }
            ["formatters", "tree", "max_lines"] => {
                let max_lines = value.parse().map_err(|_| {
                    LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be a number".to_string(),
                    ))
                })?;
                if max_lines > 100_000 {
                    return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "should not exceed 100,000".to_string(),
                    )));
                }
                self.formatters.tree.max_lines = Some(max_lines);
            }

            ["listers", "recursive", "max_entries"] => {
                let max_entries = value.parse().map_err(|_| {
                    LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be a number".to_string(),
                    ))
                })?;
                if max_entries > 100_000 {
                    return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "should not exceed 100,000".to_string(),
                    )));
                }
                self.listers.recursive.max_entries = Some(max_entries);
            }
            ["theme"] => {
                if let Ok(themes) = crate::theme::list_themes() {
                    if !themes.contains(&value.to_string()) {
                        return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                            key.to_string(),
                            format!(
                                "Theme '{}' not found. Available themes: {}",
                                value,
                                themes.join(", ")
                            ),
                        )));
                    }
                }
                self.theme = value.to_string();
            }
            ["permission_format"] => {
                if value != "symbolic"
                    && value != "octal"
                    && value != "binary"
                    && value != "verbose"
                    && value != "compact"
                {
                    return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                        key.to_string(),
                        "must be one of: symbolic, octal, binary, numeric, verbose, compact"
                            .to_string(),
                    )));
                }
                self.permission_format = value.to_string();
            }
            _ => {
                return Err(LlaError::Config(ConfigErrorKind::InvalidValue(
                    key.to_string(),
                    format!("unknown configuration key: {}", key),
                )));
            }
        }
        self.save(&Self::get_config_path())?;
        Ok(())
    }

    pub fn get_theme(&self) -> Theme {
        load_theme(&self.theme).unwrap_or_default()
    }
}

impl Default for Config {
    fn default() -> Self {
        let default_plugins_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("lla")
            .join("plugins");

        Config {
            default_sort: String::from("name"),
            default_format: String::from("default"),
            enabled_plugins: vec![],
            plugins_dir: default_plugins_dir,
            exclude_paths: Vec::new(),
            default_depth: Some(3),
            show_icons: false,
            include_dirs: false,
            sort: SortConfig::default(),
            filter: FilterConfig::default(),
            formatters: FormatterConfig::default(),
            listers: ListerConfig::default(),
            shortcuts: HashMap::new(),
            plugin_aliases: HashMap::new(),
            theme: default_theme_name(),
            permission_format: default_permission_format(),
        }
    }
}

pub fn initialize_config() -> Result<()> {
    let config_path = Config::get_config_path();
    let config_dir = config_path.parent().unwrap();
    let themes_dir = config_dir.join("themes");

    fs::create_dir_all(config_dir)?;
    fs::create_dir_all(&themes_dir)?;
    let default_theme_path = themes_dir.join("default.toml");
    if !default_theme_path.exists() {
        let default_theme_content = include_str!("default.toml");
        fs::write(&default_theme_path, default_theme_content)?;
        println!("Created default theme at {:?}", default_theme_path);
    }

    if config_path.exists() {
        println!("Config file already exists at {:?}", config_path);
        println!("Use `lla config` to view or modify the configuration.");
        return Ok(());
    }

    let config = Config::default();
    config.ensure_plugins_dir()?;
    fs::write(&config_path, config.generate_config_content())?;
    println!("Created default configuration at {:?}", config_path);
    Ok(())
}

pub fn handle_config_command(action: Option<ConfigAction>) -> Result<()> {
    let config_path = Config::get_config_path();
    match action {
        Some(ConfigAction::View) => view_config(),
        Some(ConfigAction::Set(key, value)) => {
            let mut config = Config::load(&config_path)?;
            config.set_value(&key, &value)?;
            println!("Updated {} = {}", key, value);
            Ok(())
        }
        None => view_config(),
    }
}

pub fn view_config() -> Result<()> {
    let config_path = Config::get_config_path();
    let config = Config::load(&config_path)?;
    println!("Current configuration at {:?}:", config_path);
    println!("{:#?}", config);
    Ok(())
}
