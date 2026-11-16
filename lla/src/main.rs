mod commands;
mod config;
mod error;
mod filter;
mod formatter;
mod installer;
mod lister;
#[cfg(feature = "dynamic-plugins")]
mod plugin;
#[cfg(not(feature = "dynamic-plugins"))]
#[path = "plugin/static.rs"]
mod plugin;
mod sorter;
mod theme;
mod utils;

use commands::args::{Args, Command};
use commands::command_handler::handle_command;
use config::Config;
use error::{LlaError, Result};
use plugin::{PluginManager, DYNAMIC_PLUGINS_AVAILABLE, DYNAMIC_PLUGINS_UNAVAILABLE};
use std::collections::HashSet;
use utils::color::set_theme;

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        // A reader such as `head` closed the pipe; that is not an error.
        Err(LlaError::Io(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
        Err(e) => {
            print_error(&e);
            std::process::exit(1);
        }
    }
}

/// Lets listings die quietly on a closed pipe, like `ls`, instead of panicking
/// inside `println!`. Only used for plain listings: network-facing commands
/// keep Rust's default of ignoring SIGPIPE.
#[cfg(unix)]
fn restore_default_sigpipe() {
    // SAFETY: called before any threads are spawned; resetting a signal
    // disposition to its default has no other preconditions.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_default_sigpipe() {}

/// Returns `Ok(false)` when some, but not all, listed paths failed; those
/// errors have already been reported.
fn run() -> Result<bool> {
    let (mut config, config_error) = load_config()?;

    set_theme(config.get_theme());

    let mut args = Args::parse(&config)?;
    theme::set_no_color(args.no_color);
    if args.command.is_none() {
        restore_default_sigpipe();
    }

    if let Some(Command::Clean) = args.command {
        if !DYNAMIC_PLUGINS_AVAILABLE {
            return Err(LlaError::Plugin(DYNAMIC_PLUGINS_UNAVAILABLE.to_string()));
        }
        let mut plugin_config = config.clone();
        plugin_config.plugins_dir.clone_from(&args.plugins_dir);
        let mut plugin_manager = PluginManager::new(plugin_config);
        return plugin_manager
            .clean_plugins(&args.plugins_dir)
            .map(|_| true);
    }

    let mut plugin_manager = initialize_plugin_manager(&args, &config)?;
    if args.command.is_some() || args.directories.len() < 2 {
        return handle_command(&args, &mut config, &mut plugin_manager, config_error)
            .map(|_| true)
            .map_err(|error| with_path(error, &args));
    }

    // Several paths: list each in turn like `ls`, report failures per path and
    // keep going.
    let directories = std::mem::take(&mut args.directories);
    let mut config_error = config_error;
    let mut all_ok = true;
    let mut printed_any = false;
    for directory in directories {
        let is_dir = std::path::Path::new(&directory).is_dir();
        args.directory = directory;
        if is_dir && args.search.is_none() {
            use std::io::Write;
            let mut stdout = std::io::stdout().lock();
            if printed_any {
                writeln!(stdout)?;
            }
            writeln!(stdout, "{}:", args.directory)?;
        }
        match handle_command(&args, &mut config, &mut plugin_manager, config_error.take()) {
            Ok(()) => printed_any = true,
            Err(LlaError::Io(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                return Err(LlaError::Io(e));
            }
            Err(error) => {
                print_error(&with_path(error, &args));
                all_ok = false;
            }
        }
    }
    Ok(all_ok)
}

/// Names the path in filesystem errors from a listing, e.g.
/// "/missing: No such file or directory (os error 2)".
fn with_path(error: LlaError, args: &Args) -> LlaError {
    match error {
        LlaError::Io(e) if args.command.is_none() && e.kind() != std::io::ErrorKind::BrokenPipe => {
            LlaError::Io(std::io::Error::new(
                e.kind(),
                format!("{}: {}", args.directory, e),
            ))
        }
        other => other,
    }
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
    let mut plugin_config = config.clone();
    plugin_config.plugins_dir.clone_from(&args.plugins_dir);
    let mut plugin_manager = PluginManager::new(plugin_config);
    let plugin_paths = config.plugin_search_paths(Some(&args.plugins_dir));
    match &args.command {
        Some(Command::ListPlugins | Command::Use) => {
            plugin_manager.discover_plugin_paths(&plugin_paths)?;
        }
        Some(
            Command::PluginAction(name, _, _)
            | Command::PluginRun(name, _, _, _)
            | Command::PluginInfo(name)
            | Command::PluginPermissions(name),
        ) => {
            let names = HashSet::from([config.resolve_plugin_alias(name)]);
            plugin_manager.discover_plugin_paths_named(&plugin_paths, &names)?;
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
            plugin_manager.discover_plugin_paths_named(&plugin_paths, &names)?;
        }
        _ => {}
    }
    Ok(plugin_manager)
}
