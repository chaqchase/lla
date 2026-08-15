use crate::config::Config;
use crate::error::Result;
use crate::plugin::PluginManager;
use colored::*;
use dialoguer::MultiSelect;
use lla_plugin_utils::ui::components::{BoxComponent, BoxStyle, LlaDialoguerTheme};
use std::collections::HashSet;

fn term_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

fn truncate_desc(desc: &str, max: usize) -> String {
    if desc.len() <= max {
        desc.to_string()
    } else if max > 1 {
        format!("{}…", &desc[..max - 1])
    } else {
        String::new()
    }
}

pub fn list_plugins(plugin_manager: &mut PluginManager) -> Result<()> {
    let plugins: Vec<(String, String, String)> =
        plugin_manager.list_plugins().into_iter().collect();

    if plugins.is_empty() {
        let content = format!(
            "  {}  No plugins installed.\n\n     Run {} to get started.",
            "ℹ".cyan().bold(),
            "lla install --help".yellow().bold()
        );
        let output = BoxComponent::new(content)
            .style(BoxStyle::Rounded)
            .title(format!("{}", "Plugins".cyan().bold()))
            .padding(1)
            .render();
        println!("{}", output);
        return Ok(());
    }

    let mut enabled: Vec<&(String, String, String)> = Vec::new();
    let mut disabled: Vec<&(String, String, String)> = Vec::new();

    for plugin in &plugins {
        if plugin_manager.enabled_plugins.contains(&plugin.0) {
            enabled.push(plugin);
        } else {
            disabled.push(plugin);
        }
    }

    let max_name_width = plugins
        .iter()
        .map(|(name, _, _)| name.len())
        .max()
        .unwrap_or(0);
    let max_ver_width = plugins
        .iter()
        .map(|(_, ver, _)| ver.len() + 1)
        .max()
        .unwrap_or(0);

    // box borders (2) + box padding (2) + indent (6) + name + 2 + ver + 2 + sep (1) + 2
    let fixed_cols = 2 + 2 + 6 + max_name_width + 2 + max_ver_width + 2 + 1 + 2;
    let tw = term_width();
    let desc_budget = tw.saturating_sub(fixed_cols);

    let mut lines: Vec<String> = Vec::new();

    if !enabled.is_empty() {
        lines.push(format!(
            "  {} {}",
            "●".bright_green(),
            format!("Enabled ({})", enabled.len()).bright_green().bold()
        ));
        lines.push(String::new());

        for (name, version, desc) in &enabled {
            let padded_name = format!("{:<width$}", name, width = max_name_width);
            let padded_ver = format!("v{:<width$}", version, width = max_ver_width - 1);
            let desc_text = truncate_desc(desc, desc_budget);
            lines.push(format!(
                "      {}  {}  {}  {}",
                padded_name.cyan().bold(),
                padded_ver.bright_black(),
                "│".bright_black(),
                desc_text,
            ));
        }
    }

    if !enabled.is_empty() && !disabled.is_empty() {
        lines.push(String::new());
        let separator_width = tw.saturating_sub(2 + 2 + 6 + 2).min(60);
        lines.push(format!(
            "      {}",
            "·".repeat(separator_width).bright_black()
        ));
        lines.push(String::new());
    }

    if !disabled.is_empty() {
        lines.push(format!(
            "  {} {}",
            "○".bright_black(),
            format!("Disabled ({})", disabled.len())
                .bright_black()
                .bold()
        ));
        lines.push(String::new());

        for (name, version, desc) in &disabled {
            let padded_name = format!("{:<width$}", name, width = max_name_width);
            let padded_ver = format!("v{:<width$}", version, width = max_ver_width - 1);
            let desc_text = truncate_desc(desc, desc_budget);
            lines.push(format!(
                "      {}  {}  {}  {}",
                padded_name.bright_black(),
                padded_ver.bright_black(),
                "│".bright_black(),
                desc_text.bright_black(),
            ));
        }
    }

    let content = lines.join("\n");
    let output = BoxComponent::new(content)
        .style(BoxStyle::Rounded)
        .title(format!("{}", "Installed Plugins".cyan().bold()))
        .padding(1)
        .render();
    println!("{}", output);

    let total = plugins.len();
    let en = enabled.len();
    let dis = disabled.len();
    println!(
        "  {}  {} {} {} {}",
        "∑".bright_black(),
        format!("{} enabled", en).bright_green(),
        "·".bright_black(),
        format!("{} disabled", dis).bright_black(),
        format!("· {} total", total).bright_black()
    );
    println!();

    Ok(())
}

