mod config;
mod executor;
mod model;
mod paths;
mod planner;
mod scanner;

use colored::Colorize;
use config::{FolderCleanerConfig, ProfileConfig};
use dialoguer::{Confirm, Input, MultiSelect, Select};
use executor::{
    empty_old_quarantine, execute_plan, list_runs, load_plan, quarantine_items, restore_run,
    save_plan,
};
use lazy_static::lazy_static;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use model::{CleanupPlan, OperationKind, PlanAction, ScanReport};
use parking_lot::RwLock;
use planner::build_plan;
use scanner::{options_from_config, scan_directory};
use std::{
    collections::{BTreeMap, HashSet},
    ops::Deref,
    path::{Path, PathBuf},
};

lazy_static! {
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "scan",
            "scan [directory]",
            "Analyze folder clutter and print a summary without saving a plan",
            vec!["lla plugin --name folder_cleaner --action scan --args ~/Downloads"],
            |args| FolderCleanerPlugin::scan_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "preview",
            "preview [directory] [profile]",
            "Show proposed organization and cleanup actions, then save the plan",
            vec![
                "lla plugin --name folder_cleaner --action preview --args ~/Downloads",
                "lla plugin --name folder_cleaner --action preview --args ~/Downloads project"
            ],
            |args| FolderCleanerPlugin::preview_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "clean",
            "clean [directory] [profile]",
            "Preview, interactively approve, then execute selected safe actions",
            vec!["lla plugin --name folder_cleaner --action clean --args ~/Downloads downloads"],
            |args| FolderCleanerPlugin::clean_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "apply",
            "apply <plan_id>",
            "Apply a saved plan after validation and confirmation",
            vec!["lla plugin --name folder_cleaner --action apply --args plan-20260508090000000"],
            |args| FolderCleanerPlugin::apply_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "restore",
            "restore [run_id]",
            "Restore files moved by a previous run",
            vec!["lla plugin --name folder_cleaner --action restore --args run-20260508090000000"],
            |args| FolderCleanerPlugin::restore_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "quarantine-list",
            "quarantine-list",
            "List files currently held in quarantine",
            vec!["lla plugin --name folder_cleaner --action quarantine-list"],
            |_| FolderCleanerPlugin::quarantine_list_action()
        );

        lla_plugin_utils::define_action!(
            registry,
            "quarantine-empty",
            "quarantine-empty [older_than_days]",
            "Permanently remove quarantined files older than the given number of days",
            vec!["lla plugin --name folder_cleaner --action quarantine-empty --args 30"],
            |args| FolderCleanerPlugin::quarantine_empty_action(args)
        );

        lla_plugin_utils::define_action!(
            registry,
            "config-wizard",
            "config-wizard",
            "Interactively adjust common folder cleaner settings",
            vec!["lla plugin --name folder_cleaner --action config-wizard"],
            |_| FolderCleanerPlugin::config_wizard_action()
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name folder_cleaner --action help"],
            |_| FolderCleanerPlugin::help_action()
        );

        registry
    });
}

pub struct FolderCleanerPlugin {
    base: BasePlugin<FolderCleanerConfig>,
}

