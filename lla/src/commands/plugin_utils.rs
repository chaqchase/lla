use crate::config::Config;
use crate::error::Result;
use crate::plugin::PluginManager;
use colored::*;
use dialoguer::MultiSelect;
use lla_plugin_utils::ui::components::LlaDialoguerTheme;
use std::collections::HashSet;

pub fn list_plugins(plugin_manager: &mut PluginManager) -> Result<()> {
    let plugins = plugin_manager.list_plugins();

    let plugin_names: Vec<String> = plugins
        .iter()
        .map(|plugin_info| {
            let health_indicator = if let Some(health) = &plugin_info.health {
                if health.is_healthy {
                    "✓".green()
                } else {
                    "✗".red()
                }
            } else {
                "?".yellow()
            };

            format!(
                "{} {} {} - {}",
                health_indicator,
                plugin_info.name.cyan(),
                format!("v{}", plugin_info.version).yellow(),
                plugin_info.description
            )
        })
        .collect();

    let theme = LlaDialoguerTheme::default();
    let prompt = format!(
        "{}\n{}\n\nSelect plugins",
        "Plugin Manager".cyan().bold(),
        "Space to toggle, Enter to confirm".bright_black()
    );

    let selections = MultiSelect::with_theme(&theme)
        .with_prompt(prompt)
        .items(&plugin_names)
        .defaults(
            &plugins
                .iter()
                .map(|plugin_info| plugin_manager.enabled_plugins.contains(&plugin_info.name))
                .collect::<Vec<_>>(),
        )
        .interact()?;

    let mut updated_plugins = HashSet::new();

    for idx in selections {
        let plugin_info = &plugins[idx];
        updated_plugins.insert(plugin_info.name.clone());
    }

    for plugin_info in &plugins {
        if updated_plugins.contains(&plugin_info.name) {
            plugin_manager.enable_plugin(&plugin_info.name)?;
        } else {
            plugin_manager.disable_plugin(&plugin_info.name)?;
        }
    }

    Ok(())
}

pub fn handle_plugin_action(
    config: &mut Config,
    plugin_name: &str,
    action: &str,
    args: &[String],
) -> Result<()> {
    let mut plugin_manager = PluginManager::new(config.clone());
    plugin_manager.discover_plugins(&config.plugins_dir)?;
    plugin_manager.perform_plugin_action(plugin_name, action, args)
}
