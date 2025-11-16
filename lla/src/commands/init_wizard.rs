use crate::config::Config;
use crate::error::Result;
use crate::theme;
use colored::*;
use dialoguer::{Confirm, Input, MultiSelect, Select};
use lla_plugin_utils::ui::components::LlaDialoguerTheme;
use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};

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

    print_wizard_banner(existed_already, &config_path);

    let ui_theme = LlaDialoguerTheme::default();
    const TOTAL_STEPS: usize = 5;
    let mut step = 1;

    print_section_header(
        step,
        TOTAL_STEPS,
        "Look & feel",
        "Themes, icons, and default presentation.",
    );
    step += 1;

    let show_icons = Confirm::with_theme(&ui_theme)
        .with_prompt("Show powerline-style icons next to entries?")
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
    let default_format_label = format_options[format_selection].0.to_string();

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
    let permission_label = PERMISSION_CHOICES[permission_selection].0.to_string();

    print_section_header(
        step,
        TOTAL_STEPS,
        "Listing defaults",
        "Control ordering, directories, and depth.",
    );
    step += 1;

    const SORT_CHOICES: [(&str, &str); 3] = [
        ("Name (A→Z)", "name"),
        ("Size (small → large)", "size"),
        ("Date (newest first)", "date"),
    ];
    let sort_labels: Vec<&str> = SORT_CHOICES.iter().map(|(label, _)| *label).collect();
    let default_sort_index = SORT_CHOICES
        .iter()
        .position(|(_, value)| *value == config.default_sort)
        .unwrap_or(0);
    let sort_selection = Select::with_theme(&ui_theme)
        .with_prompt("Default sort order?")
        .items(&sort_labels)
        .default(default_sort_index)
        .interact()?;
    let default_sort = SORT_CHOICES[sort_selection].1.to_string();
    let default_sort_label = SORT_CHOICES[sort_selection].0.to_string();

    let include_dirs = Confirm::with_theme(&ui_theme)
        .with_prompt("Include directories alongside files by default?")
        .default(config.include_dirs)
        .interact()?;

    let depth_initial = config
        .default_depth
        .map(|d| d.to_string())
        .unwrap_or_default();
    let depth_input: String = Input::with_theme(&ui_theme)
        .with_prompt("Depth limit for tree/listing views (blank = unlimited)")
        .with_initial_text(depth_initial)
        .allow_empty(true)
        .validate_with(|value: &String| -> std::result::Result<(), &str> {
            if value.trim().is_empty() {
                return Ok(());
            }
            match value.trim().parse::<usize>() {
                Ok(parsed) if parsed > 0 => Ok(()),
                _ => Err("Enter a positive number or leave blank"),
            }
        })
        .interact_text()?;
    let default_depth = if depth_input.trim().is_empty() {
        None
    } else {
        Some(depth_input.trim().parse::<usize>().unwrap())
    };

    print_section_header(
        step,
        TOTAL_STEPS,
        "Sorting & filtering",
        "Fine-tune how entries are grouped and hidden.",
    );
    step += 1;

    let dirs_first = Confirm::with_theme(&ui_theme)
        .with_prompt("Group directories before files?")
        .default(config.sort.dirs_first)
        .interact()?;
    let sort_case_sensitive = Confirm::with_theme(&ui_theme)
        .with_prompt("Use case-sensitive sorting?")
        .default(config.sort.case_sensitive)
        .interact()?;
    let natural_sort = Confirm::with_theme(&ui_theme)
        .with_prompt("Use natural sorting (1 < 10 < 100)?")
        .default(config.sort.natural)
        .interact()?;
    let filter_case_sensitive = Confirm::with_theme(&ui_theme)
        .with_prompt("Make filters case-sensitive?")
        .default(config.filter.case_sensitive)
        .interact()?;
    let hide_dotfiles = Confirm::with_theme(&ui_theme)
        .with_prompt("Hide dotfiles unless explicitly requested?")
        .default(config.filter.no_dotfiles)
        .interact()?;
    let respect_gitignore = Confirm::with_theme(&ui_theme)
        .with_prompt("Respect .gitignore when listing files?")
        .default(config.filter.respect_gitignore)
        .interact()?;

    print_section_header(
        step,
        TOTAL_STEPS,
        "Formatter details",
        "Customize long view behavior and columns.",
    );
    step += 1;

    let hide_group = Confirm::with_theme(&ui_theme)
        .with_prompt("Hide the group column in long view?")
        .default(config.formatters.long.hide_group)
        .interact()?;
    let relative_dates = Confirm::with_theme(&ui_theme)
        .with_prompt("Show relative timestamps (e.g., 2h ago) in long view?")
        .default(config.formatters.long.relative_dates)
        .interact()?;

    const BASE_LONG_COLUMN_CHOICES: [(&str, &str); 8] = [
        ("Permissions", "permissions"),
        ("Size", "size"),
        ("Modified", "modified"),
        ("User", "user"),
        ("Group", "group"),
        ("Name", "name"),
        ("Path", "path"),
        ("Plugins", "plugins"),
    ];
    let mut long_column_choices: Vec<(String, String)> = BASE_LONG_COLUMN_CHOICES
        .iter()
        .map(|(label, value)| (format!("{} {}", "▸", label), value.to_string()))
        .collect();
    for column in &config.formatters.long.columns {
        if !long_column_choices.iter().any(|(_, value)| value == column) {
            long_column_choices.push((format!("Custom: {}", column), column.clone()));
        }
    }
    let long_column_labels: Vec<String> = long_column_choices
        .iter()
        .map(|(label, _)| label.clone())
        .collect();
    let long_defaults: Vec<bool> = long_column_choices
        .iter()
        .map(|(_, value)| config.formatters.long.columns.contains(value))
        .collect();
    let selected_long_columns = MultiSelect::with_theme(&ui_theme)
        .with_prompt("Columns to show in long view (space = toggle, enter = accept)")
        .items(&long_column_labels)
        .defaults(&long_defaults)
        .interact()?;
    let mut long_columns: Vec<String> = if selected_long_columns.is_empty() {
        vec!["name".to_string()]
    } else {
        selected_long_columns
            .iter()
            .map(|idx| long_column_choices[*idx].1.clone())
            .collect()
    };
    long_columns.dedup();

    print_section_header(
        step,
        TOTAL_STEPS,
        "Plugins & automation",
        "Choose plugin folder, enabled plugins, and recursion safety limits.",
    );

    let plugin_dir_input: String = Input::with_theme(&ui_theme)
        .with_prompt("Directory that contains compiled plugins (.so/.dylib/.dll)")
        .with_initial_text(config.plugins_dir.display().to_string())
        .allow_empty(false)
        .interact_text()?;
    config.plugins_dir = expand_tilde_path(&plugin_dir_input);
    config.ensure_plugins_dir()?;

    let depth_guard_initial = config
        .listers
        .recursive
        .max_entries
        .map(|d| d.to_string())
        .unwrap_or_default();
    let depth_guard_input: String = Input::with_theme(&ui_theme)
        .with_prompt("Max entries when recursing (blank = unlimited, cap 100000)")
        .with_initial_text(depth_guard_initial)
        .allow_empty(true)
        .validate_with(|value: &String| -> std::result::Result<(), &str> {
            if value.trim().is_empty() {
                return Ok(());
            }
            match value.trim().parse::<usize>() {
                Ok(parsed) if parsed > 0 && parsed <= 100_000 => Ok(()),
                _ => Err("Enter a number between 1 and 100000 or leave blank"),
            }
        })
        .interact_text()?;
    let recursive_limit = if depth_guard_input.trim().is_empty() {
        None
    } else {
        Some(depth_guard_input.trim().parse::<usize>().unwrap())
    };

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
    config.default_sort = default_sort.clone();
    config.include_dirs = include_dirs;
    config.default_depth = default_depth;
    config.sort.dirs_first = dirs_first;
    config.sort.case_sensitive = sort_case_sensitive;
    config.sort.natural = natural_sort;
    config.filter.case_sensitive = filter_case_sensitive;
    config.filter.no_dotfiles = hide_dotfiles;
    config.filter.respect_gitignore = respect_gitignore;
    config.formatters.long.hide_group = hide_group;
    config.formatters.long.relative_dates = relative_dates;
    config.formatters.long.columns = long_columns.clone();
    config.listers.recursive.max_entries = recursive_limit;
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

    let long_columns_preview = long_columns.join(", ");
    let plugin_dir_display = format!("{}", config.plugins_dir.display());
    let recursive_guard_display =
        format_optional_limit(config.listers.recursive.max_entries, "entries");
    let plugins_display = config.enabled_plugins.join(", ");

    println!("\n{}", "✨ Wizard complete! Here's what we saved:".bold());
    println!(
        "{}",
        "────────────────────────────────────────────".bright_black()
    );
    print_summary_row("Theme", selected_theme.as_str().cyan());
    print_summary_row("Default view", default_format_label.as_str().green());
    print_summary_row("Permissions", permission_label.as_str().green());
    print_summary_row("Icons", format_toggle(show_icons, "enabled", "disabled"));
    print_summary_row("Sort order", default_sort_label.as_str().cyan());
    print_summary_row(
        "Include dirs",
        format_toggle(include_dirs, "include", "files only"),
    );
    print_summary_row(
        "Depth limit",
        format_optional_limit(default_depth, "levels"),
    );
    print_summary_row("Dirs first", format_toggle(dirs_first, "yes", "no"));
    print_summary_row(
        "Sort casing",
        format_toggle(sort_case_sensitive, "case sensitive", "ignore case"),
    );
    print_summary_row(
        "Natural sort",
        format_toggle(natural_sort, "natural", "lexical"),
    );
    print_summary_row(
        "Filter casing",
        format_toggle(filter_case_sensitive, "case sensitive", "ignore case"),
    );
    print_summary_row(
        "Hide dotfiles",
        format_toggle(hide_dotfiles, "hidden", "show"),
    );
    print_summary_row(
        "Gitignore filter",
        format_toggle(respect_gitignore, "respect .gitignore", "show all files"),
    );
    print_summary_row("Long view columns", long_columns_preview.as_str().purple());
    print_summary_row("Plugin dir", plugin_dir_display.as_str().yellow());
    print_summary_row("Recursive guard", recursive_guard_display);
    if config.enabled_plugins.is_empty() {
        print_summary_row("Plugins", "(none enabled)".bright_black());
    } else {
        print_summary_row("Plugins", plugins_display.as_str().magenta());
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

fn print_wizard_banner(existed_already: bool, config_path: &Path) {
    println!(
        "\n{}",
        "╔════════════════════════════════════════════════╗".bright_black()
    );
    println!(
        "{}",
        "║        ✨  lla interactive setup wizard  ✨    ║"
            .cyan()
            .bold()
    );
    println!(
        "{}",
        "╚════════════════════════════════════════════════╝".bright_black()
    );
    if existed_already {
        println!(
            "{} {}",
            "Editing config:".bright_black(),
            format!("{}", config_path.display()).cyan()
        );
    } else {
        println!(
            "{} {}",
            "Target config:".bright_black(),
            format!("{}", config_path.display()).cyan()
        );
    }
    println!(
        "{}",
        "Tip: Press space to toggle selections, Enter to accept.".bright_black()
    );
}

fn print_section_header(step: usize, total: usize, title: &str, subtitle: &str) {
    println!(
        "\n{} {}",
        format!("▸ Step {}/{}", step, total).bold().magenta(),
        title.bold()
    );
    println!("{}", subtitle.bright_black());
}

fn format_toggle(value: bool, on_label: &str, off_label: &str) -> String {
    if value {
        on_label.green().bold().to_string()
    } else {
        off_label.bright_black().to_string()
    }
}

fn format_optional_limit(value: Option<usize>, noun: &str) -> String {
    match value {
        Some(v) => format!("{} {}", v.to_string().bold(), noun),
        None => "no limit".bright_black().to_string(),
    }
}

fn print_summary_row(label: &str, value: impl Display) {
    println!("  {:<18} {}", label, value);
}

fn expand_tilde_path(input: &str) -> PathBuf {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return PathBuf::from(trimmed);
    }
    if trimmed == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    if let Some(stripped) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(trimmed)
}
