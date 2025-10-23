use colored::Colorize;
use dialoguer::{
    console::{Key, Term},
    Confirm, MultiSelect,
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use lazy_static::lazy_static;
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::time::Duration;
use sysinfo::System;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillProcessConfig {
    #[serde(default = "default_colors")]
    colors: std::collections::HashMap<String, String>,
}

fn default_colors() -> std::collections::HashMap<String, String> {
    let mut colors = std::collections::HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("error".to_string(), "bright_red".to_string());
    colors.insert("warning".to_string(), "bright_yellow".to_string());
    colors.insert("process_name".to_string(), "bright_cyan".to_string());
    colors.insert("pid".to_string(), "bright_magenta".to_string());
    colors
}

impl Default for KillProcessConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for KillProcessConfig {}

#[derive(Debug, Clone)]
struct ProcessInfo {
    pid: u32,
    name: String,
    cpu_usage: f32,
    memory: u64,
}

lazy_static! {
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "list",
            "list",
            "List all running processes with detailed information",
            vec!["lla plugin --name kill_process --action list"],
            |_| KillProcessPlugin::list_action()
        );

        lla_plugin_utils::define_action!(
            registry,
            "kill",
            "kill",
            "Interactively select and kill a process",
            vec!["lla plugin --name kill_process --action kill"],
            |_| KillProcessPlugin::kill_action(false)
        );

        lla_plugin_utils::define_action!(
            registry,
            "force-kill",
            "force-kill",
            "Forcefully terminate a process (SIGKILL on Unix, /F on Windows)",
            vec!["lla plugin --name kill_process --action force-kill"],
            |_| KillProcessPlugin::kill_action(true)
        );

        lla_plugin_utils::define_action!(
            registry,
            "kill-by-name",
            "kill-by-name <name>",
            "Kill processes matching a specific name pattern",
            vec![
                "lla plugin --name kill_process --action kill-by-name --args chrome",
                "lla plugin --name kill_process --action kill-by-name --args firefox"
            ],
            |args| KillProcessPlugin::kill_by_name_action(args, false)
        );

        lla_plugin_utils::define_action!(
            registry,
            "force-kill-by-name",
            "force-kill-by-name <name>",
            "Forcefully kill processes matching a specific name pattern",
            vec!["lla plugin --name kill_process --action force-kill-by-name --args chrome"],
            |args| KillProcessPlugin::kill_by_name_action(args, true)
        );

        lla_plugin_utils::define_action!(
            registry,
            "kill-by-pid",
            "kill-by-pid <pid>",
            "Kill a specific process by its PID",
            vec!["lla plugin --name kill_process --action kill-by-pid --args 1234"],
            |args| KillProcessPlugin::kill_by_pid_action(args, false)
        );

        lla_plugin_utils::define_action!(
            registry,
            "force-kill-by-pid",
            "force-kill-by-pid <pid>",
            "Forcefully kill a specific process by its PID",
            vec!["lla plugin --name kill_process --action force-kill-by-pid --args 1234"],
            |args| KillProcessPlugin::kill_by_pid_action(args, true)
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            vec!["lla plugin --name kill_process --action help"],
            |_| KillProcessPlugin::help_action()
        );

        registry
    });
}

pub struct KillProcessPlugin {
    base: BasePlugin<KillProcessConfig>,
}

