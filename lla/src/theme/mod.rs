use crate::config::Config;
use crate::error::{LlaError, Result};
use crate::utils::color::ColorState;
use colored::Color;
use colored::*;
use dialoguer::Select;
use lla_plugin_utils::ui::components::LlaDialoguerTheme;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use tempfile;

static NO_COLOR: AtomicBool = AtomicBool::new(false);

pub fn set_no_color(value: bool) {
    NO_COLOR.store(value, Ordering::SeqCst);
}

pub fn is_no_color() -> bool {
    NO_COLOR.load(Ordering::SeqCst)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum ColorValue {
    Named(String),
    RGB { r: u8, g: u8, b: u8 },
    RGBA { r: u8, g: u8, b: u8, a: f32 },
    HSL { h: f32, s: f32, l: f32 },
    Hex(String),
    None,
}

impl Default for ColorValue {
    fn default() -> Self {
        ColorValue::Named("white".to_string())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Theme {
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub colors: ThemeColors,
    #[serde(default)]
    pub extensions: ExtensionColors,
    #[serde(default)]
    pub special_files: SpecialFiles,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SpecialFiles {
    #[serde(default)]
    pub dotfiles: HashMap<String, ColorValue>,
    #[serde(default)]
    pub exact_match: HashMap<String, ColorValue>,
    #[serde(default)]
    pub patterns: HashMap<String, ColorValue>,
    #[serde(default)]
    pub folders: HashMap<String, ColorValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ExtensionColors {
    #[serde(default)]
    pub groups: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub colors: HashMap<String, ColorValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThemeColors {
    #[serde(default = "default_file_color")]
    pub file: ColorValue,
    #[serde(default = "default_directory_color")]
    pub directory: ColorValue,
    #[serde(default = "default_symlink_color")]
    pub symlink: ColorValue,
    #[serde(default = "default_executable_color")]
    pub executable: ColorValue,
    #[serde(default = "default_size_color")]
    pub size: ColorValue,
    #[serde(default = "default_date_color")]
    pub date: ColorValue,
    #[serde(default = "default_user_color")]
    pub user: ColorValue,
    #[serde(default = "default_group_color")]
    pub group: ColorValue,
    #[serde(default = "default_permission_dir_color")]
    pub permission_dir: ColorValue,
    #[serde(default = "default_permission_read_color")]
    pub permission_read: ColorValue,
    #[serde(default = "default_permission_write_color")]
    pub permission_write: ColorValue,
    #[serde(default = "default_permission_exec_color")]
    pub permission_exec: ColorValue,
    #[serde(default = "default_permission_none_color")]
    pub permission_none: ColorValue,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            author: None,
            description: None,
            colors: ThemeColors::default(),
            extensions: ExtensionColors::default(),
            special_files: SpecialFiles::default(),
        }
    }
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            file: default_file_color(),
            directory: default_directory_color(),
            symlink: default_symlink_color(),
            executable: default_executable_color(),
            size: default_size_color(),
            date: default_date_color(),
            user: default_user_color(),
            group: default_group_color(),
            permission_dir: default_permission_dir_color(),
            permission_read: default_permission_read_color(),
            permission_write: default_permission_write_color(),
            permission_exec: default_permission_exec_color(),
            permission_none: default_permission_none_color(),
        }
    }
}

fn default_file_color() -> ColorValue {
    ColorValue::Named("white".to_string())
}
fn default_directory_color() -> ColorValue {
    ColorValue::Named("bright_blue".to_string())
}
fn default_symlink_color() -> ColorValue {
    ColorValue::Named("bright_cyan".to_string())
}
fn default_executable_color() -> ColorValue {
    ColorValue::Named("bright_green".to_string())
}
fn default_size_color() -> ColorValue {
    ColorValue::Named("green".to_string())
}
fn default_date_color() -> ColorValue {
    ColorValue::Named("bright_blue".to_string())
}
fn default_user_color() -> ColorValue {
    ColorValue::Named("cyan".to_string())
}
fn default_group_color() -> ColorValue {
    ColorValue::Named("bright_black".to_string())
}
fn default_permission_dir_color() -> ColorValue {
    ColorValue::Named("bright_blue".to_string())
}
fn default_permission_read_color() -> ColorValue {
    ColorValue::Named("bright_cyan".to_string())
}
fn default_permission_write_color() -> ColorValue {
    ColorValue::Named("bright_yellow".to_string())
}
fn default_permission_exec_color() -> ColorValue {
    ColorValue::Named("bright_red".to_string())
}
fn default_permission_none_color() -> ColorValue {
    ColorValue::Named("bright_black".to_string())
}

pub fn color_value_to_color(color_value: &ColorValue) -> Color {
    if is_no_color() {
        return Color::White;
    }

    match color_value {
        ColorValue::None => Color::White,
        ColorValue::Named(name) => str_to_color(name),
        ColorValue::RGB { r, g, b } => Color::TrueColor {
            r: *r,
            g: *g,
            b: *b,
        },
        ColorValue::RGBA { r, g, b, a: _ } => Color::TrueColor {
            r: *r,
            g: *g,
            b: *b,
        },
        ColorValue::HSL { h, s, l } => {
            let rgb = hsl_to_rgb(*h, *s, *l);
            Color::TrueColor {
                r: rgb.0,
                g: rgb.1,
                b: rgb.2,
            }
        }
        ColorValue::Hex(hex) => hex_to_color(hex),
    }
}

fn hex_to_color(hex: &str) -> Color {
    if let Some((r, g, b)) = parse_hex_color(hex) {
        Color::TrueColor { r, g, b }
    } else {
        Color::White
    }
}

fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim_start_matches('#');

    match hex.len() {
        3 => {
            // Convert 3-digit hex to 6-digit (#RGB -> #RRGGBB)
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            Some((r, g, b))
        }
        6 => {
            // Standard 6-digit hex
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        8 => {
            // 8-digit hex with alpha (ignore alpha)
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h % 360.0;
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);

    if s == 0.0 {
        let v = (l * 255.0) as u8;
        return (v, v, v);
    }

    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

pub fn str_to_color(color_str: &str) -> Color {
    if color_str.starts_with('#') {
        if let Some((r, g, b)) = parse_hex_color(color_str) {
            return Color::TrueColor { r, g, b };
        }
    }

    match color_str.to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "bright_black" | "gray" | "grey" => Color::BrightBlack,
        "bright_red" => Color::BrightRed,
        "bright_green" => Color::BrightGreen,
        "bright_yellow" => Color::BrightYellow,
        "bright_blue" => Color::BrightBlue,
        "bright_magenta" => Color::BrightMagenta,
        "bright_cyan" => Color::BrightCyan,
        "bright_white" => Color::BrightWhite,
        "navy" => Color::TrueColor { r: 0, g: 0, b: 128 },
        "teal" => Color::TrueColor {
            r: 0,
            g: 128,
            b: 128,
        },
        "maroon" => Color::TrueColor { r: 128, g: 0, b: 0 },
        "purple" => Color::TrueColor {
            r: 128,
            g: 0,
            b: 128,
        },
        "olive" => Color::TrueColor {
            r: 128,
            g: 128,
            b: 0,
        },
        "silver" => Color::TrueColor {
            r: 192,
            g: 192,
            b: 192,
        },
        _ => Color::White,
    }
}

pub fn load_theme(name: &str) -> Option<Theme> {
    if name == "none" {
        set_no_color(true);
        return Some(Theme::default());
    }

    let config_dir = dirs::home_dir()?.join(".config").join("lla").join("themes");
    let theme_file = config_dir.join(format!("{}.toml", name));

    if theme_file.exists() {
        fs::read_to_string(&theme_file)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
    } else {
        None
    }
}

pub fn list_themes() -> std::io::Result<Vec<String>> {
    let mut themes = vec!["default".to_string(), "none".to_string()];

    let config_dir = dirs::home_dir()
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "Home directory not found")
        })?
        .join(".config")
        .join("lla")
        .join("themes");

    if config_dir.exists() {
        for entry in fs::read_dir(config_dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) == Some("toml") {
                if let Some(name) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    if name != "default" && name != "none" {
                        themes.push(name.to_string());
                    }
                }
            }
        }
    }

    Ok(themes)
}

