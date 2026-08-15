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
    empty_old_quarantine, execute_plan, inspect_runs, list_plans, list_runs, load_plan,
    orphaned_quarantine_items, quarantine_items, restore_run, save_plan,
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
    fs,
    ops::Deref,
    path::{Path, PathBuf},
};

const LARGE_ACTION_SELECTOR_THRESHOLD: usize = 80;

enum ActionSelectionMode {
    All,
    Kind(OperationKind),
    Individual,
    Cancel,
}

lazy_static! {
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "scan",
            "scan [directory]",
            "Analyze folder clutter and print a summary without saving a plan",
            ["lla plugin --name folder_cleaner --action scan --args ~/Downloads"],
            FolderCleanerPlugin::scan_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "preview",
            "preview [directory] [profile]",
            "Show proposed organization and cleanup actions, then save the plan",
            [
                "lla plugin --name folder_cleaner --action preview --args ~/Downloads",
                "lla plugin --name folder_cleaner --action preview --args ~/Downloads project"
            ],
            FolderCleanerPlugin::preview_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "clean",
            "clean [directory] [profile]",
            "Preview, interactively approve, then execute selected safe actions",
            ["lla plugin --name folder_cleaner --action clean --args ~/Downloads downloads"],
            FolderCleanerPlugin::clean_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "apply",
            "apply <plan_id>",
            "Apply a saved plan after validation and confirmation",
            ["lla plugin --name folder_cleaner --action apply --args plan-20260508090000000"],
            FolderCleanerPlugin::apply_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "restore",
            "restore [run_id]",
            "Restore files moved by a previous run",
            ["lla plugin --name folder_cleaner --action restore --args run-20260508090000000"],
            FolderCleanerPlugin::restore_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "quarantine-list",
            "quarantine-list",
            "List files currently held in quarantine",
            ["lla plugin --name folder_cleaner --action quarantine-list"],
            |_| FolderCleanerPlugin::quarantine_list_action()
        );

        lla_plugin_utils::define_action!(
            registry,
            "quarantine-empty",
            "quarantine-empty [older_than_days]",
            "Permanently remove quarantined files older than the given number of days",
            ["lla plugin --name folder_cleaner --action quarantine-empty --args 30"],
            FolderCleanerPlugin::quarantine_empty_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "history",
            "history",
            "List saved plans and completed or partial runs",
            ["lla plugin --name folder_cleaner --action history"],
            |_| FolderCleanerPlugin::history_action()
        );

        lla_plugin_utils::define_action!(
            registry,
            "show-plan",
            "show-plan <plan_id>",
            "Render a saved plan preview",
            ["lla plugin --name folder_cleaner --action show-plan --args plan-20260508090000000"],
            FolderCleanerPlugin::show_plan_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "doctor",
            "doctor [run_id] [--repair]",
            "Inspect run manifests and recover restorable items when requested",
            ["lla plugin --name folder_cleaner --action doctor",
                "lla plugin --name folder_cleaner --action doctor --args run-20260508090000000 --repair"],
            FolderCleanerPlugin::doctor_action
        );

        lla_plugin_utils::define_action!(
            registry,
            "config-wizard",
            "config-wizard",
            "Interactively adjust common folder cleaner settings",
            ["lla plugin --name folder_cleaner --action config-wizard"],
            |_| FolderCleanerPlugin::config_wizard_action()
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            ["lla plugin --name folder_cleaner --action help"],
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
        let mut plugin = Self {
            base: BasePlugin::with_name(env!("CARGO_PKG_NAME")),
        };
        plugin.base.config_mut().merge_new_defaults();
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
        let plan_arg = args
            .first()
            .ok_or_else(|| "Usage: apply <plan_id>".to_string())?;
        let plan_id = Self::resolve_plan_id(plan_arg)?;
        let plan = load_plan(&plan_id)?;
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
        let run_arg = match args.first() {
            Some(run_id) => run_id.clone(),
            None => Self::select_run_id()?,
        };
        let run_id = Self::resolve_run_id(&run_arg)?;
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

    fn history_action() -> Result<(), String> {
        let plans = list_plans()?;
        let runs = list_runs()?;

        println!("{}", "Folder Cleaner History".bright_cyan().bold());

        println!("\n{}", "Runs".bright_yellow().bold());
        if runs.is_empty() {
            println!("  {}", "No runs found".bright_black());
        } else {
            for run in runs.iter().take(20) {
                let completed = run.actions.iter().filter(|action| action.completed).count();
                let restored = run.actions.iter().filter(|action| action.restored).count();
                let quarantined = run
                    .actions
                    .iter()
                    .filter(|action| action.operation == OperationKind::Quarantine)
                    .count();
                println!(
                    "  {}  {} actions  {} completed  {} restored  {} quarantined  {}",
                    run.id.bright_white(),
                    run.actions.len().to_string().bright_white(),
                    completed.to_string().bright_green(),
                    restored.to_string().bright_blue(),
                    quarantined.to_string().bright_yellow(),
                    run.root.display().to_string().bright_black()
                );
            }
            if runs.len() > 20 {
                println!("  ... {} more runs", runs.len() - 20);
            }
        }

        println!("\n{}", "Plans".bright_yellow().bold());
        if plans.is_empty() {
            println!("  {}", "No plans found".bright_black());
        } else {
            for plan in plans.iter().take(20) {
                println!(
                    "  {}  {} actions  {}  {}",
                    plan.id.bright_white(),
                    plan.actions.len().to_string().bright_white(),
                    plan.profile.bright_cyan(),
                    plan.root.display().to_string().bright_black()
                );
            }
            if plans.len() > 20 {
                println!("  ... {} more plans", plans.len() - 20);
            }
        }

        Ok(())
    }

    fn show_plan_action(args: &[String]) -> Result<(), String> {
        let plan_id = args
            .first()
            .ok_or_else(|| "Usage: show-plan <plan_id>".to_string())?;
        let plan = load_plan(plan_id)?;
        Self::render_plan(&plan);
        Ok(())
    }

    fn doctor_action(args: &[String]) -> Result<(), String> {
        let repair = args.iter().any(|arg| arg == "--repair");
        let run_id = args
            .iter()
            .find(|arg| arg.as_str() != "--repair")
            .map(String::as_str);
        if repair && run_id.is_none() {
            return Err("Usage: doctor <run_id> --repair".to_string());
        }
        let reports = inspect_runs(run_id)?;
        let orphans = orphaned_quarantine_items()?;

        println!("{}", "Folder Cleaner Doctor".bright_cyan().bold());
        if reports.is_empty() {
            println!("  {}", "No run manifests found".bright_black());
        }

        for report in &reports {
            let health = if report.is_healthy() {
                "healthy".bright_green()
            } else {
                "needs attention".bright_yellow()
            };
            println!(
                "\n{}  {}  {} actions  {} completed  {} restored  {} restorable",
                report.run_id.bright_white(),
                health,
                report.total_actions.to_string().bright_white(),
                report.completed_actions.to_string().bright_green(),
                report.restored_actions.to_string().bright_blue(),
                report.restorable_actions.to_string().bright_cyan()
            );

            if !report.pending_actions.is_empty() {
                println!("  {}", "Pending actions".bright_yellow());
                for action in report.pending_actions.iter().take(10) {
                    println!(
                        "    {} -> {} ({})",
                        action.source.display().to_string().bright_white(),
                        action.target.display().to_string().bright_black(),
                        action.reason
                    );
                }
            }

            if !report.missing_targets.is_empty() {
                println!("  {}", "Missing moved targets".bright_red());
                for action in report.missing_targets.iter().take(10) {
                    println!(
                        "    {} ({})",
                        action.target.display().to_string().bright_black(),
                        action.reason
                    );
                }
            }

            if repair && report.restorable_actions > 0 {
                let (_manifest, restored) = restore_run(&report.run_id)?;
                println!(
                    "  {} restored {} items",
                    "Repair:".bright_green(),
                    restored.to_string().bright_white()
                );
            }
        }

        if !orphans.is_empty() {
            println!(
                "\n{} {}",
                "Orphaned quarantine files:".bright_yellow(),
                orphans.len().to_string().bright_white()
            );
            for path in orphans.iter().take(20) {
                println!("  {}", path.display().to_string().bright_black());
            }
            if orphans.len() > 20 {
                println!("  ... {} more", orphans.len() - 20);
            }
        }

        if repair && !orphans.is_empty() {
            println!(
                "{} Orphaned quarantine files were left in place because their original paths are unknown.",
                "Info:".bright_blue()
            );
        }

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
                "history",
                "List saved plans and run manifests",
                vec!["lla plugin --name folder_cleaner --action history".to_string()],
            )
            .add_command(
                "show-plan <plan_id>",
                "Render a saved plan preview",
                vec!["lla plugin --name folder_cleaner --action show-plan --args plan-20260508090000000".to_string()],
            )
            .add_command(
                "doctor [run_id] [--repair]",
                "Inspect run health and optionally restore recoverable items",
                vec!["lla plugin --name folder_cleaner --action doctor --args run-20260508090000000".to_string()],
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
        let organize_count = plan
            .actions
            .iter()
            .filter(|action| action.kind == OperationKind::Organize)
            .count();
        let quarantine_count = plan
            .actions
            .iter()
            .filter(|action| action.kind == OperationKind::Quarantine)
            .count();
        let quarantine_bytes = plan
            .actions
            .iter()
            .filter(|action| action.kind == OperationKind::Quarantine)
            .map(action_size)
            .sum::<u64>();
        let duplicate_count = plan
            .actions
            .iter()
            .filter(|action| action.reason.contains("duplicate file"))
            .count();
        let empty_dir_count = plan
            .actions
            .iter()
            .filter(|action| action.reason == "empty directory")
            .count();

        println!("{}", "Folder Cleaner Preview".bright_cyan().bold());
        println!("Plan: {}", plan.id.bright_white());
        println!(
            "Directory: {}",
            plan.root.display().to_string().bright_white()
        );
        println!("Profile: {}", plan.profile.bright_white());
        println!("Actions: {}", plan.actions.len().to_string().bright_white());
        println!(
            "Summary: {} organize · {} quarantine · {} quarantine bytes · {} duplicates · {} empty dirs",
            organize_count.to_string().bright_green(),
            quarantine_count.to_string().bright_yellow(),
            human_bytes(quarantine_bytes).bright_white(),
            duplicate_count.to_string().bright_cyan(),
            empty_dir_count.to_string().bright_black()
        );

        let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_destination: BTreeMap<String, usize> = BTreeMap::new();
        for action in &plan.actions {
            let category = action.category.as_deref().unwrap_or("cleanup");
            *by_category.entry(category.to_string()).or_default() += 1;
            let destination = action
                .target
                .parent()
                .map(|path| display_relative(&plan.root, path))
                .unwrap_or_else(|| ".".to_string());
            *by_destination.entry(destination).or_default() += 1;
        }

        if !by_category.is_empty() {
            println!("\n{}", "Categories".bright_yellow());
            for (category, count) in by_category.iter().take(12) {
                println!("  {} {}", count.to_string().bright_white(), category);
            }
        }

        if !by_destination.is_empty() {
            println!("\n{}", "Destinations".bright_yellow());
            for (destination, count) in by_destination.iter().take(12) {
                println!(
                    "  {} {}",
                    count.to_string().bright_white(),
                    destination.bright_black()
                );
            }
        }

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
        if plan.actions.len() > LARGE_ACTION_SELECTOR_THRESHOLD {
            return Self::select_action_mode(plan);
        }

        Self::select_individual_actions(plan)
    }

    fn select_action_mode(plan: &CleanupPlan) -> Result<HashSet<usize>, String> {
        let organize_count = plan
            .actions
            .iter()
            .filter(|action| action.kind == OperationKind::Organize)
            .count();
        let quarantine_count = plan
            .actions
            .iter()
            .filter(|action| action.kind == OperationKind::Quarantine)
            .count();

        let mut items = vec![format!("Apply all actions ({})", plan.actions.len())];
        let mut modes = vec![ActionSelectionMode::All];

        if organize_count > 0 {
            items.push(format!("Apply organize actions only ({})", organize_count));
            modes.push(ActionSelectionMode::Kind(OperationKind::Organize));
        }
        if quarantine_count > 0 {
            items.push(format!(
                "Apply quarantine actions only ({})",
                quarantine_count
            ));
            modes.push(ActionSelectionMode::Kind(OperationKind::Quarantine));
        }

        items.push("Choose individual actions".to_string());
        modes.push(ActionSelectionMode::Individual);
        items.push("Cancel".to_string());
        modes.push(ActionSelectionMode::Cancel);

        let selection = Select::with_theme(&LlaDialoguerTheme::default())
            .with_prompt("Select action mode")
            .items(&items)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show action selector: {}", e))?;

        match &modes[selection] {
            ActionSelectionMode::All => Ok(Self::action_ids_for_kind(plan, None)),
            ActionSelectionMode::Kind(kind) => Ok(Self::action_ids_for_kind(plan, Some(kind))),
            ActionSelectionMode::Individual => Self::select_individual_actions(plan),
            ActionSelectionMode::Cancel => Ok(HashSet::new()),
        }
    }

    fn select_individual_actions(plan: &CleanupPlan) -> Result<HashSet<usize>, String> {
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

    fn action_ids_for_kind(plan: &CleanupPlan, kind: Option<&OperationKind>) -> HashSet<usize> {
        plan.actions
            .iter()
            .filter(|action| kind.map(|kind| &action.kind == kind).unwrap_or(true))
            .map(|action| action.id)
            .collect()
    }

    fn resolve_plan_id(input: &str) -> Result<String, String> {
        let plans = list_plans()?;
        if plans.iter().any(|plan| plan.id == input) {
            return Ok(input.to_string());
        }

        let runs = list_runs()?;
        if let Some(run) = runs.iter().find(|run| run.id == input) {
            println!(
                "{} {} is a run id; applying its saved plan {} instead",
                "Info:".bright_blue(),
                input.bright_white(),
                run.plan_id.bright_white()
            );
            return Ok(run.plan_id.clone());
        }

        if let Some(plan) = Self::matching_plan_for_input(input, &plans) {
            println!(
                "{} {} looks like a run id made from a plan timestamp; applying {}",
                "Info:".bright_blue(),
                input.bright_white(),
                plan.id.bright_white()
            );
            return Ok(plan.id.clone());
        }

        Err(format!(
            "Plan '{}' was not found.{}",
            input,
            Self::latest_plan_hint(&plans)
        ))
    }

    fn resolve_run_id(input: &str) -> Result<String, String> {
        let runs = list_runs()?;
        if runs.iter().any(|run| run.id == input) {
            return Ok(input.to_string());
        }

        let plans = list_plans()?;
        if let Some(plan) = Self::matching_plan_for_input(input, &plans) {
            let matching_runs = runs
                .iter()
                .filter(|run| run.plan_id == plan.id)
                .collect::<Vec<_>>();

            match matching_runs.as_slice() {
                [run] => {
                    println!(
                        "{} {} is a plan reference; restoring applied run {} instead",
                        "Info:".bright_blue(),
                        input.bright_white(),
                        run.id.bright_white()
                    );
                    return Ok(run.id.clone());
                }
                [] => {
                    return Err(format!(
                        "'{}' is a plan id, but no run has applied plan {} yet",
                        input, plan.id
                    ));
                }
                _ => {
                    let ids = matching_runs
                        .iter()
                        .map(|run| run.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(format!(
                        "'{}' maps to multiple runs for plan {}: {}",
                        input, plan.id, ids
                    ));
                }
            }
        }

        Err(format!(
            "Run '{}' was not found.{}",
            input,
            Self::latest_run_hint(&runs)
        ))
    }

    fn matching_plan_for_input<'a>(
        input: &str,
        plans: &'a [CleanupPlan],
    ) -> Option<&'a CleanupPlan> {
        if let Some(plan) = plans.iter().find(|plan| plan.id == input) {
            return Some(plan);
        }

        input
            .strip_prefix("run-")
            .and_then(|timestamp| {
                let plan_id = format!("plan-{}", timestamp);
                plans.iter().find(|plan| plan.id == plan_id)
            })
            .or_else(|| {
                input.strip_prefix("plan-").and_then(|timestamp| {
                    let plan_id = format!("plan-{}", timestamp);
                    plans.iter().find(|plan| plan.id == plan_id)
                })
            })
    }

    fn latest_run_hint(runs: &[model::RunManifest]) -> String {
        runs.first()
            .map(|run| format!(" Latest run is {}.", run.id))
            .unwrap_or_default()
    }

    fn latest_plan_hint(plans: &[CleanupPlan]) -> String {
        plans
            .first()
            .map(|plan| format!(" Latest plan is {}.", plan.id))
            .unwrap_or_default()
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

fn action_size(action: &PlanAction) -> u64 {
    fs::metadata(&action.source)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
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
    use crate::{
        executor::execute_plan_inner,
        planner::build_plan,
        scanner::{options_from_config, scan_directory},
    };

    #[test]
    fn human_bytes_formats_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
    }

    #[test]
    fn action_ids_for_kind_filters_large_plan_modes() {
        let root = PathBuf::from("/tmp/folder-cleaner-test");
        let plan = CleanupPlan {
            id: "plan-test".to_string(),
            created_at: "2026-05-08T00:00:00Z".to_string(),
            root: root.clone(),
            profile: "downloads".to_string(),
            actions: vec![
                PlanAction {
                    id: 1,
                    kind: OperationKind::Organize,
                    source: root.join("photo.jpg"),
                    target: root.join("Images/photo.jpg"),
                    reason: "organize as images".to_string(),
                    category: Some("images".to_string()),
                    hash: None,
                },
                PlanAction {
                    id: 2,
                    kind: OperationKind::Quarantine,
                    source: root.join("draft.tmp"),
                    target: root.join(".lla-quarantine/plan-test/draft.tmp"),
                    reason: "temporary file".to_string(),
                    category: Some("cleanup".to_string()),
                    hash: None,
                },
            ],
        };

        assert_eq!(
            FolderCleanerPlugin::action_ids_for_kind(&plan, None),
            HashSet::from([1, 2])
        );
        assert_eq!(
            FolderCleanerPlugin::action_ids_for_kind(&plan, Some(&OperationKind::Organize)),
            HashSet::from([1])
        );
        assert_eq!(
            FolderCleanerPlugin::action_ids_for_kind(&plan, Some(&OperationKind::Quarantine)),
            HashSet::from([2])
        );
    }

    #[test]
    fn matching_plan_accepts_run_like_plan_timestamp() {
        let plan = CleanupPlan {
            id: "plan-20260508091007338".to_string(),
            created_at: "2026-05-08T09:10:07Z".to_string(),
            root: PathBuf::from("/tmp/folder-cleaner-test"),
            profile: "downloads".to_string(),
            actions: Vec::new(),
        };
        let plans = vec![plan];

        assert_eq!(
            FolderCleanerPlugin::matching_plan_for_input("run-20260508091007338", &plans)
                .map(|plan| plan.id.as_str()),
            Some("plan-20260508091007338")
        );
        assert_eq!(
            FolderCleanerPlugin::matching_plan_for_input("plan-20260508091007338", &plans)
                .map(|plan| plan.id.as_str()),
            Some("plan-20260508091007338")
        );
    }

    #[test]
    fn mixed_downloads_plan_organizes_and_quarantines_safely() {
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("photo.jpg");
        let temp_file = temp.path().join("draft.tmp");
        std::fs::write(&image, b"image").unwrap();
        std::fs::write(&temp_file, b"tmp").unwrap();

        let config = FolderCleanerConfig::default();
        let profile = ProfileConfig {
            recursive: Some(false),
            ..ProfileConfig::default()
        };
        let options = options_from_config(&config, &profile);
        let report = scan_directory(temp.path(), &config, &options).unwrap();
        let root = report.root.clone();
        let plan = build_plan(&report, &config, "downloads", &profile);

        assert!(plan.actions.iter().any(|action| {
            action.kind == OperationKind::Organize
                && action.target == root.join("Images").join("photo.jpg")
        }));
        assert!(plan.actions.iter().any(|action| {
            action.kind == OperationKind::Quarantine && action.source == root.join("draft.tmp")
        }));

        let manifest = execute_plan_inner(&plan, None, false).unwrap();
        assert_eq!(manifest.actions.len(), plan.actions.len());
        assert!(root.join("Images").join("photo.jpg").exists());
        assert!(manifest
            .actions
            .iter()
            .filter(|action| action.operation == OperationKind::Quarantine)
            .all(|action| action.target.starts_with(root.join(".lla-quarantine"))));
    }
}
