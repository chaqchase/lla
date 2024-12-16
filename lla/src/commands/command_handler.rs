use crate::commands::args::{Args, Command, InstallSource, ShortcutAction};
use crate::commands::file_utils::list_directory;
use crate::commands::plugin_utils::{handle_plugin_action, list_plugins};
use crate::config::{self, Config};
use crate::error::{LlaError, Result};
use crate::installer::PluginInstaller;
use crate::plugin::PluginManager;
use crate::utils::color::ColorState;
use colored::*;

pub fn handle_command(
    args: &Args,
    config: &mut Config,
    plugin_manager: &mut PluginManager,
    config_error: Option<LlaError>,
) -> Result<()> {
    let color_state = ColorState::new(args);

    match &args.command {
        Some(Command::Shortcut(action)) => handle_shortcut_action(action, config, &color_state),
        Some(Command::Install(source)) => handle_install(source, args),
        Some(Command::Update(plugin_name)) => {
            let installer = PluginInstaller::new(&args.plugins_dir, args);
            installer.update_plugins(plugin_name.as_deref())
        }
        Some(Command::ListPlugins) => list_plugins(plugin_manager),
        Some(Command::Use) => list_plugins(plugin_manager),
        Some(Command::InitConfig) => config::initialize_config(),
        Some(Command::Config(action)) => config::handle_config_command(action.clone()),
        Some(Command::PluginAction(plugin_name, action, action_args)) => {
            plugin_manager.perform_plugin_action(plugin_name, action, action_args)
        }
        Some(Command::Clean) => unreachable!(),
        None => list_directory(args, plugin_manager, config_error),
    }
}

fn handle_shortcut_action(
    action: &ShortcutAction,
    config: &mut Config,
    color_state: &ColorState,
) -> Result<()> {
    match action {
        ShortcutAction::Add(name, command) => {
            config.add_shortcut(name.clone(), command.clone())?;
            if color_state.is_enabled() {
                println!(
                    "✓ Added shortcut '{}' -> {} {}",
                    name.green(),
                    command.plugin_name.cyan(),
                    command.action.cyan()
                );
            } else {
                println!(
                    "✓ Added shortcut '{}' -> {} {}",
                    name, command.plugin_name, command.action
                );
            }
            if let Some(desc) = &command.description {
                println!("  Description: {}", desc);
            }
            Ok(())
        }
        ShortcutAction::Remove(name) => {
            if config.get_shortcut(name).is_some() {
                config.remove_shortcut(name)?;
                if color_state.is_enabled() {
                    println!("✓ Removed shortcut '{}'", name.green());
                } else {
                    println!("✓ Removed shortcut '{}'", name);
                }
            } else {
                if color_state.is_enabled() {
                    println!("✗ Shortcut '{}' not found", name.red());
                } else {
                    println!("✗ Shortcut '{}' not found", name);
                }
            }
            Ok(())
        }
        ShortcutAction::List => {
            if config.shortcuts.is_empty() {
                println!("No shortcuts configured");
                return Ok(());
            }
            if color_state.is_enabled() {
                println!("\n{}", "Configured Shortcuts:".cyan().bold());
            } else {
                println!("\nConfigured Shortcuts:");
            }
            for (name, cmd) in &config.shortcuts {
                if color_state.is_enabled() {
                    println!(
                        "\n{} → {} {}",
                        name.green(),
                        cmd.plugin_name.cyan(),
                        cmd.action.cyan()
                    );
                } else {
                    println!("\n{} → {} {}", name, cmd.plugin_name, cmd.action);
                }
                if let Some(desc) = &cmd.description {
                    println!("  Description: {}", desc);
                }
            }
            println!();
            Ok(())
        }
        ShortcutAction::Run(name, args) => match config.get_shortcut(name) {
            Some(shortcut) => {
                let plugin_name = shortcut.plugin_name.clone();
                let action = shortcut.action.clone();
                handle_plugin_action(config, &plugin_name, &action, args)
            }
            None => {
                if color_state.is_enabled() {
                    println!("✗ Shortcut '{}' not found", name.red());
                } else {
                    println!("✗ Shortcut '{}' not found", name);
                }
                Ok(())
            }
        },
    }
}

fn handle_install(source: &InstallSource, args: &Args) -> Result<()> {
    let installer = PluginInstaller::new(&args.plugins_dir, args);
    match source {
        InstallSource::GitHub(url) => installer.install_from_git(url),
        InstallSource::LocalDir(dir) => installer.install_from_directory(dir),
    }
}