pub fn get_file_color(path: &std::path::Path) -> Option<Color> {
    let theme = get_theme()?;
    let filename = path.file_name()?.to_str()?;

    if path.is_dir() {
        if let Some(color) = theme.special_files.folders.get(filename) {
            return Some(color_value_to_color(color));
        }
        for (pattern, color) in &theme.special_files.folders {
            if pattern_matches(pattern, filename) {
                return Some(color_value_to_color(color));
            }
        }
        return Some(color_value_to_color(&theme.colors.directory));
    }

    if let Some(color) = theme.special_files.exact_match.get(filename) {
        return Some(color_value_to_color(color));
    }

    if filename.starts_with('.') {
        if let Some(color) = theme.special_files.dotfiles.get(filename) {
            return Some(color_value_to_color(color));
        }
    }

    for (pattern, color) in &theme.special_files.patterns {
        if pattern_matches(pattern, filename) {
            return Some(color_value_to_color(color));
        }
    }

    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext = ext.to_lowercase();
        if let Some(color) = theme.extensions.colors.get(&ext) {
            return Some(color_value_to_color(color));
        }

        for (group_name, _) in &theme.extensions.groups {
            if let Some(extensions) = theme.extensions.groups.get(group_name) {
                if extensions.iter().any(|e| e.to_lowercase() == ext) {
                    if let Some(color) = theme.extensions.colors.get(group_name) {
                        return Some(color_value_to_color(color));
                    }
                }
            }
        }
    }

    Some(color_value_to_color(&theme.colors.file))
}

