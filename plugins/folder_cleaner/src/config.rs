use lla_plugin_utils::config::PluginConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanConfig {
    #[serde(default = "default_true")]
    pub recursive: bool,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default)]
    pub follow_symlinks: bool,
    #[serde(default = "default_true")]
    pub same_filesystem: bool,
    #[serde(default = "default_ignore_patterns")]
    pub ignore_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    #[serde(default = "default_quarantine_dir")]
    pub quarantine_dir: String,
    #[serde(default = "default_true")]
    pub require_confirmation: bool,
    #[serde(default)]
    pub allow_permanent_delete: bool,
    #[serde(default = "default_collision_policy")]
    pub collision_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupConfig {
    #[serde(default = "default_cleanup_level")]
    pub level: String,
    #[serde(default = "default_true")]
    pub duplicate_detection: bool,
    #[serde(default = "default_true")]
    pub empty_dirs: bool,
    #[serde(default = "default_true")]
    pub temp_files: bool,
    #[serde(default = "default_true")]
    pub os_junk: bool,
    #[serde(default = "default_true")]
    pub old_archives: bool,
    #[serde(default = "default_duplicate_max_bytes")]
    pub duplicate_max_bytes: u64,
    #[serde(default = "default_old_archive_days")]
    pub old_archive_days: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    #[serde(default = "default_true")]
    pub organize: bool,
    #[serde(default = "default_true")]
    pub cleanup: bool,
    #[serde(default)]
    pub recursive: Option<bool>,
    #[serde(default)]
    pub max_depth: Option<usize>,
    #[serde(default)]
    pub include_hidden: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConfig {
    pub category: String,
    pub destination: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub filename_patterns: Vec<String>,
    #[serde(default)]
    pub path_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderCleanerConfig {
    #[serde(default = "default_colors")]
    pub colors: HashMap<String, String>,
    #[serde(default)]
    pub scan: ScanConfig,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub cleanup: CleanupConfig,
    #[serde(default = "default_profiles")]
    pub profiles: HashMap<String, ProfileConfig>,
    #[serde(default = "default_rules")]
    pub rules: Vec<RuleConfig>,
}

impl FolderCleanerConfig {
    pub fn profile(&self, name: Option<&str>) -> ProfileConfig {
        name.and_then(|profile| self.profiles.get(profile).cloned())
            .or_else(|| self.profiles.get("downloads").cloned())
            .unwrap_or_default()
    }
}

impl Default for FolderCleanerConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
            scan: ScanConfig::default(),
            safety: SafetyConfig::default(),
            cleanup: CleanupConfig::default(),
            profiles: default_profiles(),
            rules: default_rules(),
        }
    }
}

impl PluginConfig for FolderCleanerConfig {
    fn validate(&self) -> Result<(), String> {
        if self.safety.collision_policy != "rename" {
            return Err("only collision_policy = \"rename\" is currently supported".into());
        }

        Ok(())
    }
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            recursive: true,
            max_depth: default_max_depth(),
            include_hidden: false,
            follow_symlinks: false,
            same_filesystem: true,
            ignore_patterns: default_ignore_patterns(),
        }
    }
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            quarantine_dir: default_quarantine_dir(),
            require_confirmation: true,
            allow_permanent_delete: false,
            collision_policy: default_collision_policy(),
        }
    }
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            level: default_cleanup_level(),
            duplicate_detection: true,
            empty_dirs: true,
            temp_files: true,
            os_junk: true,
            old_archives: true,
            duplicate_max_bytes: default_duplicate_max_bytes(),
            old_archive_days: default_old_archive_days(),
        }
    }
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            organize: true,
            cleanup: true,
            recursive: None,
            max_depth: None,
            include_hidden: None,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_max_depth() -> usize {
    8
}

fn default_quarantine_dir() -> String {
    ".lla-quarantine".to_string()
}

fn default_collision_policy() -> String {
    "rename".to_string()
}

fn default_cleanup_level() -> String {
    "conservative".to_string()
}

fn default_duplicate_max_bytes() -> u64 {
    256 * 1024 * 1024
}

fn default_old_archive_days() -> u64 {
    90
}

fn default_ignore_patterns() -> Vec<String> {
    vec![
        ".git",
        "node_modules",
        "target",
        ".venv",
        "venv",
        ".idea",
        ".vscode",
        ".DS_Store",
        ".lla-quarantine",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("warning".to_string(), "bright_yellow".to_string());
    colors.insert("error".to_string(), "bright_red".to_string());
    colors.insert("path".to_string(), "bright_cyan".to_string());
    colors.insert("name".to_string(), "bright_white".to_string());
    colors
}

fn default_profiles() -> HashMap<String, ProfileConfig> {
    let mut profiles = HashMap::new();
    profiles.insert("downloads".to_string(), ProfileConfig::default());
    profiles.insert("desktop".to_string(), ProfileConfig::default());
    profiles.insert(
        "project".to_string(),
        ProfileConfig {
            organize: true,
            cleanup: true,
            recursive: Some(false),
            max_depth: Some(2),
            include_hidden: Some(false),
        },
    );
    profiles.insert(
        "media".to_string(),
        ProfileConfig {
            organize: true,
            cleanup: false,
            recursive: Some(true),
            max_depth: Some(6),
            include_hidden: Some(false),
        },
    );
    profiles
}

fn default_rules() -> Vec<RuleConfig> {
    vec![
        rule(
            "documents",
            "Documents",
            &["pdf", "doc", "docx", "txt", "md", "rtf", "odt"],
        ),
        rule(
            "images",
            "Images",
            &["jpg", "jpeg", "png", "gif", "bmp", "svg", "webp", "heic"],
        ),
        rule(
            "videos",
            "Videos",
            &["mp4", "mov", "avi", "mkv", "wmv", "webm"],
        ),
        rule(
            "audio",
            "Audio",
            &["mp3", "wav", "flac", "m4a", "aac", "ogg"],
        ),
        rule(
            "archives",
            "Archives",
            &["zip", "rar", "7z", "tar", "gz", "bz2", "xz"],
        ),
        rule(
            "code",
            "Code",
            &[
                "rs", "py", "js", "ts", "tsx", "jsx", "go", "rb", "java", "c", "cpp", "h", "hpp",
                "sh", "css", "html",
            ],
        ),
        rule("design", "Design", &["fig", "sketch", "psd", "ai", "xd"]),
        rule(
            "spreadsheets",
            "Spreadsheets",
            &["xls", "xlsx", "csv", "tsv", "ods"],
        ),
        rule(
            "presentations",
            "Presentations",
            &["ppt", "pptx", "key", "odp"],
        ),
        rule("books", "Books", &["epub", "mobi", "azw3", "fb2"]),
        rule(
            "installers",
            "Installers",
            &["dmg", "pkg", "exe", "msi", "appimage", "deb", "rpm"],
        ),
        rule("logs", "Logs", &["log"]),
    ]
}

fn rule(category: &str, destination: &str, extensions: &[&str]) -> RuleConfig {
    RuleConfig {
        category: category.to_string(),
        destination: destination.to_string(),
        extensions: extensions.iter().map(|ext| ext.to_string()).collect(),
        filename_patterns: Vec::new(),
        path_patterns: Vec::new(),
    }
}
