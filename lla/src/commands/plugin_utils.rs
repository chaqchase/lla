use crate::config::Config;
use crate::error::Result;
use crate::plugin::PluginManager;
use colored::*;
use dialoguer::MultiSelect;
use lla_plugin_utils::ui::components::LlaDialoguerTheme;
use std::collections::HashSet;

pub fn list_plugins(plugin_manager: &mut PluginManager) -> Result<()> {
    let plugins: Vec<(String, String, String)> =
        plugin_manager.list_plugins().into_iter().collect();

    let plugin_names: Vec<String> = plugins
        .iter()
        .map(|(name, version, desc)| {
            format!(
                "{} {} - {}",
                name.cyan(),
                format!("v{}", version).yellow(),
                desc
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
                .map(|(name, _, _)| plugin_manager.enabled_plugins.contains(name))
                .collect::<Vec<_>>(),
        )
        .interact()?;

    let mut updated_plugins = HashSet::new();

    for idx in selections {
        let (name, _, _) = &plugins[idx];
        updated_plugins.insert(name.to_string());
    }

    for (name, _, _) in &plugins {
        if updated_plugins.contains(name) {
            plugin_manager.enable_plugin(name)?;
        } else {
            plugin_manager.disable_plugin(name)?;
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
    // Resolve plugin alias if exists
    let resolved_plugin = config.resolve_plugin_alias(plugin_name);
    let mut plugin_manager = PluginManager::new(config.clone());
    plugin_manager.discover_plugins(&config.plugins_dir)?;
    plugin_manager.perform_plugin_action(&resolved_plugin, action, args)
}