fn pattern_matches(pattern: &str, filename: &str) -> bool {
    if pattern.starts_with('*') {
        filename.ends_with(&pattern[1..])
    } else if pattern.ends_with('*') {
        filename.starts_with(&pattern[..pattern.len() - 1])
    } else {
        filename == pattern
    }
}

fn get_theme() -> Option<&'static Theme> {
    use crate::utils::color::get_theme;
    Some(get_theme())
}

pub fn select_theme(config: &mut Config) -> Result<()> {
    if !atty::is(atty::Stream::Stdout) {
        println!("Available themes:");
        for theme in list_themes()? {
            let current = if theme == config.theme {
                " (current)"
            } else {
                ""
            };
            println!("{}{}", theme, current);
        }
        return Ok(());
    }

    let themes = list_themes()?;
    let current_index = themes.iter().position(|t| t == &config.theme).unwrap_or(0);

    let theme_items: Vec<String> = themes
        .iter()
        .map(|name| {
            if name == &config.theme {
                format!("{} {}", name.cyan(), "(current)".bright_black())
            } else {
                name.to_string()
            }
        })
        .collect();

    let theme = LlaDialoguerTheme::default();

    println!("\n{}", "Theme Manager".cyan().bold());
    println!(
        "{}\n",
        "Arrow keys to navigate, Enter to select".bright_black()
    );

    let selection = Select::with_theme(&theme)
        .with_prompt("Select theme")
        .items(&theme_items)
        .default(current_index)
        .interact()?;

    let selected_theme = &themes[selection];
    if selected_theme != &config.theme {
        config.set_value("theme", selected_theme)?;
        println!("✓ {} theme activated", selected_theme.green());
    }

    Ok(())
}