pub fn use_plugins(plugin_manager: &mut PluginManager) -> Result<()> {
    let plugins: Vec<(String, String, String)> =
        plugin_manager.list_plugins().into_iter().collect();

    if plugins.is_empty() {
        let content = format!(
            "  {}  No plugins installed.\n\n     Run {} to get started.",
            "ℹ".cyan().bold(),
            "lla install --help".yellow().bold()
        );
        let output = BoxComponent::new(content)
            .style(BoxStyle::Rounded)
            .title(format!("{}", "Plugin Manager".cyan().bold()))
            .padding(1)
            .render();
        println!("{}", output);
        return Ok(());
    }

    let header_content = format!(
        "  Toggle plugins on/off for {}.\n  {} to toggle  {}  {} to confirm  {}  {} to cancel",
        "lla".cyan().bold(),
        "⟨Space⟩".bright_white().bold(),
        "·".bright_black(),
        "⟨Enter⟩".bright_white().bold(),
        "·".bright_black(),
        "⟨Esc⟩".bright_black(),
    );
    let header = BoxComponent::new(header_content)
        .style(BoxStyle::Rounded)
        .title(format!("{}", "Plugin Manager".cyan().bold()))
        .padding(1)
        .render();
    println!("{}", header);

    let max_name_width = plugins
        .iter()
        .map(|(name, _, _)| name.len())
        .max()
        .unwrap_or(0);
    let max_ver_width = plugins
        .iter()
        .map(|(_, ver, _)| ver.len() + 1)
        .max()
        .unwrap_or(0);

    // Account for multiselect prefix (pointer + checkbox + spaces ~8) + name + ver + sep
    let tw = term_width();
    let fixed_cols = 8 + max_name_width + 2 + max_ver_width + 2 + 1 + 2;
    let desc_budget = tw.saturating_sub(fixed_cols);

    let plugin_names: Vec<String> = plugins
        .iter()
        .map(|(name, version, desc)| {
            let padded_name = format!("{:<width$}", name, width = max_name_width);
            let padded_ver = format!("v{:<width$}", version, width = max_ver_width - 1);
            let desc_text = truncate_desc(desc, desc_budget);
            format!(
                "{}  {}  {}  {}",
                padded_name.cyan().bold(),
                padded_ver.bright_black(),
                "│".bright_black(),
                desc_text
            )
        })
        .collect();

    let old_enabled: HashSet<String> = plugin_manager.enabled_plugins.clone();

    let theme = LlaDialoguerTheme::default();
    let selections = MultiSelect::with_theme(&theme)
        .with_prompt("Select plugins to enable")
        .items(&plugin_names)
        .defaults(
            &plugins
                .iter()
                .map(|(name, _, _)| plugin_manager.enabled_plugins.contains(name))
                .collect::<Vec<_>>(),
        )
        .interact()?;

    let mut new_enabled = HashSet::new();
    for idx in selections {
        let (name, _, _) = &plugins[idx];
        new_enabled.insert(name.to_string());
    }

    for (name, _, _) in &plugins {
        if new_enabled.contains(name) {
            plugin_manager.enable_plugin(name)?;
        } else {
            plugin_manager.disable_plugin(name)?;
        }
    }

    let newly_enabled: Vec<&String> = new_enabled
        .iter()
        .filter(|name| !old_enabled.contains(*name))
        .collect();
    let newly_disabled: Vec<&String> = old_enabled
        .iter()
        .filter(|name| !new_enabled.contains(*name))
        .collect();

    println!();
    if newly_enabled.is_empty() && newly_disabled.is_empty() {
        println!(
            "  {}  {}",
            "·".bright_black(),
            "No changes made.".bright_black()
        );
    } else {
        let mut summary_lines: Vec<String> = Vec::new();

        if !newly_enabled.is_empty() {
            let names: Vec<String> = newly_enabled
                .iter()
                .map(|n| n.cyan().bold().to_string())
                .collect();
            summary_lines.push(format!(
                "  {}  {}  {}",
                "●".bright_green().bold(),
                "Enabled".bright_green().bold(),
                names.join(&format!("  {}  ", "·".bright_black()))
            ));
        }
        if !newly_disabled.is_empty() {
            let names: Vec<String> = newly_disabled
                .iter()
                .map(|n| n.bright_black().to_string())
                .collect();
            summary_lines.push(format!(
                "  {}  {}  {}",
                "○".bright_black(),
                "Disabled".bright_black().bold(),
                names.join(&format!("  {}  ", "·".bright_black()))
            ));
        }

        let summary = BoxComponent::new(summary_lines.join("\n"))
            .style(BoxStyle::Rounded)
            .title(format!("{}", "Changes".cyan().bold()))
            .padding(1)
            .render();
        println!("{}", summary);
    }

    Ok(())
}

pub fn handle_plugin_action(
    config: &mut Config,
    plugin_name: &str,
    action: &str,
    args: &[String],
) -> Result<()> {
    let resolved_plugin = config.resolve_plugin_alias(plugin_name);
    let mut plugin_manager = PluginManager::new(config.clone());
    let requested = std::collections::HashSet::from([resolved_plugin.clone()]);
    plugin_manager.discover_plugins_named(&config.plugins_dir, &requested)?;
    plugin_manager.perform_plugin_action(&resolved_plugin, action, args)
}
