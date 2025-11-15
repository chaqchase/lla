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
use error::Result;
use plugin::PluginManager;
use utils::color::set_theme;

fn main() -> Result<()> {
    let (mut config, config_error) = load_config()?;

    set_theme(config.get_theme());

    let args = Args::parse(&config);
    theme::set_no_color(args.no_color);

    if let Some(Command::Clean) = args.command {
        println!("🔄 Starting plugin cleaning...");
        let mut plugin_manager = PluginManager::new(config.clone());
        return plugin_manager.clean_plugins();
    }

    let mut plugin_manager = initialize_plugin_manager(&args, &config)?;
    handle_command(&args, &mut config, &mut plugin_manager, config_error)
}

fn load_config() -> Result<(Config, Option<error::LlaError>)> {
    let (layers, config_error) = config::load_config_layers(None)?;
    Ok((layers.effective, config_error))
}

fn initialize_plugin_manager(args: &Args, config: &Config) -> Result<PluginManager> {
    let mut plugin_manager = PluginManager::new(config.clone());
    plugin_manager.discover_plugins(&args.plugins_dir)?;
    Ok(plugin_manager)
}
