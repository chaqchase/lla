mod config;
mod strategies;
use colored::Colorize;
use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, response, ActionArguments, Plugin};
use lla_plugin_utils::{
    action_arguments_as_strings, action_infos,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter},
    ActionRegistry, BasePlugin, ConfigurablePlugin,
};
use parking_lot::RwLock;
use std::{ops::Deref, path::PathBuf};
use strategies::{
    DateStrategy, ExtensionStrategy, OrganizationStrategy, SizeStrategy, TypeStrategy,
};

use crate::config::OrganizerConfig;

lazy_static! {
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "organize",
            "organize <directory> [strategy]",
            "Organize files in the specified directory using the given strategy (defaults to extension)",
            ["lla plugin --name file_organizer --action organize --args /path/to/dir",
                "lla plugin --name file_organizer --action organize --args /path/to/dir extension",
                "lla plugin --name file_organizer --action organize --args /path/to/dir date",
                "lla plugin --name file_organizer --action organize --args /path/to/dir type",
                "lla plugin --name file_organizer --action organize --args /path/to/dir size"],
            FileOrganizerPlugin::organize_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "preview",
            "preview <directory> [strategy]",
            "Preview organization changes without applying them",
            [
                "lla plugin --name file_organizer --action preview --args /path/to/dir",
                "lla plugin --name file_organizer --action preview --args /path/to/dir extension",
                "lla plugin --name file_organizer --action preview --args /path/to/dir date",
                "lla plugin --name file_organizer --action preview --args /path/to/dir type",
                "lla plugin --name file_organizer --action preview --args /path/to/dir size"
            ],
            FileOrganizerPlugin::preview_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            ["lla plugin --name file_organizer --action help"],
            |_| FileOrganizerPlugin::help_action()
        );

        registry
    });
}

pub struct FileOrganizerPlugin {
    base: BasePlugin<OrganizerConfig>,
}

impl FileOrganizerPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[FileOrganizerPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn get_strategy(&self, strategy_name: Option<&str>) -> Box<dyn OrganizationStrategy> {
        match strategy_name.unwrap_or("extension") {
            "extension" => Box::new(ExtensionStrategy::new(self.config().extension.clone())),
            "date" => Box::new(DateStrategy::new(self.config().date.clone())),
            "type" => Box::new(TypeStrategy::new(self.config().type_strategy.clone())),
            "size" => Box::new(SizeStrategy::new(self.config().size.clone())),
            _ => Box::new(ExtensionStrategy::new(self.config().extension.clone())),
        }
    }

    fn organize_action(args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Usage: organize <directory> [strategy]".to_string());
        }

        let plugin = Self::new();
        let dir = PathBuf::from(&args[0]);
        let strategy_name = args.get(1).map(|s| s.as_str());
        let strategy = plugin.get_strategy(strategy_name);

        let moves = strategy.organize(&dir, false)?;
        if moves.is_empty() {
            println!("{} No files to organize", "Info:".bright_blue());
            return Ok(());
        }

        strategy.execute_moves(moves)?;
        println!(
            "{} Successfully organized files in {}",
            "Success:".bright_green(),
            dir.display().to_string().bright_yellow()
        );
        Ok(())
    }

    fn preview_action(args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Usage: preview <directory> [strategy]".to_string());
        }

        let plugin = Self::new();
        let dir = PathBuf::from(&args[0]);
        let strategy_name = args.get(1).map(|s| s.as_str());
        let strategy = plugin.get_strategy(strategy_name);

        let moves = strategy.organize(&dir, true)?;
        if moves.is_empty() {
            println!("{} No files to organize", "Info:".bright_blue());
            return Ok(());
        }

        let mut moves_by_dir: std::collections::HashMap<PathBuf, Vec<(PathBuf, PathBuf)>> =
            std::collections::HashMap::new();

        for (source, target) in moves {
            let parent = target.parent().unwrap_or(&target).to_path_buf();
            moves_by_dir
                .entry(parent)
                .or_default()
                .push((source, target));
        }

        let total_files: usize = moves_by_dir.values().map(|v| v.len()).sum();
        let total_dirs = moves_by_dir.len();

        println!("\n{}", "📦 File Organization Preview".bright_cyan().bold());
        println!("{}", "═".bright_black().repeat(50));

        println!(
            "{} {}",
            "Directory:".bright_yellow(),
            dir.display().to_string().bright_white()
        );
        println!(
            "{} {}",
            "Strategy:".bright_yellow(),
            strategy_name.unwrap_or("extension").bright_white()
        );
        println!("{}", "═".bright_black().repeat(50));

        for (target_dir, moves) in &moves_by_dir {
            let relative_dir = target_dir.strip_prefix(&dir).unwrap_or(target_dir);
            println!(
                "\n{} {}",
                "📁".bright_blue(),
                relative_dir.display().to_string().bright_cyan()
            );

            for (source, _) in moves {
                let file_name = source.file_name().unwrap_or_default().to_string_lossy();
                println!("  {} {}", "→".bright_green(), file_name.bright_white());
            }
        }

        println!("\n{}", "═".bright_black().repeat(50));
        println!(
            "{} {} files will be organized into {} directories",
            "Summary:".bright_yellow(),
            total_files.to_string().bright_white(),
            total_dirs.to_string().bright_white()
        );
        println!("{}", "═".bright_black().repeat(50));

        Ok(())
    }

    fn help_action() -> Result<(), String> {
        let mut help = HelpFormatter::new("File Organizer".to_string());
        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Organize files in directories using various strategies".to_string(),
            vec![],
        );

        help.add_section("Basic Commands".to_string())
            .add_command(
                "organize".to_string(),
                "Organize files in the specified directory".to_string(),
                vec![
                    "lla plugin --name file_organizer --action organize /path/to/dir".to_string(),
                    "lla plugin --name file_organizer --action organize /path/to/dir extension"
                        .to_string(),
                    "lla plugin --name file_organizer --action organize /path/to/dir date"
                        .to_string(),
                    "lla plugin --name file_organizer --action organize /path/to/dir type"
                        .to_string(),
                    "lla plugin --name file_organizer --action organize /path/to/dir size"
                        .to_string(),
                ],
            )
            .add_command(
                "preview".to_string(),
                "Preview organization changes".to_string(),
                vec![
                    "lla plugin --name file_organizer --action preview /path/to/dir".to_string(),
                    "lla plugin --name file_organizer --action preview /path/to/dir extension"
                        .to_string(),
                    "lla plugin --name file_organizer --action preview /path/to/dir date"
                        .to_string(),
                    "lla plugin --name file_organizer --action preview /path/to/dir type"
                        .to_string(),
                    "lla plugin --name file_organizer --action preview /path/to/dir size"
                        .to_string(),
                ],
            );

        println!(
            "{}",
            BoxComponent::new(help.render(&OrganizerConfig::default().colors))
                .style(BoxStyle::Minimal)
                .padding(1)
                .render()
        );
        Ok(())
    }
}

impl Default for FileOrganizerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for FileOrganizerPlugin {
    type Target = OrganizerConfig;

    fn deref(&self) -> &Self::Target {
        self.base.config()
    }
}

impl Plugin for FileOrganizerPlugin {
    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        let arguments = action_arguments_as_strings(arguments);
        response::from_result(ACTION_REGISTRY.read().handle(&action, &arguments))
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        action_infos(ACTION_REGISTRY.read().list_actions())
    }
}

impl ConfigurablePlugin for FileOrganizerPlugin {
    type Config = OrganizerConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

lla_plugin_sdk::export_plugin!(FileOrganizerPlugin);