impl FolderCleanerPlugin {
    pub fn new() -> Self {
        let plugin = Self {
            base: BasePlugin::with_name(env!("CARGO_PKG_NAME")),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[FolderCleanerPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn scan_action(args: &[String]) -> Result<(), String> {
        let plugin = Self::new();
        let (dir, profile_name) = Self::parse_dir_and_profile(args)?;
        let profile = plugin.config().profile(Some(&profile_name));
        let report = plugin.scan(&dir, &profile)?;
        Self::render_scan_summary(&report);
        Ok(())
    }

    fn preview_action(args: &[String]) -> Result<(), String> {
        let plugin = Self::new();
        let plan = plugin.plan_from_args(args)?;
        let saved = save_plan(&plan)?;
        Self::render_plan(&plan);
        println!(
            "{} Saved plan {} at {}",
            "Info:".bright_blue(),
            plan.id.bright_white(),
            saved.display().to_string().bright_cyan()
        );
        Ok(())
    }

    fn clean_action(args: &[String]) -> Result<(), String> {
        let plugin = Self::new();
        let plan = plugin.plan_from_args(args)?;
        save_plan(&plan)?;

        if plan.actions.is_empty() {
            println!(
                "{}",
                "Info: no organization or cleanup actions found".bright_blue()
            );
            return Ok(());
        }

        Self::render_plan(&plan);
        let selected = Self::select_actions(&plan)?;
        if selected.is_empty() {
            println!("{}", "Info: no actions selected".bright_blue());
            return Ok(());
        }

        if plugin.config().safety.require_confirmation
            && !Self::confirm(&format!("Apply {} selected actions?", selected.len()))?
        {
            println!("{}", "Info: clean operation cancelled".bright_blue());
            return Ok(());
        }

        let manifest = execute_plan(&plan, Some(&selected))?;
        println!(
            "{} Run {} completed with {} actions",
            "Success:".bright_green(),
            manifest.id.bright_white(),
            manifest.actions.len().to_string().bright_white()
        );
        Ok(())
    }

    fn apply_action(args: &[String]) -> Result<(), String> {
        let plan_id = args
            .first()
            .ok_or_else(|| "Usage: apply <plan_id>".to_string())?;
        let plan = load_plan(plan_id)?;
        if plan.actions.is_empty() {
            println!("{}", "Info: saved plan has no actions".bright_blue());
            return Ok(());
        }

        Self::render_plan(&plan);
        if !Self::confirm(&format!("Apply saved plan {}?", plan.id))? {
            println!("{}", "Info: apply operation cancelled".bright_blue());
            return Ok(());
        }

        let manifest = execute_plan(&plan, None)?;
        println!(
            "{} Applied plan {} as run {}",
            "Success:".bright_green(),
            plan.id.bright_white(),
            manifest.id.bright_white()
        );
        Ok(())
    }

    fn restore_action(args: &[String]) -> Result<(), String> {
        let run_id = match args.first() {
            Some(run_id) => run_id.clone(),
            None => Self::select_run_id()?,
        };
        if !Self::confirm(&format!("Restore files moved by run {}?", run_id))? {
            println!("{}", "Info: restore cancelled".bright_blue());
            return Ok(());
        }

        let (_manifest, restored) = restore_run(&run_id)?;
        println!(
            "{} Restored {} items from run {}",
            "Success:".bright_green(),
            restored.to_string().bright_white(),
            run_id.as_str().bright_white()
        );
        Ok(())
    }

    fn quarantine_list_action() -> Result<(), String> {
        let items = quarantine_items()?;
        if items.is_empty() {
            println!("{}", "Info: quarantine is empty".bright_blue());
            return Ok(());
        }

        println!("{}", "Quarantined items".bright_cyan().bold());
        for item in items {
            println!(
                "  {} {} -> {} ({})",
                "-".bright_black(),
                item.source.display().to_string().bright_yellow(),
                item.target.display().to_string().bright_cyan(),
                item.reason
            );
        }
        Ok(())
    }

    fn quarantine_empty_action(args: &[String]) -> Result<(), String> {
        let days = args
            .first()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(30);

        if !Self::confirm(&format!(
            "Permanently remove quarantined files older than {} days?",
            days
        ))? {
            println!("{}", "Info: quarantine-empty cancelled".bright_blue());
            return Ok(());
        }

        let removed = empty_old_quarantine(days)?;
        println!(
            "{} Removed {} quarantined items",
            "Success:".bright_green(),
            removed.to_string().bright_white()
        );
        Ok(())
    }

    fn config_wizard_action() -> Result<(), String> {
        let mut plugin = Self::new();
        let theme = LlaDialoguerTheme::default();
        let mut names = plugin.config().profiles.keys().cloned().collect::<Vec<_>>();
        names.sort();
        if names.is_empty() {
            return Err("No profiles configured".to_string());
        }

        let selected = Select::with_theme(&theme)
            .with_prompt("Select profile to edit")
            .items(&names)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show profile selector: {}", e))?;
        let profile_name = names[selected].clone();
        let current = plugin
            .config()
            .profiles
            .get(&profile_name)
            .cloned()
            .unwrap_or_default();

        let organize = Confirm::with_theme(&theme)
            .with_prompt("Organize files into unified folders?")
            .default(current.organize)
            .interact()
            .map_err(|e| format!("Failed to read organize setting: {}", e))?;
        let cleanup = Confirm::with_theme(&theme)
            .with_prompt("Detect conservative cleanup candidates?")
            .default(current.cleanup)
            .interact()
            .map_err(|e| format!("Failed to read cleanup setting: {}", e))?;
        let recursive = Confirm::with_theme(&theme)
            .with_prompt("Scan recursively?")
            .default(current.recursive.unwrap_or(plugin.config().scan.recursive))
            .interact()
            .map_err(|e| format!("Failed to read recursive setting: {}", e))?;
        let max_depth: usize = Input::with_theme(&theme)
            .with_prompt("Maximum scan depth")
            .default(current.max_depth.unwrap_or(plugin.config().scan.max_depth))
            .interact_text()
            .map_err(|e| format!("Failed to read max depth: {}", e))?;

        plugin.config_mut().profiles.insert(
            profile_name.clone(),
            ProfileConfig {
                organize,
                cleanup,
                recursive: Some(recursive),
                max_depth: Some(max_depth),
                include_hidden: current.include_hidden,
            },
        );
        plugin.base.save_config()?;
        println!(
            "{} Updated profile {}",
            "Success:".bright_green(),
            profile_name.bright_white()
        );
        Ok(())
    }

    fn help_action() -> Result<(), String> {
        let mut help = HelpFormatter::new("Folder Cleaner".to_string());
        help.add_section("Description").add_command(
            "",
            "Safety-first folder organizer and cleanup assistant with preview, quarantine, and restore support.",
            vec![],
        );
        help.add_section("Workflow")
            .add_command(
                "scan [directory]",
                "Analyze clutter without saving or moving anything",
                vec!["lla plugin --name folder_cleaner --action scan --args ~/Downloads".to_string()],
            )
            .add_command(
                "preview [directory] [profile]",
                "Save and display a proposed plan",
                vec!["lla plugin --name folder_cleaner --action preview --args ~/Downloads downloads".to_string()],
            )
            .add_command(
                "clean [directory] [profile]",
                "Interactively approve and apply selected actions",
                vec!["lla plugin --name folder_cleaner --action clean --args ~/Downloads".to_string()],
            )
            .add_command(
                "apply <plan_id>",
                "Apply a saved plan",
                vec!["lla plugin --name folder_cleaner --action apply --args plan-20260508090000000".to_string()],
            )
            .add_command(
                "restore <run_id>",
                "Restore files moved by a completed run",
                vec!["lla plugin --name folder_cleaner --action restore --args run-20260508090000000".to_string()],
            )
            .add_command(
                "quarantine-list",
                "Show files currently held in quarantine",
                vec!["lla plugin --name folder_cleaner --action quarantine-list".to_string()],
            )
            .add_command(
                "quarantine-empty [older_than_days]",
                "Permanently empty old quarantined files after confirmation",
                vec!["lla plugin --name folder_cleaner --action quarantine-empty --args 30".to_string()],
            )
            .add_command(
                "config-wizard",
                "Edit common profile options interactively",
                vec!["lla plugin --name folder_cleaner --action config-wizard".to_string()],
            );

        println!(
            "{}",
            BoxComponent::new(help.render(&FolderCleanerConfig::default().colors))
                .style(BoxStyle::Minimal)
                .padding(1)
                .render()
        );
        Ok(())
    }

    fn plan_from_args(&self, args: &[String]) -> Result<CleanupPlan, String> {
        let (dir, profile_name) = Self::parse_dir_and_profile(args)?;
        let profile = self.config().profile(Some(&profile_name));
        let report = self.scan(&dir, &profile)?;
        Ok(build_plan(&report, self.config(), &profile_name, &profile))
    }

    fn scan(&self, dir: &Path, profile: &ProfileConfig) -> Result<ScanReport, String> {
        let options = options_from_config(self.config(), profile);
        scan_directory(dir, self.config(), &options)
    }

    fn parse_dir_and_profile(args: &[String]) -> Result<(PathBuf, String), String> {
        let dir =
            args.first().map(expand_home).transpose()?.unwrap_or(
                std::env::current_dir().map_err(|e| format!("Failed to get cwd: {}", e))?,
            );
        let profile = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| "downloads".to_string());
        Ok((dir, profile))
    }

    fn render_scan_summary(report: &ScanReport) {
        let file_count = report.entries.iter().filter(|entry| entry.is_file).count();
        let dir_count = report.entries.iter().filter(|entry| entry.is_dir).count();
        let total_size: u64 = report
            .entries
            .iter()
            .filter(|entry| entry.is_file)
            .map(|entry| entry.size)
            .sum();

        println!("{}", "Folder Cleaner Scan".bright_cyan().bold());
        println!(
            "Directory: {}",
            report.root.display().to_string().bright_white()
        );
        println!("Files: {}", file_count.to_string().bright_white());
        println!("Directories: {}", dir_count.to_string().bright_white());
        println!("Ignored: {}", report.ignored.to_string().bright_white());
        println!(
            "Total file size: {}",
            human_bytes(total_size).bright_white()
        );
    }

    fn render_plan(plan: &CleanupPlan) {
        println!("{}", "Folder Cleaner Preview".bright_cyan().bold());
        println!("Plan: {}", plan.id.bright_white());
        println!(
            "Directory: {}",
            plan.root.display().to_string().bright_white()
        );
        println!("Profile: {}", plan.profile.bright_white());
        println!("Actions: {}", plan.actions.len().to_string().bright_white());

        let mut by_kind: BTreeMap<&str, Vec<&PlanAction>> = BTreeMap::new();
        for action in &plan.actions {
            let key = match action.kind {
                OperationKind::Organize => "Organize",
                OperationKind::Quarantine => "Quarantine",
            };
            by_kind.entry(key).or_default().push(action);
        }

        for (kind, actions) in by_kind {
            println!("\n{} {}", kind.bright_yellow(), actions.len());
            for action in actions.iter().take(20) {
                println!(
                    "  [{}] {} -> {} ({})",
                    action.id.to_string().bright_black(),
                    display_relative(&plan.root, &action.source).bright_white(),
                    display_relative(&plan.root, &action.target).bright_cyan(),
                    action.reason
                );
            }
            if actions.len() > 20 {
                println!("  ... {} more", actions.len() - 20);
            }
        }
    }

    fn select_actions(plan: &CleanupPlan) -> Result<HashSet<usize>, String> {
        let theme = LlaDialoguerTheme::default();
        let items = plan
            .actions
            .iter()
            .map(|action| {
                format!(
                    "[{}] {} -> {} ({})",
                    action.id,
                    display_relative(&plan.root, &action.source),
                    display_relative(&plan.root, &action.target),
                    action.reason
                )
            })
            .collect::<Vec<_>>();
        let defaults = vec![true; items.len()];
        let selections = MultiSelect::with_theme(&theme)
            .with_prompt("Select actions to apply")
            .items(&items)
            .defaults(&defaults)
            .interact()
            .map_err(|e| format!("Failed to show action selector: {}", e))?;

        Ok(selections
            .into_iter()
            .filter_map(|index| plan.actions.get(index).map(|action| action.id))
            .collect())
    }

    fn select_run_id() -> Result<String, String> {
        let runs = list_runs()?;
        if runs.is_empty() {
            return Err("No previous folder_cleaner runs found".to_string());
        }

        let items = runs
            .iter()
            .map(|run| {
                format!(
                    "{} - {} actions - {}",
                    run.id,
                    run.actions.len(),
                    run.root.display()
                )
            })
            .collect::<Vec<_>>();

        let selection = Select::with_theme(&LlaDialoguerTheme::default())
            .with_prompt("Select run to restore")
            .items(&items)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show run selector: {}", e))?;

        Ok(runs[selection].id.clone())
    }

    fn confirm(prompt: &str) -> Result<bool, String> {
        Confirm::with_theme(&LlaDialoguerTheme::default())
            .with_prompt(prompt)
            .default(false)
            .interact()
            .map_err(|e| format!("Failed to show confirmation: {}", e))
    }
}

fn expand_home(value: &String) -> Result<PathBuf, String> {
    if value == "~" {
        return dirs::home_dir().ok_or_else(|| "Failed to resolve home directory".to_string());
    }
    if let Some(stripped) = value.strip_prefix("~/") {
        return dirs::home_dir()
            .map(|home| home.join(stripped))
            .ok_or_else(|| "Failed to resolve home directory".to_string());
    }
    Ok(PathBuf::from(value))
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

impl Default for FolderCleanerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for FolderCleanerPlugin {
    type Target = FolderCleanerConfig;

    fn deref(&self) -> &Self::Target {
        self.base.config()
    }
}

impl ConfigurablePlugin for FolderCleanerPlugin {
    type Config = FolderCleanerConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl Plugin for FolderCleanerPlugin {
    fn handle_raw_request(&mut self, request: &[u8]) -> Vec<u8> {
        match self.decode_request(request) {
            Ok(request) => {
                let response = match request {
                    PluginRequest::GetName => {
                        PluginResponse::Name(env!("CARGO_PKG_NAME").to_string())
                    }
                    PluginRequest::GetVersion => {
                        PluginResponse::Version(env!("CARGO_PKG_VERSION").to_string())
                    }
                    PluginRequest::GetDescription => {
                        PluginResponse::Description(env!("CARGO_PKG_DESCRIPTION").to_string())
                    }
                    PluginRequest::GetSupportedFormats => {
                        PluginResponse::SupportedFormats(vec!["default".to_string()])
                    }
                    PluginRequest::Decorate(entry) => PluginResponse::Decorated(entry),
                    PluginRequest::FormatField(_, _) => PluginResponse::FormattedField(None),
                    PluginRequest::PerformAction(action, args) => {
                        let result = ACTION_REGISTRY.read().handle(&action, &args);
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::GetAvailableActions => {
                        PluginResponse::AvailableActions(ACTION_REGISTRY.read().list_actions())
                    }
                };
                self.encode_response(response)
            }
            Err(e) => self.encode_error(&e),
        }
    }
}

impl ProtobufHandler for FolderCleanerPlugin {}

lla_plugin_interface::declare_plugin!(FolderCleanerPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
    }
}
