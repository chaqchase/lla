mod commands;
mod config;
mod error;
mod filter;
mod formatter;
mod installer;
mod lister;
mod plugin;
mod sorter;
mod theme;
mod utils;

use commands::args::{Args, Command};
use commands::command_handler::handle_command;
use config::Config;
use error::{LlaError, Result};
use plugin::PluginManager;
use std::collections::HashSet;
use utils::color::set_theme;

fn main() {
    if let Err(e) = run() {
        print_error(&e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let (mut config, config_error) = load_config()?;

    set_theme(config.get_theme());

    let args = Args::parse(&config)?;
    theme::set_no_color(args.no_color);

    if let Some(Command::Clean) = args.command {
        println!("🔄 Starting plugin cleaning...");
        let mut plugin_manager = PluginManager::new(config.clone());
        return plugin_manager.clean_plugins();
    }

    let mut plugin_manager = initialize_plugin_manager(&args, &config)?;
    handle_command(&args, &mut config, &mut plugin_manager, config_error)
}

fn print_error(error: &LlaError) {
    use colored::Colorize;

    let error_type = match error {
        LlaError::Io(_) => "IO Error",
        LlaError::Parse(_) => "Parse Error",
        LlaError::Config(_) => "Config Error",
        LlaError::Plugin(_) => "Plugin Error",
        LlaError::Filter(_) => "Filter Error",
        LlaError::Other(_) => "Error",
    };

    eprintln!();
    eprintln!("{} {}", "✗".bright_red(), error_type.bright_red().bold());
    eprintln!();

    // Print the error message with proper indentation for multiline messages
    let message = error.to_string();
    for line in message.lines() {
        eprintln!("  {}", line);
    }
    eprintln!();
}

fn load_config() -> Result<(Config, Option<error::LlaError>)> {
    let (layers, config_error) = config::load_config_layers(None)?;
    Ok((layers.effective, config_error))
}

fn initialize_plugin_manager(args: &Args, config: &Config) -> Result<PluginManager> {
    let mut plugin_manager = PluginManager::new(config.clone());
    match &args.command {
        Some(Command::ListPlugins | Command::Use) => {
            plugin_manager.discover_plugins(&args.plugins_dir)?;
        }
        Some(Command::PluginAction(name, _, _)) => {
            let names = HashSet::from([config.resolve_plugin_alias(name)]);
            plugin_manager.discover_plugins_named(&args.plugins_dir, &names)?;
        }
        None => {
            let mut names: HashSet<_> = config.enabled_plugins.iter().cloned().collect();
            names.extend(args.enable_plugin.iter().cloned());
            names.extend(args.disable_plugin.iter().cloned());
            names.extend(
                args.search_pipelines
                    .iter()
                    .map(|pipeline| pipeline.plugin.clone()),
            );
            plugin_manager.discover_plugins_named(&args.plugins_dir, &names)?;
        }
        _ => {}
    }
    Ok(plugin_manager)
}
