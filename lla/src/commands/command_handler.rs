use crate::commands::args::{Args, Command, InstallSource, ShortcutAction};
use crate::commands::file_utils::list_directory;
use crate::commands::jump;
use crate::commands::plugin_utils::{handle_plugin_action, list_plugins};
use crate::commands::search::run_search;
use crate::config::{self, Config, ShortcutCommand};
use crate::error::{LlaError, Result};
use crate::installer::PluginInstaller;
use crate::plugin::PluginManager;
use crate::utils::color::ColorState;
use clap_complete;
use colored::*;
use dialoguer::{Input, Select};
use lla_plugin_utils::ui::components::LlaDialoguerTheme;
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
            // Resolve plugin alias if exists
            let resolved_plugin = config.resolve_plugin_alias(plugin_name);
            plugin_manager.perform_plugin_action(&resolved_plugin, action, action_args)
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
        ShortcutAction::Create => {
            let theme = LlaDialoguerTheme::default();

            // Create plugin manager to query plugins
            let mut plugin_manager = PluginManager::new(config.clone());
            plugin_manager.discover_plugins(&config.plugins_dir)?;

            // Get all discovered plugins
            let all_plugins = plugin_manager.list_plugins();
            let plugin_names: Vec<String> = all_plugins
                .iter()
                .map(|(name, _, _)| name.clone())
                .collect();

            if plugin_names.is_empty() {
                if color_state.is_enabled() {
                    println!("✗ No plugins found. Install plugins first",);
                } else {
                    println!("✗ No plugins found. Install plugins first");
                }
                return Ok(());
            }

            // State machine for wizard navigation
            enum WizardState {
                SelectPlugin,
                SelectAction(String),
                EnterName(String, String),
                EnterDescription(String, String, String),
                Done,
            }

            let mut state = WizardState::SelectPlugin;

            loop {
                state = match state {
                    WizardState::SelectPlugin => {
                        // Add Cancel option
                        let mut items_with_cancel = plugin_names.clone();
                        items_with_cancel.push("✗ Cancel".to_string());

                        let selection = Select::with_theme(&theme)
                            .with_prompt("Select a plugin")
                            .items(&items_with_cancel)
                            .default(0)
                            .interact()?;

                        if selection == items_with_cancel.len() - 1 {
                            // User selected Cancel
                            if color_state.is_enabled() {
                                println!("✗ Cancelled");
                            } else {
                                println!("✗ Cancelled");
                            }
                            return Ok(());
                        }

                        WizardState::SelectAction(plugin_names[selection].clone())
                    }
                    WizardState::SelectAction(ref selected_plugin) => {
                        // Query actions
                        let actions = match plugin_manager.get_plugin_actions(selected_plugin) {
                            Ok(acts) => acts,
                            Err(e) => {
                                if color_state.is_enabled() {
                                    println!(
                                        "✗ Failed to get actions for plugin '{}': {}",
                                        selected_plugin.red(),
                                        e
                                    );
                                } else {
                                    println!(
                                        "✗ Failed to get actions for plugin '{}': {}",
                                        selected_plugin, e
                                    );
                                }
                                return Ok(());
                            }
                        };

                        if actions.is_empty() {
                            if color_state.is_enabled() {
                                println!(
                                    "✗ No actions available for plugin '{}'",
                                    selected_plugin.red()
                                );
                            } else {
                                println!("✗ No actions available for plugin '{}'", selected_plugin);
                            }
                            return Ok(());
                        }

                        let mut action_items: Vec<String> = actions
                            .iter()
                            .map(|a| format!("{} - {}", a.name, a.description))
                            .collect();
                        action_items.push("← Go Back".to_string());

                        let selection = Select::with_theme(&theme)
                            .with_prompt("Select an action")
                            .items(&action_items)
                            .default(0)
                            .interact()?;

                        if selection == action_items.len() - 1 {
                            // Go back to plugin selection
                            WizardState::SelectPlugin
                        } else {
                            WizardState::EnterName(
                                selected_plugin.clone(),
                                actions[selection].name.clone(),
                            )
                        }
                    }
                    WizardState::EnterName(ref plugin, ref action) => {
                        // First ask if they want to continue or go back
                        let choice = Select::with_theme(&theme)
                            .with_prompt("Enter shortcut name")
                            .items(&["Continue", "← Go Back"])
                            .default(0)
                            .interact()?;

                        if choice == 1 {
                            // Go back to action selection
                            WizardState::SelectAction(plugin.clone())
                        } else {
                            let shortcut_name: String = Input::with_theme(&theme)
                                .with_prompt("Shortcut name")
                                .interact_text()?;

                            if shortcut_name.is_empty() {
                                if color_state.is_enabled() {
                                    println!("✗ Shortcut name cannot be empty\n");
                                } else {
                                    println!("✗ Shortcut name cannot be empty\n");
                                }
                                WizardState::EnterName(plugin.clone(), action.clone())
                            } else if config.get_shortcut(&shortcut_name).is_some() {
                                if color_state.is_enabled() {
                                    println!(
                                        "✗ Shortcut '{}' already exists. Try a different name.\n",
                                        shortcut_name.red()
                                    );
                                } else {
                                    println!(
                                        "✗ Shortcut '{}' already exists. Try a different name.\n",
                                        shortcut_name
                                    );
                                }
                                WizardState::EnterName(plugin.clone(), action.clone())
                            } else {
                                WizardState::EnterDescription(
                                    plugin.clone(),
                                    action.clone(),
                                    shortcut_name,
                                )
                            }
                        }
                    }
                    WizardState::EnterDescription(ref plugin, ref action, ref name) => {
                        // Get the action info for default description
                        let actions = plugin_manager.get_plugin_actions(plugin)?;
                        let selected_action = actions.iter().find(|a| &a.name == action).unwrap();

                        // First ask if they want to continue or go back
                        let choice = Select::with_theme(&theme)
                            .with_prompt("Enter description (optional)")
                            .items(&["Continue", "← Go Back"])
                            .default(0)
                            .interact()?;

                        if choice == 1 {
                            // Go back to name entry
                            WizardState::EnterName(plugin.clone(), action.clone())
                        } else {
                            let description: String = Input::with_theme(&theme)
                                .with_prompt("Description (press Enter to use action description)")
                                .allow_empty(true)
                                .interact_text()?;

                            let final_description = if description.is_empty() {
                                Some(selected_action.description.clone())
                            } else {
                                Some(description)
                            };

                            // Create shortcut
                            let shortcut_cmd = ShortcutCommand {
                                plugin_name: plugin.clone(),
                                action: action.clone(),
                                description: final_description.clone(),
                            };

                            config.add_shortcut(name.clone(), shortcut_cmd)?;

                            if color_state.is_enabled() {
                                println!(
                                    "\n✓ Created shortcut '{}' → {} {}",
                                    name.green(),
                                    plugin.cyan(),
                                    action.cyan()
                                );
                            } else {
                                println!("\n✓ Created shortcut '{}' → {} {}", name, plugin, action);
                            }
                            if let Some(desc) = &final_description {
                                println!("  Description: {}", desc);
                            }
                            println!("\n  Usage: {} {}", "lla".cyan(), name.green());

                            WizardState::Done
                        }
                    }
                    WizardState::Done => break,
                };
            }

            Ok(())
        }
        ShortcutAction::Export(output_path) => {
            // Create export structure
            #[derive(serde::Serialize)]
            struct ShortcutExport {
                shortcuts: std::collections::HashMap<String, ShortcutCommand>,
                plugin_aliases: std::collections::HashMap<String, String>,
            }

            let export_data = ShortcutExport {
                shortcuts: config.shortcuts.clone(),
                plugin_aliases: config.plugin_aliases.clone(),
            };

            let toml_string = toml::to_string_pretty(&export_data)
                .map_err(|e| LlaError::Other(format!("Failed to serialize shortcuts: {}", e)))?;

            match output_path {
                Some(path) => {
                    // Write to file
                    fs::write(path, toml_string)?;
                    if color_state.is_enabled() {
                        println!(
                            "✓ Exported {} shortcuts to {}",
                            config.shortcuts.len().to_string().green(),
                            path.cyan()
                        );
                    } else {
                        println!(
                            "✓ Exported {} shortcuts to {}",
                            config.shortcuts.len(),
                            path
                        );
                    }
                }
                None => {
                    // Print to stdout
                    println!("{}", toml_string);
                }
            }
            Ok(())
        }
        ShortcutAction::Import(file_path, merge) => {
            // Read and parse file
            let content = fs::read_to_string(&file_path).map_err(|e| {
                LlaError::Other(format!("Failed to read file '{}': {}", file_path, e))
            })?;

            #[derive(serde::Deserialize)]
            struct ShortcutImport {
                #[serde(default)]
                shortcuts: std::collections::HashMap<String, ShortcutCommand>,
                #[serde(default)]
                plugin_aliases: std::collections::HashMap<String, String>,
            }

            let import_data: ShortcutImport = toml::from_str(&content)
                .map_err(|e| LlaError::Other(format!("Failed to parse TOML file: {}", e)))?;

            if *merge {
                // Merge mode: add new shortcuts, skip conflicts
                let mut added = 0;
                let mut skipped = 0;

                for (name, cmd) in import_data.shortcuts {
                    if config.get_shortcut(&name).is_some() {
                        skipped += 1;
                    } else {
                        config.add_shortcut(name, cmd)?;
                        added += 1;
                    }
                }

                // Merge plugin aliases
                for (alias, plugin) in import_data.plugin_aliases {
                    if !config.plugin_aliases.contains_key(&alias) {
                        config.plugin_aliases.insert(alias, plugin);
                    }
                }
                config.save(&Config::get_config_path())?;

                if color_state.is_enabled() {
                    println!(
                        "✓ Imported {} shortcuts, skipped {} conflicts",
                        added.to_string().green(),
                        skipped.to_string().yellow()
                    );
                } else {
                    println!(
                        "✓ Imported {} shortcuts, skipped {} conflicts",
                        added, skipped
                    );
                }
            } else {
                // Replace mode: replace all shortcuts
                config.shortcuts = import_data.shortcuts;
                config.plugin_aliases = import_data.plugin_aliases;
                config.save(&Config::get_config_path())?;

                if color_state.is_enabled() {
                    println!(
                        "✓ Imported {} shortcuts (replaced existing)",
                        config.shortcuts.len().to_string().green()
                    );
                } else {
                    println!(
                        "✓ Imported {} shortcuts (replaced existing)",
                        config.shortcuts.len()
                    );
                }
            }
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
        InstallSource::Prebuilt => installer.install_from_prebuilt(),
        InstallSource::GitHub(url) => installer.install_from_git(url),
        InstallSource::LocalDir(dir) => installer.install_from_directory(dir),
    }
}