pub fn pull_themes(color_state: &ColorState) -> Result<()> {
    let config_dir = dirs::home_dir()
        .ok_or_else(|| LlaError::Other("Could not find home directory".into()))?
        .join(".config")
        .join("lla");
    let themes_dir = config_dir.join("themes");

    fs::create_dir_all(&themes_dir)?;
    let temp_dir = tempfile::tempdir()?;
    if color_state.is_enabled() {
        println!("{}", "Pulling themes from repository...".cyan());
    } else {
        println!("Pulling themes from repository...");
    }

    let status = Command::new("git")
        .args(&[
            "clone",
            "--depth=1",
            "https://github.com/chaqchase/lla.git",
            temp_dir.path().to_str().unwrap(),
        ])
        .status()?;

    if !status.success() {
        return Err(LlaError::Other("Failed to clone repository".into()));
    }

    let repo_themes_dir = temp_dir.path().join("themes");
    if repo_themes_dir.exists() {
        for entry in fs::read_dir(repo_themes_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let dest_path = themes_dir.join(path.file_name().unwrap());
                fs::copy(&path, &dest_path)?;
                if color_state.is_enabled() {
                    println!(
                        "✓ Installed theme: {}",
                        path.file_name().unwrap().to_string_lossy().green()
                    );
                } else {
                    println!(
                        "✓ Installed theme: {}",
                        path.file_name().unwrap().to_string_lossy()
                    );
                }
            }
        }
    }

    if color_state.is_enabled() {
        println!("\n{}", "Themes installed successfully!".green());
        println!("Use {} to select a theme", "lla theme".cyan());
    } else {
        println!("\nThemes installed successfully!");
        println!("Use 'lla theme' to select a theme");
    }

    Ok(())
}

pub fn install_themes(path: &str, color_state: &ColorState) -> Result<()> {
    let config_dir = dirs::home_dir()
        .ok_or_else(|| LlaError::Other("Could not find home directory".into()))?
        .join(".config")
        .join("lla");
    let themes_dir = config_dir.join("themes");

    // Create themes directory if it doesn't exist
    fs::create_dir_all(&themes_dir)?;

    let path = std::path::Path::new(path);
    if !path.exists() {
        return Err(LlaError::Other(format!(
            "Path does not exist: {}",
            path.display()
        )));
    }

    let mut installed_count = 0;

    if path.is_file() {
        if path.extension().and_then(|s| s.to_str()) == Some("toml") {
            let dest_path = themes_dir.join(path.file_name().unwrap());
            fs::copy(path, &dest_path)?;
            if color_state.is_enabled() {
                println!(
                    "✓ Installed theme: {}",
                    path.file_name().unwrap().to_string_lossy().green()
                );
            } else {
                println!(
                    "✓ Installed theme: {}",
                    path.file_name().unwrap().to_string_lossy()
                );
            }
            installed_count += 1;
        } else {
            return Err(LlaError::Other(
                "Theme file must have .toml extension".into(),
            ));
        }
    } else if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let dest_path = themes_dir.join(path.file_name().unwrap());
                fs::copy(&path, &dest_path)?;
                if color_state.is_enabled() {
                    println!(
                        "✓ Installed theme: {}",
                        path.file_name().unwrap().to_string_lossy().green()
                    );
                } else {
                    println!(
                        "✓ Installed theme: {}",
                        path.file_name().unwrap().to_string_lossy()
                    );
                }
                installed_count += 1;
            }
        }
    }

    if installed_count > 0 {
        if color_state.is_enabled() {
            println!(
                "\n{}",
                format!("{} theme(s) installed successfully!", installed_count).green()
            );
            println!("Use {} to select a theme", "lla theme".cyan());
        } else {
            println!("\n{} theme(s) installed successfully!", installed_count);
            println!("Use 'lla theme' to select a theme");
        }
    } else {
        if color_state.is_enabled() {
            println!("{}", "No themes were installed".yellow());
        } else {
            println!("No themes were installed");
        }
    }

    Ok(())
}

pub fn preview_theme(theme_name: &str) -> Result<()> {
    let theme = load_theme(theme_name).ok_or_else(|| {
        LlaError::Other(format!(
            "Theme '{}' not found. Install it with `lla theme pull` or place it in ~/.config/lla/themes",
            theme_name
        ))
    })?;

    println!();
    println!(
        "{} {}",
        "Theme preview:".cyan().bold(),
        theme.name.as_str().bold()
    );
    if let Some(author) = &theme.author {
        println!("Author : {}", author);
    }
    if let Some(desc) = &theme.description {
        println!("Notes  : {}", desc);
    }

    println!("\n{}", "Core palette".bright_black());
    render_palette(&theme);

    println!("\n{}", "Sample directory listing".bright_black());
    render_directory_sample(&theme);

    println!("\n{}", "Sample search output".bright_black());
    render_search_sample(&theme);
    println!();
    Ok(())
}

