use crate::config::Config;
use crate::error::Result;
use crate::theme;
use colored::*;
use dialoguer::{Confirm, MultiSelect, Select};
use lla_plugin_utils::ui::components::LlaDialoguerTheme;
use std::fs;
use std::path::Path;

pub fn run_wizard() -> Result<()> {
    let config_path = Config::get_config_path();
    let config_dir = config_path
        .parent()
        .expect("Config path must have a parent directory");

    fs::create_dir_all(config_dir)?;
    let themes_dir = config_dir.join("themes");
    fs::create_dir_all(&themes_dir)?;
    seed_default_theme(&themes_dir)?;

    let (mut config, existed_already) = if config_path.exists() {
        match Config::load(&config_path) {
            Ok(cfg) => (cfg, true),
            Err(err) => {
                println!(
                    "{}",
                    format!(
                        "⚠ Failed to load existing config ({}). Falling back to defaults.",
                        err
                    )
                    .yellow()
                );
                (Config::default(), true)
            }
        }
    } else {
        (Config::default(), false)
    };

    println!("\n{}", "Welcome to the lla setup wizard ✨".cyan().bold());
    println!(
        "{}\n",
        "Answer a few quick questions and we will generate a tuned config for you.".bright_black()
    );

    let ui_theme = LlaDialoguerTheme::default();

    let show_icons = Confirm::with_theme(&ui_theme)
        .with_prompt("Show icons by default?")
        .default(config.show_icons)
        .interact()?;

    let available_themes = theme::list_themes().unwrap_or_else(|_| vec!["default".to_string()]);
    let theme_labels: Vec<String> = available_themes
        .iter()
        .map(|name| {
            if name == "default" {
                format!("{} {}", name, "(built-in)")
            } else {
                name.to_string()
            }
        })
        .collect();
    let default_theme_index = available_themes
        .iter()
        .position(|t| t == &config.theme)
        .or_else(|| available_themes.iter().position(|t| t == "default"))
        .unwrap_or(0);
    let theme_selection = Select::with_theme(&ui_theme)
        .with_prompt("Pick a color theme")
        .items(&theme_labels)
        .default(default_theme_index)
        .interact()?;
    let selected_theme = available_themes[theme_selection].clone();

    let format_options = [
        ("Recommended default", "default"),
        ("Tree (hierarchical)", "tree"),
        ("Long (detailed)", "long"),
        ("Grid (compact columns)", "grid"),
        ("Table (structured)", "table"),
        ("Timeline (by dates)", "timeline"),
        ("Git status dashboard", "git"),
        ("Size map (disk usage)", "sizemap"),
        ("Fuzzy finder", "fuzzy"),
    ];
    let format_labels: Vec<&str> = format_options.iter().map(|(label, _)| *label).collect();
    let default_format_index = format_options
        .iter()
        .position(|(_, value)| *value == config.default_format)
        .unwrap_or_else(|| {
            format_options
                .iter()
                .position(|(_, value)| *value == "default")
                .unwrap_or(0)
        });
    let format_selection = Select::with_theme(&ui_theme)
        .with_prompt("Which view should open by default?")
        .items(&format_labels)
        .default(default_format_index)
        .interact()?;
    let default_format = format_options[format_selection].1.to_string();

    const PERMISSION_CHOICES: [(&str, &str); 5] = [
        ("Symbolic (-rwxr-xr-x)", "symbolic"),
        ("Octal (755)", "octal"),
        ("Binary (111101101)", "binary"),
        ("Verbose (owner:rwx ...)", "verbose"),
        ("Compact (755)", "compact"),
    ];
    let permission_index = PERMISSION_CHOICES
        .iter()
        .position(|(_, value)| *value == config.permission_format)
        .unwrap_or(0);
    let permission_selection = Select::with_theme(&ui_theme)
        .with_prompt("Preferred permission format?")
        .items(
            &PERMISSION_CHOICES
                .iter()
                .map(|(label, _)| *label)
                .collect::<Vec<_>>(),
        )
        .default(permission_index)
        .interact()?;
    let permission_format = PERMISSION_CHOICES[permission_selection].1.to_string();

    let installed_plugins = discover_plugins(&config.plugins_dir).unwrap_or_default();
    let selected_plugins = if installed_plugins.is_empty() {
        config.enabled_plugins.clone()
    } else {
        let mut plugin_items = installed_plugins.clone();
        plugin_items.sort();
        let defaults: Vec<bool> = plugin_items
            .iter()
            .map(|name| config.enabled_plugins.contains(name))
            .collect();
        let chosen = MultiSelect::with_theme(&ui_theme)
            .with_prompt("Enable any installed plugins?")
            .items(&plugin_items)
            .defaults(&defaults)
            .interact()?;
        chosen
            .into_iter()
            .map(|idx| plugin_items[idx].clone())
            .collect()
    };

    config.show_icons = show_icons;
    config.theme = selected_theme.clone();
    config.default_format = default_format.clone();
    config.permission_format = permission_format.clone();
    config.enabled_plugins = selected_plugins;
    config.ensure_plugins_dir()?;
    config.save(&config_path)?;

    println!(
        "\n{} {}",
        "✓ Configuration saved to".green(),
        config_path.display()
    );
    if existed_already {
        println!("  (Updated existing configuration)");
    }
    println!("  Theme        : {}", selected_theme);
    println!(
        "  Icons        : {}",
        if show_icons { "enabled" } else { "disabled" }
    );
    println!("  Default view : {}", default_format);
    println!("  Permissions  : {}", permission_format);
    if config.enabled_plugins.is_empty() {
        println!("  Plugins      : (none enabled)");
    } else {
        println!("  Plugins      : {}", config.enabled_plugins.join(", "));
    }
    println!(
        "Next steps: try `{}` or `{}` to tweak things further.",
        "lla config show-effective".cyan(),
        "lla config diff --default".cyan()
    );
    Ok(())
}

fn seed_default_theme(themes_dir: &Path) -> Result<()> {
    let default_theme_path = themes_dir.join("default.toml");
    if !default_theme_path.exists() {
        let default_theme_content = include_str!("../config/default.toml");
        fs::write(&default_theme_path, default_theme_content)?;
    }
    Ok(())
}

fn discover_plugins(plugins_dir: &Path) -> std::io::Result<Vec<String>> {
    let mut names = Vec::new();
    if !plugins_dir.exists() {
        return Ok(names);
    }
    for entry in fs::read_dir(plugins_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            if let Some(name) = entry.path().file_name().and_then(|s| s.to_str()) {
                if let Some(canonical) = canonical_plugin_name(name) {
                    names.push(canonical);
                }
            }
        }
    }
    Ok(names)
}

fn canonical_plugin_name(filename: &str) -> Option<String> {
    let mut name = filename.to_string();
    if let Some(ext) = Path::new(filename).extension().and_then(|s| s.to_str()) {
        if ["so", "dylib", "dll"].contains(&ext) {
            name = filename[..filename.len() - ext.len() - 1].to_string();
        }
    }
    if let Some(stripped) = name.strip_prefix("lib") {
        name = stripped.to_string();
    }
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}