impl KillProcessPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        Self {
            base: BasePlugin::with_name(plugin_name),
        }
    }

    fn get_processes() -> Vec<ProcessInfo> {
        let mut sys = System::new_all();

        // Sleep briefly and refresh for accurate CPU usage
        std::thread::sleep(std::time::Duration::from_millis(200));
        sys.refresh_all();

        let mut processes: Vec<ProcessInfo> = sys
            .processes()
            .iter()
            .map(|(pid, process)| ProcessInfo {
                pid: pid.as_u32(),
                name: process.name().to_string_lossy().to_string(),
                cpu_usage: process.cpu_usage(),
                memory: process.memory(),
            })
            .collect();

        processes.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        processes
    }

    fn format_process_display(process: &ProcessInfo) -> String {
        format!(
            "{:<40} PID: {:<8} CPU: {:>5.1}% MEM: {:>8}",
            process.name.chars().take(40).collect::<String>(),
            process.pid.to_string().bright_magenta(),
            process.cpu_usage,
            Self::format_memory(process.memory)
        )
    }

    fn format_memory(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    fn is_windows() -> bool {
        cfg!(target_os = "windows")
    }

    fn is_macos() -> bool {
        cfg!(target_os = "macos")
    }

    fn get_kill_command(pid: u32, force: bool) -> Result<String, String> {
        if Self::is_windows() {
            if force {
                Ok(format!("taskkill /F /PID {}", pid))
            } else {
                Ok(format!("taskkill /PID {}", pid))
            }
        } else {
            // Unix-like (macOS, Linux)
            if force {
                // For force kill, we might need sudo, but let's try without first
                Ok(format!("kill -9 {}", pid))
            } else {
                Ok(format!("kill -15 {}", pid))
            }
        }
    }

    fn kill_process(pid: u32, name: &str, force: bool) -> Result<(), String> {
        let command = Self::get_kill_command(pid, force)?;

        let output = if Self::is_windows() {
            std::process::Command::new("cmd")
                .args(["/C", &command])
                .output()
        } else {
            std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .output()
        };

        match output {
            Ok(output) => {
                if output.status.success() {
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let error_msg = if !stderr.is_empty() {
                        stderr.to_string()
                    } else {
                        "Process termination failed".to_string()
                    };

                    Err(format!(
                        "Failed to kill process {} (PID: {}): {}{}",
                        name,
                        pid,
                        error_msg,
                        Self::get_platform_specific_help(force)
                    ))
                }
            }
            Err(e) => Err(format!(
                "Failed to execute kill command: {}{}",
                e,
                Self::get_platform_specific_help(force)
            )),
        }
    }

    fn get_platform_specific_help(force: bool) -> String {
        if Self::is_windows() {
            "\n\nTip: You may need to run your terminal as Administrator to kill this process."
                .to_string()
        } else if Self::is_macos() && force {
            "\n\nTip: Force killing may require elevated privileges. Try running with sudo if this fails.".to_string()
        } else {
            "\n\nTip: The process may have already exited or requires elevated privileges."
                .to_string()
        }
    }

    fn list_action() -> Result<(), String> {
        let processes = Self::get_processes();

        if processes.is_empty() {
            println!("{} No processes found", "Info:".bright_blue());
            return Ok(());
        }

        let count = processes.len();

        println!("\n{}\n", "Running Processes".bright_cyan().bold());
        println!(
            "{:<40} {:<15} {:<12} {:<10}",
            "Name", "PID", "CPU Usage", "Memory"
        );
        println!("{}", "─".repeat(80));

        for process in &processes {
            println!(
                "{:<40} {:<15} {:>5.1}%      {}",
                process
                    .name
                    .chars()
                    .take(40)
                    .collect::<String>()
                    .bright_cyan(),
                process.pid.to_string().bright_magenta(),
                process.cpu_usage,
                Self::format_memory(process.memory)
            );
        }

        println!("\n{} {} processes", "Total:".bright_blue(), count);

        Ok(())
    }

    fn live_fuzzy_search(all_processes: &[ProcessInfo]) -> Result<Vec<ProcessInfo>, String> {
        let term = Term::stdout();
        let mut input = String::new();
        let mut filtered_processes: Vec<ProcessInfo>;
        let mut selected_idx: usize = 0;

        term.hide_cursor()
            .map_err(|e| format!("Failed to hide cursor: {}", e))?;

        loop {
            // Clear screen and render
            term.clear_screen()
                .map_err(|e| format!("Failed to clear screen: {}", e))?;

            // Header
            println!("{}", "🔍 Live Process Search".bright_cyan().bold());
            println!("{}", "─".repeat(80));
            println!();

            // Search input
            println!(
                "{} {}",
                "Search:".bright_yellow(),
                if input.is_empty() {
                    "(type to filter)".bright_black().to_string()
                } else {
                    input.bright_white().to_string()
                }
            );
            println!();

            // Filter processes based on input
            if input.is_empty() {
                filtered_processes = all_processes.to_vec();
            } else {
                let matcher = SkimMatcherV2::default().ignore_case();
                let mut scored: Vec<(i64, ProcessInfo)> = all_processes
                    .iter()
                    .filter_map(|p| {
                        let search_text = format!("{} {}", p.name, p.pid);
                        matcher
                            .fuzzy_match(&search_text, &input)
                            .map(|score| (score, p.clone()))
                    })
                    .collect();

                scored.sort_by(|a, b| b.0.cmp(&a.0));
                filtered_processes = scored.into_iter().map(|(_, p)| p).collect();
            }

            // Keep selected_idx in bounds
            if selected_idx >= filtered_processes.len() && !filtered_processes.is_empty() {
                selected_idx = filtered_processes.len() - 1;
            }

            // Display results
            if filtered_processes.is_empty() {
                println!("{}", "No matches found".bright_red());
            } else {
                println!(
                    "{} {} process(es) • Use ↑↓ to navigate, Enter to select, Esc to cancel",
                    "Found:".bright_green(),
                    filtered_processes.len()
                );
                println!("{}", "─".repeat(80));
                println!();

                // Show up to 20 processes
                let display_count = filtered_processes.len().min(20);
                for (idx, process) in filtered_processes.iter().take(display_count).enumerate() {
                    let prefix = if idx == selected_idx {
                        "❯".bright_cyan().bold()
                    } else {
                        " ".normal()
                    };

                    let name_display = if idx == selected_idx {
                        process.name.bright_white().bold().to_string()
                    } else {
                        process.name.bright_white().to_string()
                    };

                    println!(
                        "{} {:<30} PID: {:<8} CPU: {:>5.1}% MEM: {:>8}",
                        prefix,
                        name_display.chars().take(30).collect::<String>(),
                        process.pid.to_string().bright_magenta(),
                        process.cpu_usage,
                        Self::format_memory(process.memory)
                    );
                }

                if filtered_processes.len() > 20 {
                    println!();
                    println!(
                        "{}",
                        format!("... and {} more", filtered_processes.len() - 20).bright_black()
                    );
                }
            }

            // Wait for key input
            match term
                .read_key()
                .map_err(|e| format!("Failed to read key: {}", e))?
            {
                Key::Char(c) => {
                    input.push(c);
                    selected_idx = 0;
                }
                Key::Backspace => {
                    input.pop();
                    selected_idx = 0;
                }
                Key::ArrowUp => {
                    if selected_idx > 0 {
                        selected_idx -= 1;
                    }
                }
                Key::ArrowDown => {
                    if selected_idx < filtered_processes.len().saturating_sub(1) {
                        selected_idx += 1;
                    }
                }
                Key::Enter => {
                    term.show_cursor()
                        .map_err(|e| format!("Failed to show cursor: {}", e))?;
                    term.clear_screen()
                        .map_err(|e| format!("Failed to clear screen: {}", e))?;
                    return Ok(filtered_processes);
                }
                Key::Escape => {
                    term.show_cursor()
                        .map_err(|e| format!("Failed to show cursor: {}", e))?;
                    term.clear_screen()
                        .map_err(|e| format!("Failed to clear screen: {}", e))?;
                    return Err("Cancelled by user".to_string());
                }
                _ => {}
            }

            // Small delay to prevent CPU spinning
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn kill_action(force: bool) -> Result<(), String> {
        let all_processes = Self::get_processes();

        if all_processes.is_empty() {
            println!("{} No processes found", "Info:".bright_blue());
            return Ok(());
        }

        // Live fuzzy search
        let processes = Self::live_fuzzy_search(&all_processes)?;

        if processes.is_empty() {
            println!("{} No processes to kill", "Info:".bright_blue());
            return Ok(());
        }

        println!(
            "\n{} Found {} matching process(es)\n",
            "Results:".bright_green(),
            processes.len()
        );

        let items: Vec<String> = processes
            .iter()
            .map(|p| Self::format_process_display(p))
            .collect();

        let theme = LlaDialoguerTheme::default();
        let prompt = if force {
            "Select processes to force kill (SIGKILL/taskkill /F)"
        } else {
            "Select processes to kill (SIGTERM/taskkill)"
        };

        let selections = MultiSelect::with_theme(&theme)
            .with_prompt(prompt)
            .items(&items)
            .interact()
            .map_err(|e| format!("Failed to show selector: {}", e))?;

        if selections.is_empty() {
            println!("{} No processes selected", "Info:".bright_blue());
            return Ok(());
        }

        // flush the screen
        print!("\x1B[2J\x1B[1;1H");

        // Show confirmation
        println!("\n{}", "Selected processes:\n".bright_yellow());
        for &idx in &selections {
            let process = &processes[idx];
            println!(
                "  {} {} (PID: {})",
                "•".bright_red(),
                process.name.bright_cyan(),
                process.pid.to_string().bright_magenta()
            );
        }

        let confirm_msg = if force {
            " Are you sure you want to FORCE KILL these processes? This cannot be undone."
        } else {
            " Are you sure you want to kill these processes?"
        };

        let confirmed = Confirm::with_theme(&theme)
            .with_prompt(confirm_msg)
            .default(false)
            .interact()
            .map_err(|e| format!("Failed to show confirmation: {}", e))?;

        if !confirmed {
            println!("{} Operation cancelled", "Info:".bright_blue());
            return Ok(());
        }

        // Kill the processes
        let mut success_count = 0;
        let mut failed: Vec<(String, String)> = Vec::new();

        for &idx in &selections {
            let process = &processes[idx];
            match Self::kill_process(process.pid, &process.name, force) {
                Ok(()) => {
                    println!(
                        "{} Killed process {} (PID: {})",
                        "✓".bright_green(),
                        process.name.bright_cyan(),
                        process.pid.to_string().bright_magenta()
                    );
                    success_count += 1;
                }
                Err(e) => {
                    eprintln!("{} {}", "✗".bright_red(), e);
                    failed.push((process.name.clone(), e));
                }
            }
        }

        println!(
            "\n{} Successfully killed {}/{} processes",
            "Summary:".bright_blue(),
            success_count,
            selections.len()
        );

        if !failed.is_empty() {
            println!(
                "{} {} processes failed",
                "Warning:".bright_yellow(),
                failed.len()
            );
        }

        Ok(())
    }

    fn kill_by_name_action(args: &[String], force: bool) -> Result<(), String> {
        if args.is_empty() {
            return Err("Process name is required. Usage: kill-by-name <name>".to_string());
        }

        let pattern = args[0].to_lowercase();
        let processes = Self::get_processes();

        let matching: Vec<&ProcessInfo> = processes
            .iter()
            .filter(|p| p.name.to_lowercase().contains(&pattern))
            .collect();

        if matching.is_empty() {
            println!(
                "{} No processes found matching '{}'",
                "Info:".bright_blue(),
                pattern.bright_yellow()
            );
            return Ok(());
        }

        let count = matching.len();
        println!(
            "\n{} {} matching process(es) for '{}':",
            "Found:".bright_blue(),
            count,
            pattern.bright_yellow()
        );

        for process in &matching {
            println!(
                "  {} {} (PID: {})",
                "•".bright_cyan(),
                process.name.bright_cyan(),
                process.pid.to_string().bright_magenta()
            );
        }

        let theme = LlaDialoguerTheme::default();
        let confirm_msg = if force {
            format!(
                "\nAre you sure you want to FORCE KILL {} process(es)? This cannot be undone.",
                count
            )
        } else {
            format!("\nAre you sure you want to kill {} process(es)?", count)
        };

        let confirmed = Confirm::with_theme(&theme)
            .with_prompt(confirm_msg)
            .default(false)
            .interact()
            .map_err(|e| format!("Failed to show confirmation: {}", e))?;

        if !confirmed {
            println!("{} Operation cancelled", "Info:".bright_blue());
            return Ok(());
        }

        let mut success_count = 0;
        for process in &matching {
            match Self::kill_process(process.pid, &process.name, force) {
                Ok(()) => {
                    println!(
                        "{} Killed process {} (PID: {})",
                        "✓".bright_green(),
                        process.name.bright_cyan(),
                        process.pid.to_string().bright_magenta()
                    );
                    success_count += 1;
                }
                Err(e) => {
                    eprintln!("{} {}", "✗".bright_red(), e);
                }
            }
        }

        println!(
            "\n{} Successfully killed {}/{} processes",
            "Summary:".bright_blue(),
            success_count,
            count
        );

        Ok(())
    }

    fn kill_by_pid_action(args: &[String], force: bool) -> Result<(), String> {
        if args.is_empty() {
            return Err("PID is required. Usage: kill-by-pid <pid>".to_string());
        }

        let pid: u32 = args[0]
            .parse()
            .map_err(|_| format!("Invalid PID: '{}'", args[0]))?;

        // Verify process exists
        let processes = Self::get_processes();
        let process = processes
            .iter()
            .find(|p| p.pid == pid)
            .ok_or_else(|| format!("Process with PID {} not found", pid))?;

        println!(
            "\n{} Process: {} (PID: {})",
            "Target:".bright_blue(),
            process.name.bright_cyan(),
            process.pid.to_string().bright_magenta()
        );
        println!(
            "  CPU: {:.1}%, Memory: {}",
            process.cpu_usage,
            Self::format_memory(process.memory)
        );

        let theme = LlaDialoguerTheme::default();
        let confirm_msg = if force {
            format!(
                "\nAre you sure you want to FORCE KILL {} (PID: {})? This cannot be undone.",
                process.name, pid
            )
        } else {
            format!(
                "\nAre you sure you want to kill {} (PID: {})?",
                process.name, pid
            )
        };

        let confirmed = Confirm::with_theme(&theme)
            .with_prompt(confirm_msg)
            .default(false)
            .interact()
            .map_err(|e| format!("Failed to show confirmation: {}", e))?;

        if !confirmed {
            println!("{} Operation cancelled", "Info:".bright_blue());
            return Ok(());
        }

        match Self::kill_process(pid, &process.name, force) {
            Ok(()) => {
                println!(
                    "{} Successfully killed process {} (PID: {})",
                    "Success:".bright_green(),
                    process.name.bright_cyan(),
                    pid.to_string().bright_magenta()
                );
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn help_action() -> Result<(), String> {
        let mut help = HelpFormatter::new("Kill Process".to_string());
        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Manage and terminate system processes with an interactive interface, inspired by Raycast".to_string(),
            vec![],
        );

        help.add_section("Process Management".to_string())
            .add_command(
                "list".to_string(),
                "List all running processes with detailed information".to_string(),
                vec!["lla plugin --name kill_process --action list".to_string()],
            )
            .add_command(
                "kill".to_string(),
                "Interactively select and kill processes with fuzzy search (SIGTERM/taskkill)"
                    .to_string(),
                vec!["lla plugin --name kill_process --action kill".to_string()],
            )
            .add_command(
                "force-kill".to_string(),
                "Forcefully terminate processes with fuzzy search (SIGKILL/taskkill /F)"
                    .to_string(),
                vec!["lla plugin --name kill_process --action force-kill".to_string()],
            );

        help.add_section("Direct Targeting".to_string())
            .add_command(
                "kill-by-name <name>".to_string(),
                "Kill processes matching a specific name pattern".to_string(),
                vec![
                    "lla plugin --name kill_process --action kill-by-name --args chrome"
                        .to_string(),
                    "lla plugin --name kill_process --action kill-by-name --args firefox"
                        .to_string(),
                ],
            )
            .add_command(
                "force-kill-by-name <name>".to_string(),
                "Forcefully kill processes matching a name pattern".to_string(),
                vec![
                    "lla plugin --name kill_process --action force-kill-by-name --args chrome"
                        .to_string(),
                ],
            )
            .add_command(
                "kill-by-pid <pid>".to_string(),
                "Kill a specific process by its PID".to_string(),
                vec!["lla plugin --name kill_process --action kill-by-pid --args 1234".to_string()],
            )
            .add_command(
                "force-kill-by-pid <pid>".to_string(),
                "Forcefully kill a specific process by its PID".to_string(),
                vec![
                    "lla plugin --name kill_process --action force-kill-by-pid --args 1234"
                        .to_string(),
                ],
            );

        help.add_section("Platform-Specific Notes".to_string())
            .add_command(
                "macOS/Linux".to_string(),
                "Uses kill -15 (SIGTERM) for normal, kill -9 (SIGKILL) for force. May require sudo.".to_string(),
                vec![],
            )
            .add_command(
                "Windows".to_string(),
                "Uses taskkill /PID for normal, taskkill /F /PID for force. May require Administrator.".to_string(),
                vec![],
            );

        help.add_section("Safety".to_string()).add_command(
            "".to_string(),
            "Always confirms before killing processes. Force kill should be used as last resort."
                .to_string(),
            vec![],
        );

        println!(
            "{}",
            BoxComponent::new(help.render(&KillProcessConfig::default().colors))
                .style(BoxStyle::Minimal)
                .padding(1)
                .render()
        );
        Ok(())
    }
}

impl Deref for KillProcessPlugin {
    type Target = KillProcessConfig;

    fn deref(&self) -> &Self::Target {
        self.base.config()
    }
}

impl Plugin for KillProcessPlugin {
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

impl ConfigurablePlugin for KillProcessPlugin {
    type Config = KillProcessConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for KillProcessPlugin {}

lla_plugin_interface::declare_plugin!(KillProcessPlugin);

impl Default for KillProcessPlugin {
    fn default() -> Self {
        Self::new()
    }
}