fn render_palette(theme: &Theme) {
    let palette = [
        ("Files", &theme.colors.file),
        ("Dirs", &theme.colors.directory),
        ("Symlinks", &theme.colors.symlink),
        ("Exec", &theme.colors.executable),
        ("Size", &theme.colors.size),
        ("Date", &theme.colors.date),
    ];
    for (label, color_value) in palette {
        let block = format!(" {:<10} ", label);
        print!("{}", paint(&block, color_value).bold());
    }
    println!();
}

#[derive(Clone, Copy)]
enum PreviewEntryKind {
    Directory,
    File,
    Executable,
    Symlink,
    Dotfile,
}

fn render_directory_sample(theme: &Theme) {
    struct SampleEntry<'a> {
        perms: &'a str,
        user: &'a str,
        group: &'a str,
        size: &'a str,
        date: &'a str,
        icon: &'a str,
        name: &'a str,
        kind: PreviewEntryKind,
    }

    let entries = [
        SampleEntry {
            perms: "drwxr-xr-x",
            user: "mona",
            group: "staff",
            size: "4.0K",
            date: "Nov 15 10:22",
            icon: "\u{f07b}", // folder
            name: "src/",
            kind: PreviewEntryKind::Directory,
        },
        SampleEntry {
            perms: "-rw-r--r--",
            user: "mona",
            group: "staff",
            size: "2.4K",
            date: "Nov 14 18:02",
            icon: "\u{e7a8}", // Cargo/Rust icon
            name: "Cargo.toml",
            kind: PreviewEntryKind::File,
        },
        SampleEntry {
            perms: "-rwxr-xr-x",
            user: "mona",
            group: "staff",
            size: "8.8K",
            date: "Nov 13 09:10",
            icon: "\u{f0ad}", // gear/executable
            name: "scripts/build.sh",
            kind: PreviewEntryKind::Executable,
        },
        SampleEntry {
            perms: "lrwxrwxrwx",
            user: "mona",
            group: "staff",
            size: "9B",
            date: "Nov 12 08:01",
            icon: "\u{f0c1}", // link
            name: "current -> releases/2025",
            kind: PreviewEntryKind::Symlink,
        },
        SampleEntry {
            perms: "-rw-r--r--",
            user: "mona",
            group: "staff",
            size: "640B",
            date: "Nov 11 21:58",
            icon: "\u{f462}", // .env / config icon
            name: ".env",
            kind: PreviewEntryKind::Dotfile,
        },
    ];

    println!("{}", "Default view (lla)".bright_black());
    for entry in &entries {
        let name = format_name(entry.icon, entry.name, entry.kind, theme);
        println!("  {}", name);
    }

    println!("\n{}", "Long view (lla -l)".bright_black());
    let max_size_len = entries
        .iter()
        .map(|e| e.size.len())
        .max()
        .unwrap_or(0)
        .max(4);
    let max_date_len = entries
        .iter()
        .map(|e| e.date.len())
        .max()
        .unwrap_or(0)
        .max(8);
    let max_user_len = entries
        .iter()
        .map(|e| e.user.len())
        .max()
        .unwrap_or(0)
        .max(4);
    let max_group_len = entries
        .iter()
        .map(|e| e.group.len())
        .max()
        .unwrap_or(0)
        .max(5);

    println!(
        "{} {} {} {} {} {}",
        pad_right_plain("Permissions", 12),
        pad_left_plain("Size", max_size_len),
        pad_right_plain("Modified", max_date_len),
        pad_right_plain("User", max_user_len),
        pad_right_plain("Group", max_group_len),
        "Name"
    );
    for entry in &entries {
        let perms = format_permissions_sample(entry.perms, theme);
        let size_colored = paint(entry.size, &theme.colors.size).to_string();
        let size_field = pad_left_colored(&size_colored, entry.size.len(), max_size_len);
        let date_colored = paint(entry.date, &theme.colors.date).to_string();
        let date_field = pad_right_colored(&date_colored, entry.date.len(), max_date_len);
        let user_colored = paint(entry.user, &theme.colors.user).to_string();
        let user_field = pad_right_colored(&user_colored, entry.user.len(), max_user_len);
        let group_colored = paint(entry.group, &theme.colors.group).to_string();
        let group_field = pad_right_colored(&group_colored, entry.group.len(), max_group_len);
        let name = format_name(entry.icon, entry.name, entry.kind, theme);
        println!(
            "{} {} {} {} {} {}",
            pad_right_colored(&perms, entry.perms.len(), 12),
            size_field,
            date_field,
            user_field,
            group_field,
            name
        );
    }
}

