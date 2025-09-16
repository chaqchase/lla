use crate::commands::args::{Args, Command, InstallSource, ShortcutAction};
use crate::commands::file_utils::list_directory;
use crate::commands::jump;
use crate::commands::plugin_utils::{handle_plugin_action, list_plugins};
use crate::commands::search::run_search;
use crate::config::{self, Config};
use crate::error::{LlaError, Result};
use crate::installer::PluginInstaller;
use crate::plugin::PluginManager;
use crate::utils::color::ColorState;
use clap_complete;
use colored::*;
use std::fs::{self, create_dir_all, File};
use std::io::Write;

fn install_completion(
    shell: clap_complete::Shell,
    app: &mut clap::App,
    color_state: &ColorState,
    custom_path: Option<&str>,
    output_path: Option<&str>,
) -> Result<()> {
    let mut buf = Vec::new();
    clap_complete::generate(shell, app, env!("CARGO_PKG_NAME"), &mut buf);

    match output_path {
        Some(path) => {
            if let Some(parent) = std::path::Path::new(path).parent() {
                create_dir_all(parent)?;
            }
            let mut file = File::create(path)?;
            file.write_all(&buf)?;
            if color_state.is_enabled() {
                println!(
                    "✓ Generated {} shell completion to {}",
                    format!("{:?}", shell).green(),
                    path.cyan()
                );
            } else {
                println!("✓ Generated {:?} shell completion to {}", shell, path);
            }
            Ok(())
        }
        None => {
            let (install_path, post_install_msg) = if let Some(path) = custom_path {
                (
                    std::path::PathBuf::from(path),
                    "Restart your shell to apply changes",
                )
            } else {
                match shell {
                    clap_complete::Shell::Bash => {
                        let path = dirs::home_dir()
                            .map(|h| h.join(".local/share/bash-completion/completions"))
                            .ok_or_else(|| {
                                LlaError::Other("Could not determine home directory".into())
                            })?;
                        (
                            path.join("lla"),
                            "Restart your shell or run 'source ~/.bashrc'",
                        )
                    }
                    clap_complete::Shell::Fish => {
                        let path = dirs::home_dir()
                            .map(|h| h.join(".config/fish/completions"))
                            .ok_or_else(|| {
                                LlaError::Other("Could not determine home directory".into())
                            })?;
                        (
                            path.join("lla.fish"),
                            "Restart your shell or run 'source ~/.config/fish/config.fish'",
                        )
                    }
                    clap_complete::Shell::Zsh => {
                        let path = dirs::home_dir()
                            .map(|h| h.join(".zsh/completions"))
                            .ok_or_else(|| {
                                LlaError::Other("Could not determine home directory".into())
                            })?;
                        (
                            path.join("_lla"),
                            "Add 'fpath=(~/.zsh/completions $fpath)' to ~/.zshrc and restart your shell",
                        )
                    }
                    clap_complete::Shell::PowerShell => {
                        let path = dirs::home_dir()
                            .map(|h| h.join("Documents/WindowsPowerShell"))
                            .ok_or_else(|| {
                                LlaError::Other("Could not determine home directory".into())
                            })?;
                        (
                            path.join("lla.ps1"),
                            "Restart PowerShell or reload your profile",
                        )
                    }
                    clap_complete::Shell::Elvish => {
                        let path =
                            dirs::home_dir()
                                .map(|h| h.join(".elvish/lib"))
                                .ok_or_else(|| {
                                    LlaError::Other("Could not determine home directory".into())
                                })?;
                        (path.join("lla.elv"), "Restart your shell")
                    }
                    _ => return Err(LlaError::Other(format!("Unsupported shell: {:?}", shell))),
                }
            };

            if let Some(parent) = install_path.parent() {
                create_dir_all(parent)?;
            }
            fs::write(&install_path, buf)?;

            if color_state.is_enabled() {
                println!(
                    "✓ {} shell completion installed to {}",
                    format!("{:?}", shell).green(),
                    install_path.display().to_string().cyan()
                );
                println!("ℹ {}", post_install_msg.cyan());
            } else {
                println!(
                    "✓ {:?} shell completion installed to {}",
                    shell,
                    install_path.display()
                );
                println!("ℹ {}", post_install_msg);
            }
            Ok(())
        }
    }
}

pub fn handle_command(
    args: &Args,
    config: &mut Config,
    plugin_manager: &mut PluginManager,
    config_error: Option<LlaError>,
) -> Result<()> {
    let color_state = ColorState::new(args);

    match &args.command {
        Some(Command::GenerateCompletion(shell, custom_path, output_path)) => {
            let mut app = Args::get_cli(config);
            install_completion(
                *shell,
                &mut app,
                &color_state,
                custom_path.as_deref(),
                output_path.as_deref(),
            )
        }
        Some(Command::Theme) => crate::theme::select_theme(config),
        Some(Command::ThemePull) => crate::theme::pull_themes(&color_state),
        Some(Command::ThemeInstall(path)) => crate::theme::install_themes(&path, &color_state),
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
        Some(Command::Jump(action)) => jump::handle_jump(action, config),
        Some(Command::Clean) => unreachable!(),
        None => {
            if args.search.is_some() {
                run_search(args, config)
            } else {
                list_directory(args, config, plugin_manager, config_error)
            }
        }
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
            } else if color_state.is_enabled() {
                println!("✗ Shortcut '{}' not found", name.red());
            } else {
                println!("✗ Shortcut '{}' not found", name);
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