fn render_search_sample(theme: &Theme) {
    let path = paint("src/main.rs", &theme.colors.directory).bold();
    let line_no = paint("42", &theme.colors.date);
    let line = "    let theme = load_theme(name)?;";
    let highlighted = highlight_match(line, "theme", &theme.colors.executable);
    let caret = paint("^^^^", &theme.colors.permission_exec).bold();

    println!("{}", "rg --search \"theme\"".bright_black());
    println!("{}:{} {}", path, line_no, highlighted);
    println!("         {}", caret);
    println!(
        "{}",
        "1 match across 1 file".color(color_value_to_color(&theme.colors.permission_none))
    );
}

fn highlight_match(line: &str, needle: &str, color: &ColorValue) -> String {
    if let Some(index) = line.find(needle) {
        let before = &line[..index];
        let after = &line[index + needle.len()..];
        format!(
            "{}{}{}",
            before,
            paint(needle, color).bold(),
            highlight_match(after, needle, color)
        )
    } else {
        line.to_string()
    }
}

fn format_name(icon: &str, name: &str, kind: PreviewEntryKind, theme: &Theme) -> ColoredString {
    let text = format!("{} {}", icon, name);
    match kind {
        PreviewEntryKind::Directory => paint(&text, &theme.colors.directory).bold(),
        PreviewEntryKind::File => paint(&text, &theme.colors.file),
        PreviewEntryKind::Executable => paint(&text, &theme.colors.executable).bold(),
        PreviewEntryKind::Symlink => paint(&text, &theme.colors.symlink).italic(),
        PreviewEntryKind::Dotfile => paint(&text, &theme.colors.permission_none),
    }
}

fn format_permissions_sample(symbolic: &str, theme: &Theme) -> String {
    symbolic
        .chars()
        .map(|ch| match ch {
            'd' | 'l' => paint_char(ch, &theme.colors.permission_dir),
            'r' => paint_char(ch, &theme.colors.permission_read),
            'w' => paint_char(ch, &theme.colors.permission_write),
            'x' => paint_char(ch, &theme.colors.permission_exec),
            _ => paint_char(ch, &theme.colors.permission_none),
        })
        .collect::<Vec<_>>()
        .join("")
}

fn pad_left_colored(text: &str, plain_len: usize, width: usize) -> String {
    if plain_len >= width {
        text.to_string()
    } else {
        format!("{}{}", " ".repeat(width - plain_len), text)
    }
}

fn pad_right_colored(text: &str, plain_len: usize, width: usize) -> String {
    if plain_len >= width {
        text.to_string()
    } else {
        format!("{}{}", text, " ".repeat(width - plain_len))
    }
}

fn pad_right_plain(text: &str, width: usize) -> String {
    if text.len() >= width {
        text.to_string()
    } else {
        format!("{:<width$}", text, width = width)
    }
}

fn pad_left_plain(text: &str, width: usize) -> String {
    if text.len() >= width {
        text.to_string()
    } else {
        format!("{:>width$}", text, width = width)
    }
}

fn paint_char(ch: char, color: &ColorValue) -> String {
    paint(&ch.to_string(), color).to_string()
}

fn paint(text: &str, color_value: &ColorValue) -> ColoredString {
    text.color(color_value_to_color(color_value))
}
