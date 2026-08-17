use crate::commands::args::{Args, UpgradeCommand};
use crate::error::{LlaError, Result};
use crate::theme::color_value_to_color;
use crate::utils::color::{get_theme, ColorState};
use colored::{Color, Colorize};
use console::Term;
#[cfg(feature = "dynamic-plugins")]
use dialoguer::Confirm;
use dialoguer::MultiSelect;
use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
#[cfg(feature = "dynamic-plugins")]
use lla_plugin_interface::{
    manifest::{PluginManifest, PluginRuntime},
    PLUGIN_API_VERSION,
};
use lla_plugin_utils::ui::components::{BoxComponent, BoxStyle, LlaDialoguerTheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};
use tar::Archive;
use toml::{self, Value};
use ureq::{Agent, AgentBuilder, Error as UreqError, Request};
use walkdir::WalkDir;
use zip::ZipArchive;

const GITHUB_REPOSITORY: &str = "chaqchase/lla";
const PREBUILT_USER_AGENT: &str = concat!("lla/", env!("CARGO_PKG_VERSION"), " prebuilt-installer");

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone)]
struct PrebuiltPlugin {
    path: PathBuf,
    name: String,
    version: String,
    description: String,
}

#[derive(Debug, Clone, Copy)]
struct HostTarget {
    os_label: &'static str,
    arch_label: &'static str,
    library_extension: &'static str,
}

impl HostTarget {
    fn detect() -> Result<Self> {
        use std::env::consts::{ARCH, OS};

        match (OS, ARCH) {
            ("macos", "x86_64") => Ok(Self {
                os_label: "macos",
                arch_label: "amd64",
                library_extension: "dylib",
            }),
            ("macos", "aarch64") => Ok(Self {
                os_label: "macos",
                arch_label: "arm64",
                library_extension: "dylib",
            }),
            ("linux", "x86_64") => Ok(Self {
                os_label: "linux",
                arch_label: "amd64",
                library_extension: "so",
            }),
            ("linux", "aarch64") => Ok(Self {
                os_label: "linux",
                arch_label: "arm64",
                library_extension: "so",
            }),
            ("linux", arch) if arch == "i686" || arch == "x86" => Ok(Self {
                os_label: "linux",
                arch_label: "i686",
                library_extension: "so",
            }),
            _ => Err(LlaError::Plugin(format!(
                "Unsupported platform for prebuilt plugins: {}-{}",
                OS, ARCH
            ))),
        }
    }

    fn asset_candidates(&self) -> Vec<String> {
        vec![
            format!("plugins-{}-{}.tar.gz", self.os_label, self.arch_label),
            format!("plugins-{}-{}.zip", self.os_label, self.arch_label),
        ]
    }
}

#[derive(Debug, Clone, Copy)]
struct CliBinaryTarget {
    os_label: &'static str,
    arch_label: &'static str,
    display_os: &'static str,
    musl: bool,
}

impl CliBinaryTarget {
    fn detect() -> Result<Self> {
        use std::env::consts::{ARCH, OS};

        Self::for_platform(OS, ARCH, cfg!(target_env = "musl"))
    }

    fn for_platform(os: &str, arch: &str, musl: bool) -> Result<Self> {
        if musl && os == "linux" && arch != "x86_64" && arch != "aarch64" {
            return Err(LlaError::Other(format!(
                "Unsupported platform for static musl CLI upgrades: {}-{} (supported: Linux on amd64 and arm64)",
                os, arch
            )));
        }

        match (os, arch) {
            ("macos", "x86_64") => Ok(Self {
                os_label: "macos",
                arch_label: "amd64",
                display_os: "macOS",
                musl: false,
            }),
            ("macos", "aarch64") => Ok(Self {
                os_label: "macos",
                arch_label: "arm64",
                display_os: "macOS",
                musl: false,
            }),
            ("linux", "x86_64") => Ok(Self {
                os_label: "linux",
                arch_label: "amd64",
                display_os: "Linux",
                musl,
            }),
            ("linux", "aarch64") => Ok(Self {
                os_label: "linux",
                arch_label: "arm64",
                display_os: "Linux",
                musl,
            }),
            ("linux", arch) if arch == "i686" || arch == "x86" => Ok(Self {
                os_label: "linux",
                arch_label: "i686",
                display_os: "Linux",
                musl: false,
            }),
            _ => Err(LlaError::Other(format!(
                "Unsupported platform for CLI upgrades: {}-{} (supported: macOS/Linux on amd64, arm64, i686)",
                os, arch
            ))),
        }
    }

    fn asset_name(&self) -> String {
        let musl_suffix = if self.musl { "-musl" } else { "" };
        format!("lla-{}-{}{}", self.os_label, self.arch_label, musl_suffix)
    }

    fn human_label(&self) -> String {
        let libc = if self.musl { " musl" } else { "" };
        format!("{}{} ({})", self.display_os, libc, self.arch_label)
    }
}

#[derive(Clone, Copy)]
enum StatusKind {
    Success,
    Info,
    Error,
}

struct InstallerUi<'a> {
    color_state: &'a ColorState,
    stdout: Term,
}

impl<'a> InstallerUi<'a> {
    fn new(color_state: &'a ColorState) -> Self {
        Self {
            color_state,
            stdout: Term::stdout(),
        }
    }

    fn write_stdout(&self, line: impl AsRef<str>) {
        let _ = self.stdout.write_line(line.as_ref());
        let _ = self.stdout.flush();
    }

    fn blank_line(&self) {
        self.write_stdout("");
    }

    fn stylize(&self, text: &str, color: Color, bold: bool) -> String {
        if self.color_state.is_enabled() {
            let styled = if bold {
                text.color(color).bold()
            } else {
                text.color(color)
            };
            styled.to_string()
        } else {
            text.to_string()
        }
    }

    fn accent_color(&self) -> Color {
        let theme = get_theme();
        color_value_to_color(&theme.colors.directory)
    }

    fn success_color(&self) -> Color {
        let theme = get_theme();
        color_value_to_color(&theme.colors.executable)
    }

    fn error_color(&self) -> Color {
        let theme = get_theme();
        color_value_to_color(&theme.colors.permission_exec)
    }

    fn info_color(&self) -> Color {
        let theme = get_theme();
        color_value_to_color(&theme.colors.date)
    }

    fn muted_color(&self) -> Color {
        Color::BrightBlack
    }

    fn format_size(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{:.0} {}", size, UNITS[unit_index])
        } else {
            format!("{:.1} {}", size, UNITS[unit_index])
        }
    }

    fn format_speed(bytes_per_sec: f64) -> String {
        Self::format_size(bytes_per_sec as u64) + "/s"
    }

    fn format_duration(duration: Duration) -> String {
        let total_secs = duration.as_secs();
        if total_secs >= 3600 {
            format!(
                "{}h{}m{}s",
                total_secs / 3600,
                (total_secs % 3600) / 60,
                total_secs % 60
            )
        } else if total_secs >= 60 {
            format!("{}m{}s", total_secs / 60, total_secs % 60)
        } else {
            format!("{}s", total_secs)
        }
    }

    fn accent_text(&self, text: &str) -> String {
        self.stylize(text, self.accent_color(), true)
    }

    fn highlight_text(&self, text: &str) -> String {
        self.stylize(text, self.accent_color(), false)
    }

    fn muted_text(&self, text: &str) -> String {
        self.stylize(text, self.muted_color(), false)
    }

    fn info_text(&self, text: &str) -> String {
        self.stylize(text, self.info_color(), false)
    }

    fn error_text(&self, text: &str) -> String {
        self.stylize(text, self.error_color(), false)
    }

    fn status_icon(&self, kind: StatusKind) -> String {
        match kind {
            StatusKind::Success => self.stylize("✔", self.success_color(), true),
            StatusKind::Info => self.stylize("ℹ", self.info_color(), true),
            StatusKind::Error => self.stylize("✗", self.error_color(), true),
        }
    }

    fn format_status(&self, kind: StatusKind, message: impl AsRef<str>) -> String {
        format!("  {} {}", self.status_icon(kind), message.as_ref())
    }

    fn print_status(&self, kind: StatusKind, message: impl AsRef<str>) {
        let formatted = self.format_status(kind, message);
        self.write_stdout(formatted);
    }

    fn section(&self, title: &str) {
        self.blank_line();
        let dot = self.stylize("›", self.accent_color(), true);
        let label = self.accent_text(title);
        self.write_stdout(format!("  {} {}", dot, label));
    }

    fn banner(&self, title: &str) {
        let content = format!("  {}", title);
        let output = BoxComponent::new(content)
            .style(BoxStyle::Rounded)
            .title(self.accent_text("lla"))
            .padding(1)
            .render();
        self.write_stdout(output);
    }

    fn name_with_version(&self, name: &str, version: &str) -> String {
        let version_tag = format!("v{}", version);
        format!(
            "{}  {}",
            self.accent_text(name),
            self.muted_text(&version_tag)
        )
    }

    fn progress_message(&self, label: &str, subject: &str) -> String {
        format!("{} {}", self.info_text(label), self.highlight_text(subject))
    }

    fn spinner_token(&self) -> &'static str {
        match self.accent_color() {
            Color::Black => "black",
            Color::Red => "red",
            Color::Green => "green",
            Color::Yellow => "yellow",
            Color::Blue => "blue",
            Color::Magenta => "magenta",
            Color::Cyan => "cyan",
            Color::White => "white",
            Color::BrightBlack => "bright_black",
            Color::BrightRed => "bright_red",
            Color::BrightGreen => "bright_green",
            Color::BrightYellow => "bright_yellow",
            Color::BrightBlue => "bright_blue",
            Color::BrightMagenta => "bright_magenta",
            Color::BrightCyan => "bright_cyan",
            Color::BrightWhite => "bright_white",
            _ => "cyan",
        }
    }

    fn progress_style(&self) -> ProgressStyle {
        let template = if self.color_state.is_enabled() {
            format!("{{spinner:.{}}} {{wide_msg}}", self.spinner_token())
        } else {
            "{spinner} {wide_msg}".to_string()
        };

        ProgressStyle::with_template(&template)
            .expect("valid progress style template")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
    }

    fn download_progress_style(&self) -> ProgressStyle {
        let template = if self.color_state.is_enabled() {
            format!(
                "{{spinner:.{}}} {{msg}} [{{bar:18.{}/{}}}] {{percent}}% {{bytes}}/{{total_bytes}} ({{eta}})",
                self.spinner_token(),
                self.spinner_token(),
                "black"
            )
        } else {
            "{spinner} {msg} [{bar:18}] {percent}% {bytes}/{total_bytes} ({eta})".to_string()
        };

        ProgressStyle::with_template(&template)
            .expect("valid download progress style template")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .progress_chars("█▉▊▋▌▍▎▏ ")
    }

    fn build_progress_style(&self) -> ProgressStyle {
        let template = if self.color_state.is_enabled() {
            format!(
                "{{spinner:.{}}} {{wide_msg}} {{elapsed}}",
                self.spinner_token()
            )
        } else {
            "{spinner} {wide_msg} {elapsed}".to_string()
        };

        ProgressStyle::with_template(&template)
            .expect("valid build progress style template")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
    }

    fn complete_progress(&self, pb: &ProgressBar) {
        // Stop the spinner animation
        pb.disable_steady_tick();

        // Finish and clear the progress bar immediately
        // Don't show any completion message here - it will be shown in the summary
        pb.finish_and_clear();

        // Brief delay to let the terminal fully process the clear
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    fn complete_progress_standalone(
        &self,
        pb: &ProgressBar,
        kind: StatusKind,
        message: impl AsRef<str>,
    ) {
        // This version is for standalone progress bars (not grouped with others)
        // Stop the spinner animation
        pb.disable_steady_tick();

        // Format the final message with status icon
        let final_message = self.format_status(kind, message);

        // Clear first, then print to stdout
        pb.finish_and_clear();

        // Critical: ensure stderr is flushed and give terminal time to process
        let _ = std::io::stderr().flush();
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Now write the status to stdout
        self.write_stdout(final_message);
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum PluginSource {
    Git {
        url: String,
    },
    Local {
        directory: String,
    },
    Prebuilt {
        release_tag: String,
        asset: String,
        checksum: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PluginMetadata {
    name: String,
    version: String,
    source: PluginSource,
    installed_at: String,
    last_updated: String,
    repository_name: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct MetadataStore {
    plugins: HashMap<String, PluginMetadata>,
}

impl PluginMetadata {
    fn new(
        name: String,
        version: String,
        source: PluginSource,
        repository_name: Option<String>,
    ) -> Self {
        let now = chrono::Local::now().to_rfc3339();
        Self {
            name,
            version,
            source,
            installed_at: now.clone(),
            last_updated: now,
            repository_name,
        }
    }

    fn update_timestamp(&mut self) {
        self.last_updated = chrono::Local::now().to_rfc3339();
    }
}

#[derive(Default)]
struct InstallSummary {
    successful: Vec<(String, String)>,
    failed: Vec<(String, String)>,
}

impl InstallSummary {
    fn add_success(&mut self, name: String, version: String) {
        self.successful.push((name, version));
    }

    fn add_failure(&mut self, name: String, error: String) {
        self.failed.push((name, error));
    }

    fn display(&self, ui: &InstallerUi) {
        if self.successful.is_empty() && self.failed.is_empty() {
            ui.print_status(StatusKind::Info, "No plugins processed");
            return;
        }

        let mut lines: Vec<String> = Vec::new();

        if !self.successful.is_empty() {
            let header = format!(
                "  {} {}",
                ui.stylize("●", ui.success_color(), true),
                ui.stylize(
                    &format!("Installed ({})", self.successful.len()),
                    ui.success_color(),
                    true,
                )
            );
            lines.push(header);
            lines.push(String::new());
            for (name, version) in &self.successful {
                lines.push(format!(
                    "      {}  {}",
                    ui.accent_text(name),
                    ui.muted_text(&format!("v{}", version))
                ));
            }
        }

        if !self.successful.is_empty() && !self.failed.is_empty() {
            lines.push(String::new());
        }

        if !self.failed.is_empty() {
            let header = format!(
                "  {} {}",
                ui.stylize("●", ui.error_color(), true),
                ui.stylize(
                    &format!("Failed ({})", self.failed.len()),
                    ui.error_color(),
                    true,
                )
            );
            lines.push(header);
            lines.push(String::new());
            for (name, error) in &self.failed {
                lines.push(format!(
                    "      {}  {}",
                    ui.highlight_text(name),
                    ui.error_text(error)
                ));
            }
        }

        let content = lines.join("\n");
        let output = BoxComponent::new(content)
            .style(BoxStyle::Rounded)
            .title(ui.accent_text("Summary"))
            .padding(1)
            .render();
        ui.write_stdout(output);

        let total = self.successful.len() + self.failed.len();
        ui.write_stdout(format!(
            "  {}  {} {} {} {}",
            ui.muted_text("∑"),
            ui.stylize(
                &format!("{} installed", self.successful.len()),
                ui.success_color(),
                false,
            ),
            ui.muted_text("·"),
            ui.stylize(
                &format!("{} failed", self.failed.len()),
                ui.error_color(),
                false,
            ),
            ui.muted_text(&format!("· {} total", total))
        ));
    }
}

pub struct PluginInstaller {
    plugins_dir: PathBuf,
    color_state: ColorState,
    no_progress: bool,
}

pub fn upgrade_cli(args: &Args, options: &UpgradeCommand) -> Result<()> {
    let installer = PluginInstaller::new(&args.plugins_dir, args);
    let ui = installer.ui();

    ui.banner("lla upgrade");

    let target = CliBinaryTarget::detect()?;
    let install_path = determine_install_path(options)?;

    let requested_version = options.version.as_deref().unwrap_or("latest").to_string();
    let normalized_version = options
        .version
        .as_ref()
        .map(|value| normalize_release_tag(value));

    let install_path_display = install_path.display().to_string();

    let env_lines = [
        format!(
            "  {}  {}  {}",
            ui.muted_text("Platform    "),
            ui.muted_text("│"),
            ui.highlight_text(&target.human_label())
        ),
        format!(
            "  {}  {}  {}",
            ui.muted_text("Install Path"),
            ui.muted_text("│"),
            ui.highlight_text(&install_path_display)
        ),
        format!(
            "  {}  {}  {}",
            ui.muted_text("Requested   "),
            ui.muted_text("│"),
            ui.highlight_text(&requested_version)
        ),
    ];
    let env_box = BoxComponent::new(env_lines.join("\n"))
        .style(BoxStyle::Rounded)
        .title(ui.accent_text("Environment"))
        .padding(1)
        .render();
    ui.write_stdout(env_box);

    let release_message = match normalized_version.as_deref() {
        Some(tag) => format!("Fetching release {}", tag),
        None => "Fetching latest release".to_string(),
    };
    let release_spinner = installer.create_status_spinner(&release_message);
    let release = PluginInstaller::fetch_release(normalized_version.as_deref()).map_err(|err| {
        ui.complete_progress_standalone(
            &release_spinner,
            StatusKind::Error,
            format!("Failed to fetch release: {}", err),
        );
        err
    })?;
    ui.complete_progress_standalone(
        &release_spinner,
        StatusKind::Success,
        format!("Release {}", ui.highlight_text(&release.tag_name)),
    );

    let asset_name = target.asset_name();
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name.eq_ignore_ascii_case(&asset_name))
        .ok_or_else(|| {
            LlaError::Other(format!(
                "Release {} does not contain the {} binary",
                release.tag_name, asset_name
            ))
        })?;

    ui.section("Download");
    let agent = PluginInstaller::github_agent();
    let temp_dir = tempfile::tempdir()?;
    let download_path = temp_dir.path().join(&asset_name);
    installer.download_to_path(&agent, &asset.browser_download_url, &download_path, &ui)?;
    mark_file_executable(&download_path)?;

    ui.section("Verification");
    verify_cli_checksum(
        &installer,
        &agent,
        &release,
        &asset_name,
        &download_path,
        &ui,
    )?;

    ui.section("Installation");
    install_cli_binary(
        &installer,
        &download_path,
        &install_path,
        &release.tag_name,
        &ui,
    )?;

    let current_version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let summary_lines = [
        format!(
            "  {}  {}  {}  {}",
            ui.muted_text("Previous"),
            ui.muted_text(&current_version),
            ui.stylize("→", ui.accent_color(), true),
            ui.stylize(&release.tag_name, ui.success_color(), true),
        ),
        format!(
            "  {}  {}",
            ui.muted_text("Path    "),
            ui.highlight_text(&install_path_display),
        ),
    ];
    let summary_content = summary_lines.join("\n");
    let summary_box = BoxComponent::new(summary_content)
        .style(BoxStyle::Rounded)
        .title(ui.accent_text("Upgrade Complete"))
        .padding(1)
        .render();
    ui.write_stdout(summary_box);

    ui.print_status(
        StatusKind::Info,
        format!("Run {} to verify", ui.highlight_text("lla --version")),
    );

    // Keep the temporary directory alive until here so the downloaded file is not removed mid-install
    drop(temp_dir);

    Ok(())
}

fn determine_install_path(options: &UpgradeCommand) -> Result<PathBuf> {
    if let Some(path) = &options.install_path {
        return Ok(path.clone());
    }

    std::env::current_exe().map_err(|err| {
        LlaError::Other(format!(
            "Failed to determine current executable path: {}. Pass --path to specify the install location.",
            err
        ))
    })
}

fn normalize_release_tag(tag: &str) -> String {
    let trimmed = tag.trim();
    if trimmed.starts_with('v') || trimmed.is_empty() {
        trimmed.to_string()
    } else {
        format!("v{}", trimmed)
    }
}

fn verify_cli_checksum(
    installer: &PluginInstaller,
    agent: &Agent,
    release: &GithubRelease,
    asset_name: &str,
    binary_path: &Path,
    ui: &InstallerUi,
) -> Result<()> {
    let spinner = installer.create_status_spinner("Verifying checksum…");
    match PluginInstaller::fetch_asset_checksum(agent, release, asset_name)? {
        Some(expected) => {
            let actual = PluginInstaller::calculate_sha256(binary_path)?;
            if actual.eq_ignore_ascii_case(&expected) {
                ui.complete_progress_standalone(
                    &spinner,
                    StatusKind::Success,
                    format!(
                        "Checksum OK ({})",
                        ui.muted_text(&expected[..expected.len().min(12)])
                    ),
                );
                Ok(())
            } else {
                ui.complete_progress_standalone(&spinner, StatusKind::Error, "Checksum mismatch");
                Err(LlaError::Other(format!(
                    "Checksum verification failed. Expected {}, got {}",
                    expected, actual
                )))
            }
        }
        None => {
            ui.complete_progress_standalone(
                &spinner,
                StatusKind::Info,
                "No checksum published for this release; skipping verification",
            );
            Ok(())
        }
    }
}

fn install_cli_binary(
    installer: &PluginInstaller,
    source: &Path,
    destination: &Path,
    release_tag: &str,
    ui: &InstallerUi,
) -> Result<()> {
    let install_message = ui.progress_message("Installing", &destination.display().to_string());
    let spinner = installer.create_status_spinner(&install_message);

    let parent = destination.parent().ok_or_else(|| {
        LlaError::Other(format!(
            "Invalid install path '{}': missing parent directory",
            destination.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|err| {
        ui.complete_progress_standalone(
            &spinner,
            StatusKind::Error,
            format!("Failed to prepare {}: {}", parent.display(), err),
        );
        LlaError::Other(format!(
            "Unable to create parent directory {}: {}",
            parent.display(),
            err
        ))
    })?;

    let unique_suffix = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_target = parent.join(format!(
        ".lla-upgrade-{}-{}",
        unique_suffix,
        std::process::id()
    ));

    fs::copy(source, &temp_target).map_err(|err| {
        ui.complete_progress_standalone(
            &spinner,
            StatusKind::Error,
            format!("Failed to copy binary: {}", err),
        );
        LlaError::Other(format!(
            "Failed to copy binary to {}: {}",
            temp_target.display(),
            err
        ))
    })?;
    mark_file_executable(&temp_target)?;

    let rename_result = fs::rename(&temp_target, destination);
    if let Err(err) = rename_result {
        let _ = fs::remove_file(&temp_target);
        ui.complete_progress_standalone(
            &spinner,
            StatusKind::Error,
            format!("Failed to install binary: {}", err),
        );
        return Err(LlaError::Other(format!(
            "Failed to install to {}: {}. Try re-running with elevated permissions or provide --path.",
            destination.display(),
            err
        )));
    }

    ui.complete_progress_standalone(
        &spinner,
        StatusKind::Success,
        format!(
            "Installed {} to {}",
            ui.highlight_text(release_tag),
            ui.highlight_text(&destination.display().to_string())
        ),
    );
    Ok(())
}

fn mark_file_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(path, perms)?;
    }

    #[cfg(not(unix))]
    {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_readonly(false);
        fs::set_permissions(path, perms)?;
    }

    Ok(())
}

impl PluginInstaller {
    pub fn new(plugins_dir: &Path, args: &Args) -> Self {
        let progress_pref = std::env::var("LLA_PROGRESS").unwrap_or_default();
        let force_progress = matches!(
            progress_pref.as_str(),
            "1" | "true" | "TRUE" | "on" | "ON" | "yes" | "YES"
        );
        let force_quiet = matches!(
            progress_pref.as_str(),
            "0" | "false" | "FALSE" | "off" | "OFF" | "no" | "NO"
        );
        let interactive = atty::is(atty::Stream::Stderr);
        let no_progress = if force_progress {
            false
        } else if force_quiet {
            true
        } else {
            !interactive
        };

        PluginInstaller {
            plugins_dir: plugins_dir.to_path_buf(),
            color_state: ColorState::new(args),
            no_progress,
        }
    }

    fn ui(&self) -> InstallerUi<'_> {
        InstallerUi::new(&self.color_state)
    }

    #[cfg(feature = "dynamic-plugins")]
    fn ensure_wasm_permissions(&self, manifest: &PluginManifest) -> Result<()> {
        if manifest.plugin.runtime != PluginRuntime::WasmComponent {
            return Ok(());
        }
        let path = crate::plugin::grants::GrantStore::path(&self.plugins_dir);
        let mut grants =
            crate::plugin::grants::GrantStore::load(&path).map_err(LlaError::Plugin)?;
        let expanded = grants.expanded_permissions(manifest);
        if expanded.is_empty() && grants.approves(manifest) {
            return Ok(());
        }
        if !atty::is(atty::Stream::Stdin) || !atty::is(atty::Stream::Stderr) {
            return Err(LlaError::Plugin(format!(
                "Plugin '{}' requests new permissions ({}); reinstall from a TTY to approve them",
                manifest.plugin.name,
                expanded.join(", ")
            )));
        }
        eprintln!(
            "Plugin '{}' requests: {}",
            manifest.plugin.name,
            if expanded.is_empty() {
                "no capabilities".to_string()
            } else {
                expanded.join(", ")
            }
        );
        let approved = Confirm::new()
            .with_prompt("Approve these WASM sandbox permissions?")
            .default(false)
            .interact()
            .map_err(|error| LlaError::Plugin(format!("permission prompt failed: {error}")))?;
        if !approved {
            return Err(LlaError::Plugin(format!(
                "Permission approval denied for '{}'",
                manifest.plugin.name
            )));
        }
        grants.record(manifest);
        grants.save(&path).map_err(LlaError::Plugin)
    }

    fn truncate_desc(desc: &str, max: usize) -> String {
        if desc.len() <= max {
            desc.to_string()
        } else if max > 1 {
            format!("{}…", &desc[..max - 1])
        } else {
            String::new()
        }
    }

    fn get_plugin_version(&self, plugin_dir: &Path) -> Result<String> {
        let cargo_toml_path = plugin_dir.join("Cargo.toml");
        let contents = fs::read_to_string(&cargo_toml_path)
            .map_err(|e| LlaError::Plugin(format!("Failed to read Cargo.toml: {}", e)))?;

        let cargo_toml: Value = toml::from_str(&contents)
            .map_err(|e| LlaError::Plugin(format!("Failed to parse Cargo.toml: {}", e)))?;

        let version = cargo_toml
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlaError::Plugin("No version found in Cargo.toml".to_string()))?;

        Ok(version.to_string())
    }

    fn get_plugin_package_name(&self, plugin_dir: &Path) -> Result<String> {
        let cargo_toml_path = plugin_dir.join("Cargo.toml");
        let contents = fs::read_to_string(&cargo_toml_path)
            .map_err(|e| LlaError::Plugin(format!("Failed to read Cargo.toml: {e}")))?;
        let cargo_toml: Value = toml::from_str(&contents)
            .map_err(|e| LlaError::Plugin(format!("Failed to parse Cargo.toml: {e}")))?;
        cargo_toml
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| LlaError::Plugin("No package name found in Cargo.toml".to_string()))
    }

    fn load_metadata_store(&self) -> Result<MetadataStore> {
        let metadata_path = self.plugins_dir.join("metadata.toml");
        if !metadata_path.exists() {
            return Ok(MetadataStore::default());
        }

        let contents = fs::read_to_string(&metadata_path)
            .map_err(|e| LlaError::Plugin(format!("Failed to read metadata.toml: {}", e)))?;

        toml::from_str(&contents)
            .map_err(|e| LlaError::Plugin(format!("Failed to parse metadata.toml: {}", e)))
    }

    fn save_metadata_store(&self, store: &MetadataStore) -> Result<()> {
        let metadata_path = self.plugins_dir.join("metadata.toml");
        fs::create_dir_all(&self.plugins_dir)?;

        let toml_string = toml::to_string_pretty(store)
            .map_err(|e| LlaError::Plugin(format!("Failed to serialize metadata: {}", e)))?;

        fs::write(&metadata_path, toml_string)
            .map_err(|e| LlaError::Plugin(format!("Failed to write metadata.toml: {}", e)))
    }

    fn update_plugin_metadata(&self, plugin_name: &str, metadata: PluginMetadata) -> Result<()> {
        let mut store = self.load_metadata_store()?;
        store.plugins.insert(plugin_name.to_string(), metadata);
        self.save_metadata_store(&store)
    }

    fn create_progress_style(&self) -> ProgressStyle {
        self.ui().progress_style()
    }

    fn progress_draw_target(&self) -> ProgressDrawTarget {
        if self.no_progress {
            ProgressDrawTarget::hidden()
        } else {
            ProgressDrawTarget::stderr_with_hz(16)
        }
    }

    fn create_status_spinner(&self, message: &str) -> ProgressBar {
        // Always visible single-line status spinner
        let pb = ProgressBar::new_spinner();
        pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(16));
        pb.set_style(self.create_progress_style());
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(120));
        pb
    }

    fn create_spinner(&self, message: &str) -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        pb.set_draw_target(self.progress_draw_target());
        pb.set_style(self.create_progress_style());
        pb.set_message(message.to_string());
        if !self.no_progress {
            pb.enable_steady_tick(Duration::from_millis(120));
        }
        pb
    }

    fn create_download_progress(&self, message: &str, total_size: Option<u64>) -> ProgressBar {
        let pb = if let Some(size) = total_size {
            ProgressBar::new(size)
        } else {
            ProgressBar::new_spinner()
        };

        let ui = self.ui();
        pb.set_draw_target(self.progress_draw_target());
        pb.set_style(ui.download_progress_style());
        pb.set_message(message.to_string());
        if !self.no_progress {
            pb.enable_steady_tick(Duration::from_millis(120));
        }
        pb
    }

    fn create_build_progress(&self, message: &str) -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        let ui = self.ui();
        pb.set_draw_target(self.progress_draw_target());
        pb.set_style(ui.build_progress_style());
        pb.set_message(message.to_string());
        if !self.no_progress {
            pb.enable_steady_tick(Duration::from_millis(120));
        }
        pb
    }

    fn github_agent() -> Agent {
        AgentBuilder::new().timeout(Duration::from_secs(60)).build()
    }

    fn github_request(agent: &Agent, url: &str) -> Request {
        let mut request = agent.get(url).set("User-Agent", PREBUILT_USER_AGENT);
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.trim().is_empty() {
                request = request.set("Authorization", &format!("Bearer {}", token));
            }
        }
        request
    }

    fn map_http_error(context: &str, err: UreqError) -> LlaError {
        match err {
            UreqError::Status(code, response) => {
                let body = response
                    .into_string()
                    .unwrap_or_else(|_| "<no body>".to_string());
                LlaError::Plugin(format!("{} (status {}): {}", context, code, body.trim()))
            }
            UreqError::Transport(transport) => {
                LlaError::Plugin(format!("{}: {}", context, transport))
            }
        }
    }

    fn fetch_release(tag: Option<&str>) -> Result<GithubRelease> {
        let agent = Self::github_agent();
        let url = match tag {
            Some(tag) => format!(
                "https://api.github.com/repos/{}/releases/tags/{}",
                GITHUB_REPOSITORY, tag
            ),
            None => format!(
                "https://api.github.com/repos/{}/releases/latest",
                GITHUB_REPOSITORY
            ),
        };

        let response = Self::github_request(&agent, &url)
            .call()
            .map_err(|err| Self::map_http_error("Failed to fetch release metadata", err))?;

        let body = response
            .into_string()
            .map_err(|err| LlaError::Plugin(format!("Failed to read release response: {}", err)))?;

        serde_json::from_str::<GithubRelease>(&body)
            .map_err(|err| LlaError::Plugin(format!("Failed to parse release metadata: {}", err)))
    }

    fn fetch_asset_checksum(
        agent: &Agent,
        release: &GithubRelease,
        asset_name: &str,
    ) -> Result<Option<String>> {
        let checksum_asset = release
            .assets
            .iter()
            .find(|asset| asset.name.eq_ignore_ascii_case("SHA256SUMS"));

        let Some(asset) = checksum_asset else {
            return Ok(None);
        };

        let response = Self::github_request(agent, &asset.browser_download_url)
            .call()
            .map_err(|err| Self::map_http_error("Failed to download checksum file", err))?;

        let content = response
            .into_string()
            .map_err(|err| LlaError::Plugin(format!("Failed to read checksum file: {}", err)))?;

        let checksum_line = content
            .lines()
            .find(|line| line.trim().ends_with(asset_name));

        Ok(checksum_line.map(|line| {
            let mut parts = line.split_whitespace();
            let checksum_part = parts.next().unwrap_or("");
            if let Some(idx) = checksum_part.rfind(':') {
                checksum_part[idx + 1..].to_string()
            } else {
                checksum_part.to_string()
            }
        }))
    }

    fn download_to_path(
        &self,
        agent: &Agent,
        url: &str,
        destination: &Path,
        ui: &InstallerUi,
    ) -> Result<u64> {
        let response = Self::github_request(agent, url)
            .call()
            .map_err(|err| Self::map_http_error("Failed to download archive", err))?;

        let content_length = response
            .header("content-length")
            .and_then(|h| h.parse::<u64>().ok());

        let asset_name = destination
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive");

        let download_message = ui.progress_message("Downloading", asset_name);
        let progress = self.create_download_progress(&download_message, content_length);

        let mut reader = response.into_reader();
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(destination)?;

        let mut buffer = [0u8; 8192];
        let mut total_bytes = 0u64;
        let start_time = Instant::now();

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            file.write_all(&buffer[..bytes_read])?;
            total_bytes += bytes_read as u64;

            if let Some(total) = content_length {
                progress.set_position(total_bytes);
                let speed = if start_time.elapsed().as_secs_f64() > 0.0 {
                    total_bytes as f64 / start_time.elapsed().as_secs_f64()
                } else {
                    0.0
                };

                let eta_text = if total_bytes < total && speed > 0.0 {
                    let remaining_bytes = total - total_bytes;
                    let eta_secs = remaining_bytes as f64 / speed;
                    format!(
                        " - {}",
                        InstallerUi::format_duration(Duration::from_secs(eta_secs as u64))
                    )
                } else {
                    String::new()
                };

                progress.set_message(format!(
                    "{} ({}){}",
                    download_message,
                    InstallerUi::format_speed(speed),
                    eta_text
                ));
            } else {
                progress.set_message(format!(
                    "{} ({})",
                    download_message,
                    InstallerUi::format_size(total_bytes)
                ));
            }
        }

        file.flush()?;

        let size_text = InstallerUi::format_size(total_bytes);
        let elapsed = start_time.elapsed();
        let speed_text = if elapsed.as_secs_f64() > 0.0 {
            format!(
                " at {}",
                InstallerUi::format_speed(total_bytes as f64 / elapsed.as_secs_f64())
            )
        } else {
            String::new()
        };

        let success_message = format!(
            "Downloaded {} {}{}",
            ui.highlight_text(asset_name),
            ui.muted_text(&size_text),
            ui.muted_text(&speed_text)
        );

        ui.complete_progress_standalone(&progress, StatusKind::Success, success_message);
        Ok(total_bytes)
    }

    fn calculate_sha256(path: &Path) -> Result<String> {
        let mut file = fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];

        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }

        let digest = hasher.finalize();
        Ok(format!("{:x}", digest))
    }

    fn write_package_checksums(package_dir: &Path) -> Result<()> {
        let manifest_path = package_dir.join("plugin.toml");
        let manifest_source = fs::read_to_string(&manifest_path).map_err(|error| {
            LlaError::Plugin(format!(
                "Failed to read plugin manifest {}: {error}",
                manifest_path.display()
            ))
        })?;
        let manifest: Value = toml::from_str(&manifest_source).map_err(|error| {
            LlaError::Plugin(format!(
                "Failed to parse plugin manifest {}: {error}",
                manifest_path.display()
            ))
        })?;
        let entrypoint = manifest
            .get("plugin")
            .and_then(|plugin| plugin.get("entrypoint"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                LlaError::Plugin(format!(
                    "Plugin manifest {} is missing plugin.entrypoint",
                    manifest_path.display()
                ))
            })?;
        let logical_entrypoint = Path::new(entrypoint);
        if logical_entrypoint.is_absolute()
            || !matches!(
                logical_entrypoint
                    .components()
                    .collect::<Vec<_>>()
                    .as_slice(),
                [std::path::Component::Normal(_)]
            )
        {
            return Err(LlaError::Plugin(format!(
                "Plugin manifest {} has an invalid package entrypoint '{}'",
                manifest_path.display(),
                entrypoint
            )));
        }

        let platform_entrypoint = format!(
            "{}{}{}",
            std::env::consts::DLL_PREFIX,
            entrypoint,
            std::env::consts::DLL_SUFFIX
        );
        let entrypoint_without_prefix = format!("{}{}", entrypoint, std::env::consts::DLL_SUFFIX);
        let entrypoint_path = [
            package_dir.join(entrypoint),
            package_dir.join(platform_entrypoint),
            package_dir.join(entrypoint_without_prefix),
        ]
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            LlaError::Plugin(format!(
                "Cannot find runtime entrypoint '{}' in plugin package {}",
                entrypoint,
                package_dir.display()
            ))
        })?;
        let entrypoint_name = entrypoint_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                LlaError::Plugin(format!(
                    "Plugin package entrypoint {} has an invalid file name",
                    entrypoint_path.display()
                ))
            })?;

        let mut files = BTreeMap::new();
        for (name, path) in [
            ("plugin.toml", manifest_path.as_path()),
            (entrypoint_name, entrypoint_path.as_path()),
        ] {
            if !path.is_file() {
                return Err(LlaError::Plugin(format!(
                    "Cannot checksum missing plugin package file {}",
                    path.display()
                )));
            }
            files.insert(name.to_string(), Self::calculate_sha256(path)?);
        }

        #[derive(Serialize)]
        struct Inventory {
            files: BTreeMap<String, String>,
        }

        let document = toml::to_string_pretty(&Inventory { files }).map_err(|error| {
            LlaError::Plugin(format!("Failed to serialize plugin checksums: {error}"))
        })?;
        let temporary = package_dir.join("checksums.toml.tmp");
        fs::write(&temporary, document)?;
        fs::rename(temporary, package_dir.join("checksums.toml"))?;
        Ok(())
    }

    fn extract_archive(archive_path: &Path, destination: &Path) -> Result<()> {
        fs::create_dir_all(destination)?;
        let extension = archive_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");

        if extension.eq_ignore_ascii_case("zip") {
            let file = fs::File::open(archive_path)?;
            let mut archive = ZipArchive::new(file).map_err(|err| {
                LlaError::Plugin(format!(
                    "Failed to read zip archive {:?}: {}",
                    archive_path, err
                ))
            })?;

            for index in 0..archive.len() {
                let mut entry = archive.by_index(index).map_err(|err| {
                    LlaError::Plugin(format!("Failed to read zip entry: {}", err))
                })?;

                let mut out_path = destination.to_path_buf();
                out_path.push(entry.mangled_name());

                if entry.name().ends_with('/') {
                    fs::create_dir_all(&out_path)?;
                    continue;
                }

                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }

                let mut outfile = fs::File::create(&out_path)?;
                std::io::copy(&mut entry, &mut outfile)?;

                #[cfg(unix)]
                if let Some(mode) = entry.unix_mode() {
                    fs::set_permissions(&out_path, fs::Permissions::from_mode(mode))?;
                }
            }
        } else {
            let file = fs::File::open(archive_path)?;
            let decoder = GzDecoder::new(file);
            let mut archive = Archive::new(decoder);
            archive.unpack(destination)?;
        }

        Ok(())
    }

    fn collect_prebuilt_plugin_files(target_dir: &Path, extension: &str) -> Result<Vec<PathBuf>> {
        let mut plugins = Vec::new();
        for entry in WalkDir::new(target_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| {
                        ext.eq_ignore_ascii_case(extension) || ext.eq_ignore_ascii_case("wasm")
                    })
                    .unwrap_or(false)
            {
                plugins.push(path.to_path_buf());
            }
        }
        plugins.sort();
        Ok(plugins)
    }

    #[cfg(feature = "dynamic-plugins")]
    fn load_prebuilt_plugin(path: &Path) -> Result<PrebuiltPlugin> {
        Self::verify_plugin_package(path)?;
        let manifest_path = path
            .parent()
            .map(|directory| directory.join("plugin.toml"))
            .unwrap_or_default();
        if manifest_path.is_file() {
            let manifest = PluginManifest::from_path(&manifest_path).map_err(LlaError::Plugin)?;
            if manifest.plugin.runtime == PluginRuntime::WasmComponent
                && !crate::plugin::wasm_runtime_supported(std::env::consts::ARCH)
            {
                return Err(LlaError::Plugin(format!(
                    "Plugin '{}' is a WASM component, but Wasmtime is unsupported on {}",
                    manifest.plugin.name,
                    std::env::consts::ARCH
                )));
            }
            if !manifest.supports_host_api(PLUGIN_API_VERSION) {
                return Err(LlaError::Plugin(format!(
                    "Plugin '{}' supports API {}..={} but lla uses API {}",
                    manifest.plugin.name,
                    manifest.plugin.api_min,
                    manifest.plugin.api_max,
                    PLUGIN_API_VERSION
                )));
            }
            // Permission expansion is checked before any package files are installed.
            // Native plugins remain trusted and therefore skip this prompt.
            // WASM grants are persisted atomically in plugin-grants.toml.
            //
            // This also makes unattended installs fail closed.
            //
            // `self` is not available in this associated loader, so the caller
            // performs the prompt immediately before copying the package.
            return Ok(PrebuiltPlugin {
                path: path.to_path_buf(),
                name: manifest.plugin.name,
                version: manifest.plugin.version,
                description: manifest.plugin.description,
            });
        }

        Err(LlaError::Plugin(format!(
            "Plugin package {:?} has no API v3 plugin.toml",
            path
        )))
    }

    #[cfg(feature = "dynamic-plugins")]
    fn verify_plugin_package(entrypoint: &Path) -> Result<()> {
        crate::plugin::package::verify_package_checksums(entrypoint)
            .and_then(|present| {
                present
                    .then_some(())
                    .ok_or_else(|| "API v3 package is missing checksums.toml".to_string())
            })
            .map_err(LlaError::Plugin)
    }

    #[cfg(not(feature = "dynamic-plugins"))]
    fn load_prebuilt_plugin(_path: &Path) -> Result<PrebuiltPlugin> {
        Err(LlaError::Plugin(
            crate::plugin::DYNAMIC_PLUGINS_UNAVAILABLE.to_string(),
        ))
    }

    fn select_prebuilt_plugins(&self, plugins: &[PrebuiltPlugin]) -> Result<Vec<PrebuiltPlugin>> {
        if plugins.is_empty() {
            return Err(LlaError::Plugin("No plugins found in archive".to_string()));
        }

        let ui = self.ui();

        if !atty::is(atty::Stream::Stdout) {
            return Ok(plugins.to_vec());
        }

        let max_name_width = plugins.iter().map(|p| p.name.len()).max().unwrap_or(0);
        let max_ver_width = plugins
            .iter()
            .map(|p| p.version.len() + 1)
            .max()
            .unwrap_or(0);

        ui.section("Select Plugins");

        let tw = terminal_size::terminal_size()
            .map(|(w, _)| w.0 as usize)
            .unwrap_or(80);
        // multiselect prefix (~8) + name + ver + separators
        let fixed_cols = 8 + max_name_width + 2 + max_ver_width + 2 + 1 + 2;
        let desc_budget = tw.saturating_sub(fixed_cols);

        let theme = LlaDialoguerTheme::default();
        let items: Vec<String> = plugins
            .iter()
            .map(|plugin| {
                let padded_name = format!("{:<width$}", plugin.name, width = max_name_width);
                let padded_ver = format!("v{:<width$}", plugin.version, width = max_ver_width - 1);
                let desc = Self::truncate_desc(&plugin.description, desc_budget);
                format!(
                    "{}  {}  {}  {}",
                    ui.accent_text(&padded_name),
                    ui.muted_text(&padded_ver),
                    ui.muted_text("│"),
                    desc
                )
            })
            .collect();

        let selections = MultiSelect::with_theme(&theme)
            .with_prompt("Select plugins to install")
            .items(&items)
            .defaults(&vec![true; items.len()])
            .interact_on(&Term::stderr())?;

        if selections.is_empty() {
            return Err(LlaError::Plugin("No plugins selected".to_string()));
        }

        Ok(selections
            .into_iter()
            .map(|index| plugins[index].clone())
            .collect())
    }

    fn install_prebuilt_plugin(
        &self,
        plugin: &PrebuiltPlugin,
        release_tag: &str,
        asset_name: &str,
        checksum: Option<&str>,
    ) -> Result<()> {
        fs::create_dir_all(&self.plugins_dir)?;

        let file_name = plugin.path.file_name().ok_or_else(|| {
            LlaError::Plugin(format!("Plugin {} has an invalid file name", plugin.name))
        })?;

        let manifest_source = plugin.path.parent().map(|dir| dir.join("plugin.toml"));
        #[cfg(feature = "dynamic-plugins")]
        if let Some(path) = manifest_source.as_ref().filter(|path| path.is_file()) {
            let manifest = PluginManifest::from_path(path).map_err(LlaError::Plugin)?;
            self.ensure_wasm_permissions(&manifest)?;
        }
        let is_v3_package = manifest_source.as_ref().is_some_and(|path| path.is_file());
        let package_dir = self.plugins_dir.join(&plugin.name);
        let destination = if is_v3_package {
            fs::create_dir_all(&package_dir)?;
            package_dir.join(file_name)
        } else {
            self.plugins_dir.join(file_name)
        };
        fs::copy(&plugin.path, &destination).map_err(|err| {
            LlaError::Plugin(format!(
                "Failed to copy plugin {} to {:?}: {}",
                plugin.name, destination, err
            ))
        })?;

        if let Some(manifest_source) = manifest_source.filter(|path| path.is_file()) {
            fs::copy(&manifest_source, package_dir.join("plugin.toml")).map_err(|err| {
                LlaError::Plugin(format!(
                    "Failed to install manifest for {}: {}",
                    plugin.name, err
                ))
            })?;
            let checksums_source = manifest_source.with_file_name("checksums.toml");
            if checksums_source.is_file() {
                fs::copy(checksums_source, package_dir.join("checksums.toml"))?;
            }
        }

        let metadata = PluginMetadata::new(
            plugin.name.clone(),
            plugin.version.clone(),
            PluginSource::Prebuilt {
                release_tag: release_tag.to_string(),
                asset: asset_name.to_string(),
                checksum: checksum.map(|value| value.to_string()),
            },
            None,
        );

        self.update_plugin_metadata(&plugin.name, metadata)
    }

    fn select_plugins(&self, plugin_dirs: &[PathBuf]) -> Result<Vec<PathBuf>> {
        if !atty::is(atty::Stream::Stdout) {
            return Ok(plugin_dirs.to_vec());
        }

        let ui = self.ui();

        let plugins_info: Vec<(String, String)> = plugin_dirs
            .iter()
            .map(|p| {
                let name = Self::get_display_name(p);
                let version = self
                    .get_plugin_version(p)
                    .unwrap_or_else(|_| "unknown".to_string());
                (name, version)
            })
            .collect();

        if plugins_info.is_empty() {
            return Err(LlaError::Plugin("No plugins found".to_string()));
        }

        let max_name_width = plugins_info.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
        let max_ver_width = plugins_info
            .iter()
            .map(|(_, v)| v.len() + 1)
            .max()
            .unwrap_or(0);

        ui.section("Select Plugins");

        let theme = LlaDialoguerTheme::default();
        let plugin_names: Vec<String> = plugins_info
            .iter()
            .map(|(name, version)| {
                let padded_name = format!("{:<width$}", name, width = max_name_width);
                let padded_ver = format!("v{:<width$}", version, width = max_ver_width - 1);
                format!(
                    "{}  {}",
                    ui.accent_text(&padded_name),
                    ui.muted_text(&padded_ver),
                )
            })
            .collect();

        let selections = MultiSelect::with_theme(&theme)
            .with_prompt("Select plugins to install")
            .items(&plugin_names)
            .defaults(&vec![false; plugin_names.len()])
            .interact_on(&Term::stderr())?;

        if selections.is_empty() {
            return Err(LlaError::Plugin("No plugins selected".to_string()));
        }

        Ok(selections
            .into_iter()
            .map(|i| plugin_dirs[i].clone())
            .collect())
    }

    pub fn install_from_prebuilt(&self) -> Result<()> {
        let ui = self.ui();
        ui.banner("Prebuilt Plugin Installation");

        let host = HostTarget::detect()?;
        let agent = Self::github_agent();
        let release = Self::fetch_release(None)?;

        let asset_candidates = host.asset_candidates();
        let asset = release
            .assets
            .iter()
            .find(|asset| {
                asset_candidates
                    .iter()
                    .any(|candidate| candidate == &asset.name)
            })
            .cloned()
            .ok_or_else(|| {
                LlaError::Plugin(format!(
                    "No prebuilt plugins available for {}-{}",
                    host.os_label, host.arch_label
                ))
            })?;

        let checksum = Self::fetch_asset_checksum(&agent, &release, &asset.name)?;

        let mut detail_lines = vec![
            format!(
                "  {}  {}  {}",
                ui.muted_text("Release "),
                ui.muted_text("│"),
                ui.highlight_text(&release.tag_name)
            ),
            format!(
                "  {}  {}  {}",
                ui.muted_text("Asset   "),
                ui.muted_text("│"),
                ui.highlight_text(&asset.name)
            ),
        ];
        if let Some(ref sum) = checksum {
            detail_lines.push(format!(
                "  {}  {}  {}",
                ui.muted_text("Checksum"),
                ui.muted_text("│"),
                ui.muted_text(sum)
            ));
        }
        let detail_box = BoxComponent::new(detail_lines.join("\n"))
            .style(BoxStyle::Rounded)
            .title(ui.accent_text("Release"))
            .padding(1)
            .render();
        ui.write_stdout(detail_box);

        let temp_dir = tempfile::tempdir()?;
        let archive_path = temp_dir.path().join(&asset.name);
        let extracted_dir = temp_dir.path().join("plugins");

        self.download_to_path(&agent, &asset.browser_download_url, &archive_path, &ui)?;

        if let Some(expected) = checksum.as_deref() {
            let verify_message = ui.progress_message("Verifying", "checksum");
            let verify_pb = self.create_spinner(&verify_message);
            let actual = Self::calculate_sha256(&archive_path)?;
            if actual.eq_ignore_ascii_case(expected) {
                let verified_msg = ui.muted_text("Checksum verified");
                ui.complete_progress_standalone(&verify_pb, StatusKind::Success, verified_msg);
            } else {
                let mismatch = format!(
                    "Checksum mismatch (expected {}, got {})",
                    ui.muted_text(expected),
                    ui.error_text(&actual)
                );
                ui.complete_progress_standalone(&verify_pb, StatusKind::Error, mismatch);
                return Err(LlaError::Plugin(format!(
                    "Checksum verification failed: expected {}, got {}",
                    expected, actual
                )));
            }
        }

        let extract_message = ui.progress_message("Extracting", &asset.name);
        let extract_pb = self.create_spinner(&extract_message);
        Self::extract_archive(&archive_path, &extracted_dir)?;
        let extracted_msg = ui.muted_text("Archive extracted");
        ui.complete_progress_standalone(&extract_pb, StatusKind::Success, extracted_msg);

        // Show a short, single-line spinner while discovering plugins in the extracted archive
        let discover_pb =
            self.create_status_spinner(&ui.progress_message("Discovering", "plugins"));
        let plugin_paths =
            Self::collect_prebuilt_plugin_files(&extracted_dir, host.library_extension)?;
        ui.complete_progress(&discover_pb);

        if plugin_paths.is_empty() {
            return Err(LlaError::Plugin(
                "Archive did not contain any plugins".to_string(),
            ));
        }

        let plugin_count = plugin_paths.len();
        ui.print_status(
            StatusKind::Info,
            format!(
                "Found {} {}",
                plugin_count,
                if plugin_count == 1 {
                    "plugin binary"
                } else {
                    "plugin binaries"
                }
            ),
        );

        let mut plugins = Vec::new();
        for path in plugin_paths {
            plugins.push(Self::load_prebuilt_plugin(&path)?);
        }

        let selected_plugins = self.select_prebuilt_plugins(&plugins)?;
        let selected_count = selected_plugins.len();
        ui.blank_line();
        ui.print_status(
            StatusKind::Info,
            format!(
                "Selected {} {}",
                selected_count,
                if selected_count == 1 {
                    "plugin"
                } else {
                    "plugins"
                }
            ),
        );

        let mut summary = InstallSummary::default();

        for plugin in selected_plugins.into_iter() {
            let spinner = if self.no_progress {
                None
            } else {
                let initial = ui.progress_message("Installing", &plugin.name);
                Some(self.create_build_progress(&initial))
            };

            match self.install_prebuilt_plugin(
                &plugin,
                &release.tag_name,
                &asset.name,
                checksum.as_deref(),
            ) {
                Ok(_) => {
                    if let Some(ref pb) = spinner {
                        let msg = format!(
                            "Installed {}",
                            ui.name_with_version(&plugin.name, &plugin.version)
                        );
                        ui.complete_progress_standalone(pb, StatusKind::Success, msg);
                    } else {
                        ui.print_status(
                            StatusKind::Success,
                            format!(
                                "Installed {}",
                                ui.name_with_version(&plugin.name, &plugin.version)
                            ),
                        );
                    }
                    summary.add_success(plugin.name.clone(), plugin.version.clone());
                }
                Err(err) => {
                    let error_text = err.to_string();
                    if let Some(ref pb) = spinner {
                        let msg = format!(
                            "{} {}",
                            ui.highlight_text(&plugin.name),
                            ui.error_text("install failed")
                        );
                        ui.complete_progress_standalone(pb, StatusKind::Error, msg);
                    } else {
                        ui.print_status(
                            StatusKind::Error,
                            format!(
                                "{} {}",
                                ui.highlight_text(&plugin.name),
                                ui.error_text("install failed")
                            ),
                        );
                    }
                    summary.add_failure(plugin.name.clone(), error_text);
                }
            }
        }

        ui.blank_line();
        summary.display(&ui);

        if summary.failed.is_empty() {
            Ok(())
        } else {
            Err(LlaError::Plugin(format!(
                "{}/{} plugins failed to install",
                summary.failed.len(),
                summary.failed.len() + summary.successful.len()
            )))
        }
    }

    pub fn migrate_prebuilt(&self) -> Result<()> {
        fs::create_dir_all(&self.plugins_dir)?;
        let metadata_path = self.plugins_dir.join("metadata.toml");
        let grants_path = self.plugins_dir.join("plugin-grants.toml");
        let metadata_snapshot = fs::read(&metadata_path).ok();
        let grants_snapshot = fs::read(&grants_path).ok();
        let metadata = self.load_metadata_store()?;
        let official = metadata
            .plugins
            .iter()
            .filter_map(|(name, metadata)| {
                matches!(metadata.source, PluginSource::Prebuilt { .. }).then_some(name.as_str())
            })
            .collect::<std::collections::HashSet<_>>();
        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let backup_dir = self.plugins_dir.join(".legacy").join(timestamp);
        let mut backups = Vec::<(PathBuf, PathBuf)>::new();
        let mut unmatched = Vec::new();

        let mut entries = fs::read_dir(&self.plugins_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort();
        let original_paths = entries
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        for path in entries {
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if matches!(
                file_name,
                "metadata.toml" | "plugin-grants.toml" | ".legacy"
            ) {
                continue;
            }
            let candidate = if path.is_dir() {
                file_name.to_string()
            } else {
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .map(|name| name.strip_prefix("lib").unwrap_or(name).to_string())
                    .unwrap_or_default()
            };
            if official.contains(candidate.as_str()) {
                fs::create_dir_all(&backup_dir)?;
                let destination = backup_dir.join(file_name);
                fs::rename(&path, &destination)?;
                backups.push((path, destination));
            } else {
                unmatched.push(candidate);
            }
        }

        match self.install_from_prebuilt() {
            Ok(()) => {
                println!(
                    "Migrated {} official plugin artifact(s); legacy files are preserved in {}",
                    backups.len(),
                    backup_dir.display()
                );
                unmatched.sort();
                unmatched.dedup();
                if !unmatched.is_empty() {
                    println!(
                        "Unmatched third-party plugins left untouched: {}",
                        unmatched.join(", ")
                    );
                }
                Ok(())
            }
            Err(error) => {
                let failed_dir = backup_dir.join("failed-v3-install");
                for (original, backup) in backups.iter().rev() {
                    if original.exists() {
                        fs::create_dir_all(&failed_dir)?;
                        let name = original.file_name().unwrap();
                        fs::rename(original, failed_dir.join(name))?;
                    }
                    if backup.exists() {
                        fs::rename(backup, original)?;
                    }
                }
                for entry in fs::read_dir(&self.plugins_dir)? {
                    let path = entry?.path();
                    let name = path.file_name().and_then(|name| name.to_str());
                    if matches!(
                        name,
                        Some("metadata.toml" | "plugin-grants.toml" | ".legacy")
                    ) || original_paths.contains(&path)
                    {
                        continue;
                    }
                    fs::create_dir_all(&failed_dir)?;
                    let file_name = path.file_name().ok_or_else(|| {
                        LlaError::Plugin("partial migration artifact has no file name".to_string())
                    })?;
                    fs::rename(&path, failed_dir.join(file_name))?;
                }
                for (path, snapshot) in [
                    (&metadata_path, metadata_snapshot.as_deref()),
                    (&grants_path, grants_snapshot.as_deref()),
                ] {
                    if let Some(snapshot) = snapshot {
                        fs::write(path, snapshot)?;
                    } else if path.exists() {
                        fs::remove_file(path)?;
                    }
                }
                Err(LlaError::Plugin(format!(
                    "Plugin migration failed and legacy artifacts were restored: {error}"
                )))
            }
        }
    }

    pub fn install_from_git(&self, url: &str) -> Result<()> {
        let ui = self.ui();
        ui.banner("Git Installation");

        let repo_line = format!(
            "  {}  {}  {}",
            ui.muted_text("Repository"),
            ui.muted_text("│"),
            ui.highlight_text(url)
        );
        let repo_box = BoxComponent::new(repo_line)
            .style(BoxStyle::Rounded)
            .title(ui.accent_text("Source"))
            .padding(1)
            .render();
        ui.write_stdout(repo_box);

        let repo_name = url
            .split('/')
            .next_back()
            .ok_or_else(|| LlaError::Plugin(format!("Invalid GitHub URL: {}", url)))?
            .trim_end_matches(".git");

        let clone_message = ui.progress_message("Cloning", repo_name);
        let clone_pb = self.create_status_spinner(&clone_message);

        let temp_dir = tempfile::tempdir()?;
        let start_time = Instant::now();

        let mut child = Command::new("git")
            .args(["clone", "--progress", url])
            .current_dir(&temp_dir)
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let status = child.wait()?;
        let elapsed = start_time.elapsed();

        if !status.success() {
            let message = format!(
                "{} {}",
                ui.highlight_text(repo_name),
                ui.error_text("Clone failed")
            );
            ui.complete_progress_standalone(&clone_pb, StatusKind::Error, message);
            return Err(LlaError::Plugin("Failed to clone repository".to_string()));
        }

        let cloned_msg = format!(
            "Cloned {} {}",
            ui.highlight_text(repo_name),
            ui.muted_text(&format!("in {}", InstallerUi::format_duration(elapsed)))
        );
        ui.complete_progress_standalone(&clone_pb, StatusKind::Success, cloned_msg);

        self.install_plugins(&temp_dir.path().join(repo_name), Some((repo_name, url)))
    }

    pub fn install_from_directory(&self, dir: &str) -> Result<()> {
        let ui = self.ui();
        ui.banner("Local Installation");

        let dir_line = format!(
            "  {}  {}  {}",
            ui.muted_text("Directory"),
            ui.muted_text("│"),
            ui.highlight_text(dir)
        );
        let dir_box = BoxComponent::new(dir_line)
            .style(BoxStyle::Rounded)
            .title(ui.accent_text("Source"))
            .padding(1)
            .render();
        ui.write_stdout(dir_box);

        let source_dir = PathBuf::from(dir.trim_end_matches('/'))
            .canonicalize()
            .map_err(|_| LlaError::Plugin(format!("Directory not found: {}", dir)))?;

        if !source_dir.exists() || !source_dir.is_dir() {
            return Err(LlaError::Plugin(format!("Not a valid directory: {}", dir)));
        }

        self.install_plugins(&source_dir, None)
    }

    fn is_workspace_member(&self, plugin_dir: &Path, silent: bool) -> Result<Option<PathBuf>> {
        let mut current_dir = plugin_dir.to_path_buf();
        let plugin_name = Self::get_display_name(plugin_dir);
        let ui = self.ui();

        while let Some(parent) = current_dir.parent() {
            let workspace_cargo = parent.join("Cargo.toml");
            if workspace_cargo.exists() {
                if let Ok(contents) = fs::read_to_string(&workspace_cargo) {
                    if contents.contains("[workspace]") {
                        if let Ok(rel_path) = plugin_dir.strip_prefix(parent) {
                            let rel_path_str = rel_path.to_string_lossy();

                            if contents.contains(&format!("\"{}\"", rel_path_str))
                                || contents.contains(&format!("'{}'", rel_path_str))
                            {
                                if !silent {
                                    ui.print_status(
                                        StatusKind::Info,
                                        ui.muted_text("Workspace member detected"),
                                    );
                                    ui.print_status(
                                        StatusKind::Success,
                                        format!(
                                            "{} in {}",
                                            ui.highlight_text(&plugin_name),
                                            ui.muted_text(&parent.display().to_string())
                                        ),
                                    );
                                }
                                return Ok(Some(parent.to_path_buf()));
                            }
                            if contents.contains("members = [") {
                                let patterns = [
                                    format!(
                                        "\"{}/*\"",
                                        rel_path_str.split('/').next().unwrap_or("")
                                    ),
                                    format!("'{}/*'", rel_path_str.split('/').next().unwrap_or("")),
                                    format!(
                                        "\"{}/\"",
                                        rel_path_str.split('/').next().unwrap_or("")
                                    ),
                                    format!("'{}/", rel_path_str.split('/').next().unwrap_or("")),
                                ];

                                for pattern in patterns {
                                    if contents.contains(&pattern) {
                                        if !silent {
                                            ui.print_status(
                                                StatusKind::Info,
                                                ui.muted_text("Workspace member detected"),
                                            );
                                            ui.print_status(
                                                StatusKind::Success,
                                                format!(
                                                    "{} matches {}",
                                                    ui.highlight_text(&plugin_name),
                                                    ui.muted_text(&pattern)
                                                ),
                                            );
                                        }
                                        return Ok(Some(parent.to_path_buf()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            current_dir = parent.to_path_buf();
        }
        if !silent {
            ui.print_status(
                StatusKind::Info,
                format!(
                    "{} will be built independently",
                    ui.highlight_text(&plugin_name)
                ),
            );
        }
        Ok(None)
    }

    fn get_display_name(path: &Path) -> String {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    }

    fn find_plugin_directories(&self, root_dir: &Path) -> Result<Vec<PathBuf>> {
        let ui = self.ui();
        let mut plugin_dirs = Vec::new();
        let mut found_plugins = Vec::new();

        let workspace_cargo = root_dir.join("Cargo.toml");
        if workspace_cargo.exists() {
            if let Ok(contents) = fs::read_to_string(&workspace_cargo) {
                if contents.contains("[workspace]") {
                    for entry in WalkDir::new(root_dir)
                        .follow_links(true)
                        .min_depth(1)
                        .max_depth(3)
                        .into_iter()
                        .filter_map(|e| e.ok())
                    {
                        let path = entry.path();
                        if path.is_dir() {
                            let cargo_toml = path.join("Cargo.toml");
                            if cargo_toml.exists() {
                                if let Ok(contents) = fs::read_to_string(&cargo_toml) {
                                    if contents.contains("lla_plugin_interface")
                                        || contents.contains("lla_plugin_sdk")
                                    {
                                        let name = Self::get_display_name(path);
                                        if name != "lla_plugin_interface" {
                                            if let Ok(version) = self.get_plugin_version(path) {
                                                found_plugins
                                                    .push(format!("{} v{}", name, version));
                                                plugin_dirs.push(path.to_path_buf());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !found_plugins.is_empty() {
                        let list = found_plugins.join(", ");
                        ui.print_status(
                            StatusKind::Info,
                            format!("Found plugins: {}", ui.muted_text(&list)),
                        );
                        return Ok(plugin_dirs);
                    }
                }
            }
        }

        for entry in WalkDir::new(root_dir)
            .follow_links(true)
            .min_depth(0)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_dir() {
                let cargo_toml = path.join("Cargo.toml");
                if cargo_toml.exists() {
                    if let Ok(contents) = fs::read_to_string(&cargo_toml) {
                        if contents.contains("lla_plugin_interface")
                            || contents.contains("lla_plugin_sdk")
                        {
                            let name = Self::get_display_name(path);
                            if name != "lla_plugin_interface" {
                                if let Ok(version) = self.get_plugin_version(path) {
                                    found_plugins.push(format!("{} v{}", name, version));
                                    plugin_dirs.push(path.to_path_buf());
                                }
                            }
                        }
                    }
                }
            }
        }

        if !found_plugins.is_empty() {
            let list = found_plugins.join(", ");
            ui.print_status(
                StatusKind::Info,
                format!("Found plugins: {}", ui.muted_text(&list)),
            );
        }

        Ok(plugin_dirs)
    }

    fn find_plugin_files(&self, target_dir: &Path, plugin_name: &str) -> Result<Vec<PathBuf>> {
        let mut plugin_files = Vec::new();
        if let Ok(entries) = target_dir.read_dir() {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let is_plugin = match std::env::consts::OS {
                    "macos" => file_name.contains(plugin_name) && file_name.ends_with(".dylib"),
                    "linux" => file_name.contains(plugin_name) && file_name.ends_with(".so"),
                    "windows" => file_name.contains(plugin_name) && file_name.ends_with(".dll"),
                    _ => false,
                };

                if is_plugin {
                    plugin_files.push(path);
                }
            }
        }
        Ok(plugin_files)
    }

    fn build_and_install_plugin(
        &self,
        plugin_dir: &Path,
        pb: Option<&ProgressBar>,
        _base_progress: Option<u64>,
    ) -> Result<()> {
        let manifest_source = plugin_dir.join("plugin.toml");
        #[cfg(feature = "dynamic-plugins")]
        let manifest = manifest_source
            .is_file()
            .then(|| PluginManifest::from_path(&manifest_source).map_err(LlaError::Plugin))
            .transpose()?;
        #[cfg(feature = "dynamic-plugins")]
        let wasm_component = manifest
            .as_ref()
            .is_some_and(|manifest| manifest.plugin.runtime == PluginRuntime::WasmComponent);
        #[cfg(not(feature = "dynamic-plugins"))]
        let wasm_component = false;
        if wasm_component && !crate::plugin::wasm_runtime_supported(std::env::consts::ARCH) {
            return Err(LlaError::Plugin(format!(
                "WASM component plugins are unsupported on {}",
                std::env::consts::ARCH
            )));
        }
        #[cfg(feature = "dynamic-plugins")]
        let plugin_name = manifest
            .as_ref()
            .map(|manifest| manifest.plugin.name.clone())
            .unwrap_or_else(|| Self::get_display_name(plugin_dir));
        #[cfg(not(feature = "dynamic-plugins"))]
        let plugin_name = Self::get_display_name(plugin_dir);
        let package_name = self.get_plugin_package_name(plugin_dir)?;

        // Silent mode when using progress bars to avoid stdout interference
        let silent = pb.is_some();
        let workspace_info = self.is_workspace_member(plugin_dir, silent)?;
        let ui = self.ui();

        let (build_dir, mut build_args) = match workspace_info {
            Some(workspace_root) => {
                if let Some(pb) = pb {
                    let message = format!(
                        "{} {}",
                        ui.progress_message("Building", &plugin_name),
                        ui.muted_text("(workspace)")
                    );
                    pb.set_message(message);
                }
                (
                    workspace_root,
                    vec![
                        "build".to_string(),
                        "--release".to_string(),
                        "-p".to_string(),
                        package_name.clone(),
                    ],
                )
            }
            None => {
                if let Some(pb) = pb {
                    pb.set_message(ui.progress_message("Building", &plugin_name));
                }
                (
                    plugin_dir.to_path_buf(),
                    vec!["build".to_string(), "--release".to_string()],
                )
            }
        };
        if wasm_component {
            build_args.extend(["--target".to_string(), "wasm32-wasip2".to_string()]);
        }

        let start_time = Instant::now();
        let mut build_command = if wasm_component {
            let target_status = Command::new("rustup")
                .args(["target", "add", "wasm32-wasip2", "--toolchain", "stable"])
                .status()
                .map_err(|error| {
                    LlaError::Plugin(format!(
                        "rustup is required to build WASM component plugins: {error}"
                    ))
                })?;
            if !target_status.success() {
                return Err(LlaError::Plugin(
                    "failed to install the wasm32-wasip2 Rust target".to_string(),
                ));
            }
            let rustc = Command::new("rustup")
                .args(["which", "rustc", "--toolchain", "stable"])
                .output()
                .map_err(|error| {
                    LlaError::Plugin(format!("failed to locate rustup's stable rustc: {error}"))
                })?;
            if !rustc.status.success() {
                return Err(LlaError::Plugin(
                    "failed to locate rustup's stable rustc".to_string(),
                ));
            }
            let rustc = String::from_utf8(rustc.stdout)
                .map_err(|error| LlaError::Plugin(error.to_string()))?;
            let mut command = Command::new("rustup");
            command
                .args(["run", "stable", "cargo"])
                .env("RUSTC", rustc.trim());
            command
        } else {
            Command::new("cargo")
        };
        #[cfg(target_os = "macos")]
        if !wasm_component {
            // rustc's Mach-O stripping can produce malformed LINKEDIT string pools
            // for cdylibs. This matches the release packaging path.
            build_command.env("CARGO_PROFILE_RELEASE_STRIP", "none");
        }
        let mut child = build_command
            .args(&build_args)
            .current_dir(&build_dir)
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        if let Some(pb) = pb {
            if let Some(stderr) = child.stderr.take() {
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines().map_while(std::result::Result::ok) {
                    if line.trim().starts_with("Compiling") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let package_info = parts[1..].join(" ");
                            pb.set_message(format!(
                                "{} {}",
                                ui.progress_message("Building", &plugin_name),
                                ui.muted_text(&format!("({})", package_info))
                            ));
                        }
                    } else if line.trim().starts_with("Finished") {
                        pb.set_message(ui.progress_message("Finalizing", &plugin_name));
                    }
                }
            }
        }

        let status = child.wait()?;
        let build_elapsed = start_time.elapsed();

        if !status.success() {
            if pb.is_none() {
                ui.print_status(
                    StatusKind::Error,
                    format!(
                        "{} {}",
                        ui.highlight_text(&plugin_name),
                        ui.error_text("Build failed")
                    ),
                );
            }
            return Err(LlaError::Plugin(format!(
                "Build failed for plugin '{}'",
                plugin_name
            )));
        }

        let target_dir = if wasm_component {
            build_dir
                .join("target")
                .join("wasm32-wasip2")
                .join("release")
        } else {
            build_dir.join("target").join("release")
        };
        #[cfg(feature = "dynamic-plugins")]
        let plugin_files = if let Some(manifest) = manifest.as_ref().filter(|_| wasm_component) {
            let entrypoint = target_dir.join(&manifest.plugin.entrypoint);
            if entrypoint.is_file() {
                vec![entrypoint]
            } else {
                return Err(LlaError::Plugin(format!(
                    "WASM build did not produce manifest entrypoint {}",
                    entrypoint.display()
                )));
            }
        } else {
            self.find_plugin_files(&target_dir, &package_name.replace('-', "_"))?
        };
        #[cfg(not(feature = "dynamic-plugins"))]
        let plugin_files = self.find_plugin_files(&target_dir, &package_name.replace('-', "_"))?;

        if plugin_files.is_empty() {
            return Err(LlaError::Plugin(format!(
                "No plugin files found for '{}'",
                plugin_name
            )));
        }

        if let Some(pb) = pb {
            pb.set_message(ui.progress_message("Installing", &plugin_name));
        }

        fs::create_dir_all(&self.plugins_dir)?;
        let destination_dir = if manifest_source.is_file() {
            #[cfg(feature = "dynamic-plugins")]
            {
                let manifest = manifest.as_ref().expect("manifest was parsed above");
                self.ensure_wasm_permissions(manifest)?;
            }
            let destination = self.plugins_dir.join(&plugin_name);
            fs::create_dir_all(&destination)?;
            fs::copy(&manifest_source, destination.join("plugin.toml"))?;
            destination
        } else {
            self.plugins_dir.clone()
        };

        for plugin_file in plugin_files.iter() {
            let dest_path = destination_dir.join(plugin_file.file_name().unwrap());
            fs::copy(plugin_file, &dest_path)?;
        }

        if manifest_source.is_file() {
            Self::write_package_checksums(&destination_dir)?;
        }

        // When progress bar is provided, leave it active for the caller to complete
        // with appropriate status. When no progress bar, print success directly.
        if pb.is_none() {
            ui.print_status(
                StatusKind::Success,
                format!(
                    "Built and installed {} {}",
                    ui.highlight_text(&plugin_name),
                    ui.muted_text(&format!(
                        "in {}",
                        InstallerUi::format_duration(build_elapsed)
                    ))
                ),
            );
        }
        Ok(())
    }

    fn install_plugins(&self, root_dir: &Path, repo_info: Option<(&str, &str)>) -> Result<()> {
        let plugin_dirs = self.find_plugin_directories(root_dir)?;
        if plugin_dirs.is_empty() {
            return Err(LlaError::Plugin(format!(
                "No plugins found in {:?}",
                root_dir
            )));
        }

        let selected_plugins = self.select_plugins(&plugin_dirs)?;
        let mut summary = InstallSummary::default();
        let total_plugins = selected_plugins.len();
        let ui = self.ui();

        ui.blank_line();
        ui.print_status(
            StatusKind::Info,
            format!(
                "Selected {} {}",
                total_plugins,
                if total_plugins == 1 {
                    "plugin"
                } else {
                    "plugins"
                }
            ),
        );

        for plugin_dir in selected_plugins.iter() {
            #[cfg(feature = "dynamic-plugins")]
            let manifest = plugin_dir
                .join("plugin.toml")
                .is_file()
                .then(|| PluginManifest::from_path(&plugin_dir.join("plugin.toml")))
                .transpose()
                .map_err(LlaError::Plugin)?;
            #[cfg(feature = "dynamic-plugins")]
            let plugin_name = manifest
                .as_ref()
                .map(|manifest| manifest.plugin.name.clone())
                .unwrap_or_else(|| Self::get_display_name(plugin_dir));
            #[cfg(not(feature = "dynamic-plugins"))]
            let plugin_name = Self::get_display_name(plugin_dir);
            let spinner = if self.no_progress {
                None
            } else {
                let initial = ui.progress_message("Building", &plugin_name);
                Some(self.create_build_progress(&initial))
            };

            match self.build_and_install_plugin(plugin_dir, spinner.as_ref(), None) {
                Ok(_) => {
                    #[cfg(feature = "dynamic-plugins")]
                    let version = manifest
                        .as_ref()
                        .map(|manifest| manifest.plugin.version.clone())
                        .unwrap_or(self.get_plugin_version(plugin_dir)?);
                    #[cfg(not(feature = "dynamic-plugins"))]
                    let version = self.get_plugin_version(plugin_dir)?;
                    let metadata = if let Some((repo_name, url)) = repo_info {
                        PluginMetadata::new(
                            plugin_name.clone(),
                            version.clone(),
                            PluginSource::Git {
                                url: url.to_string(),
                            },
                            Some(repo_name.to_string()),
                        )
                    } else {
                        let canonical_path = plugin_dir.canonicalize().map_err(|e| {
                            LlaError::Plugin(format!("Failed to resolve plugin path: {}", e))
                        })?;
                        PluginMetadata::new(
                            plugin_name.clone(),
                            version.clone(),
                            PluginSource::Local {
                                directory: canonical_path.to_string_lossy().into_owned(),
                            },
                            None,
                        )
                    };

                    if let Err(e) = self.update_plugin_metadata(&plugin_name, metadata) {
                        let error_text = format!("metadata error: {}", e);
                        summary.add_failure(plugin_name.clone(), error_text.clone());
                        if let Some(ref pb) = spinner {
                            let msg = format!(
                                "{} {}",
                                ui.highlight_text(&plugin_name),
                                ui.error_text("metadata update failed")
                            );
                            ui.complete_progress_standalone(pb, StatusKind::Error, msg);
                        } else {
                            ui.print_status(
                                StatusKind::Error,
                                format!(
                                    "{} {}",
                                    ui.highlight_text(&plugin_name),
                                    ui.error_text("metadata update failed")
                                ),
                            );
                        }
                    } else {
                        summary.add_success(plugin_name.clone(), version.clone());
                        if let Some(ref pb) = spinner {
                            let msg = format!(
                                "Installed {}",
                                ui.name_with_version(&plugin_name, &version)
                            );
                            ui.complete_progress_standalone(pb, StatusKind::Success, msg);
                        } else {
                            ui.print_status(
                                StatusKind::Success,
                                format!(
                                    "Installed {}",
                                    ui.name_with_version(&plugin_name, &version)
                                ),
                            );
                        }
                    }
                }
                Err(e) => {
                    let error_text = e.to_string();
                    summary.add_failure(plugin_name.clone(), error_text.clone());
                    if let Some(ref pb) = spinner {
                        let msg = format!(
                            "{} {}",
                            ui.highlight_text(&plugin_name),
                            ui.error_text("build failed")
                        );
                        ui.complete_progress_standalone(pb, StatusKind::Error, msg);
                    } else {
                        ui.print_status(
                            StatusKind::Error,
                            format!(
                                "{} {}",
                                ui.highlight_text(&plugin_name),
                                ui.error_text("build failed")
                            ),
                        );
                    }
                }
            }
        }

        ui.blank_line();
        summary.display(&ui);

        if !summary.failed.is_empty() {
            Err(LlaError::Plugin(format!(
                "{}/{} plugins failed to install",
                summary.failed.len(),
                total_plugins
            )))
        } else {
            Ok(())
        }
    }

    pub fn update_plugins(&self, plugin_name: Option<&str>) -> Result<()> {
        let store = self.load_metadata_store()?;
        if store.plugins.is_empty() {
            return Err(LlaError::Plugin(
                "No plugins are currently installed".to_string(),
            ));
        }

        let plugins: Vec<_> = if let Some(name) = plugin_name {
            store.plugins.iter().filter(|(n, _)| *n == name).collect()
        } else {
            store.plugins.iter().collect()
        };

        if plugins.is_empty() {
            return Err(LlaError::Plugin(format!(
                "Plugin '{}' not found",
                plugin_name.unwrap_or_default()
            )));
        }

        let ui = self.ui();
        let plugin_count = plugins.len();

        let header_content = format!(
            "  Checking {} {} for updates.",
            plugin_count,
            if plugin_count == 1 {
                "plugin"
            } else {
                "plugins"
            }
        );
        let header = BoxComponent::new(header_content)
            .style(BoxStyle::Rounded)
            .title(ui.accent_text("Plugin Update"))
            .padding(1)
            .render();
        ui.write_stdout(header);

        // Track results for summary
        let mut updated: Vec<(String, String, String)> = Vec::new(); // (name, old, new)
        let mut up_to_date: Vec<(String, String)> = Vec::new(); // (name, version)
        let mut failed: Vec<(String, String)> = Vec::new(); // (name, error)
        let mut prebuilt: Vec<String> = Vec::new();

        for (name, metadata) in plugins.into_iter() {
            let plugin_label = ui.highlight_text(name);

            let spinner = if self.no_progress {
                None
            } else {
                let initial = ui.progress_message("Updating", name);
                Some(self.create_build_progress(&initial))
            };

            match &metadata.source {
                PluginSource::Git { url } => {
                    let temp_dir = match tempfile::tempdir() {
                        Ok(dir) => dir,
                        Err(e) => {
                            let err_msg = format!("temp directory error: {}", e);
                            if let Some(ref pb) = spinner {
                                let msg = format!("{} {}", plugin_label, ui.error_text(&err_msg));
                                ui.complete_progress_standalone(pb, StatusKind::Error, msg);
                            } else {
                                ui.print_status(
                                    StatusKind::Error,
                                    format!("{} {}", plugin_label, ui.error_text(&err_msg)),
                                );
                            }
                            failed.push((name.clone(), err_msg));
                            continue;
                        }
                    };

                    let output = Command::new("git")
                        .args(["clone", "--quiet", url])
                        .current_dir(&temp_dir)
                        .output()?;

                    if !output.status.success() {
                        let err_msg = "Failed to clone repository".to_string();
                        if let Some(ref pb) = spinner {
                            let msg = format!("{} {}", plugin_label, ui.error_text(&err_msg));
                            ui.complete_progress_standalone(pb, StatusKind::Error, msg);
                        } else {
                            ui.print_status(
                                StatusKind::Error,
                                format!("{} {}", plugin_label, ui.error_text(&err_msg)),
                            );
                        }
                        failed.push((name.clone(), err_msg));
                        continue;
                    }

                    let repo_name = url
                        .split('/')
                        .next_back()
                        .map(|n| n.trim_end_matches(".git"))
                        .unwrap_or(name);

                    let repo_dir = temp_dir.path().join(repo_name);
                    let plugin_dirs = self.find_plugin_directories(&repo_dir)?;

                    let Some(plugin_dir) = plugin_dirs.iter().find(|dir| {
                        dir.file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n == name)
                            .unwrap_or(false)
                    }) else {
                        let err_msg = "Plugin not found in repository".to_string();
                        if let Some(ref pb) = spinner {
                            let msg = format!("{} {}", plugin_label, ui.error_text(&err_msg));
                            ui.complete_progress_standalone(pb, StatusKind::Error, msg);
                        } else {
                            ui.print_status(
                                StatusKind::Error,
                                format!("{} {}", plugin_label, ui.error_text(&err_msg)),
                            );
                        }
                        failed.push((name.clone(), err_msg));
                        continue;
                    };

                    match self.build_and_install_plugin(plugin_dir, spinner.as_ref(), None) {
                        Ok(_) => {
                            let new_version = self.get_plugin_version(plugin_dir)?;
                            let mut updated_metadata = metadata.clone();

                            let (kind, message) = if new_version != metadata.version {
                                updated.push((
                                    name.clone(),
                                    metadata.version.clone(),
                                    new_version.clone(),
                                ));
                                (
                                    StatusKind::Success,
                                    format!(
                                        "{} {}",
                                        plugin_label,
                                        ui.muted_text(&format!(
                                            "{} → {}",
                                            metadata.version, new_version
                                        ))
                                    ),
                                )
                            } else {
                                up_to_date.push((name.clone(), new_version.clone()));
                                (
                                    StatusKind::Info,
                                    format!(
                                        "{} {}",
                                        plugin_label,
                                        ui.muted_text(&format!("already at v{}", new_version))
                                    ),
                                )
                            };

                            updated_metadata.version = new_version;
                            updated_metadata.update_timestamp();
                            self.update_plugin_metadata(name, updated_metadata)?;

                            if let Some(ref pb) = spinner {
                                ui.complete_progress_standalone(pb, kind, message);
                            } else {
                                ui.print_status(kind, message);
                            }
                        }
                        Err(e) => {
                            let err_msg = e.to_string();
                            if let Some(ref pb) = spinner {
                                let msg = format!("{} {}", plugin_label, ui.error_text(&err_msg));
                                ui.complete_progress_standalone(pb, StatusKind::Error, msg);
                            } else {
                                ui.print_status(
                                    StatusKind::Error,
                                    format!("{} {}", plugin_label, ui.error_text(&err_msg)),
                                );
                            }
                            failed.push((name.clone(), err_msg));
                        }
                    }
                }
                PluginSource::Local { directory } => {
                    let source_dir = PathBuf::from(directory);

                    if !source_dir.exists() {
                        let err_msg = "Source directory not found".to_string();
                        if let Some(ref pb) = spinner {
                            let msg = format!("{} {}", plugin_label, ui.error_text(&err_msg));
                            ui.complete_progress_standalone(pb, StatusKind::Error, msg);
                        } else {
                            ui.print_status(
                                StatusKind::Error,
                                format!("{} {}", plugin_label, ui.error_text(&err_msg)),
                            );
                        }
                        failed.push((name.clone(), err_msg));
                        continue;
                    }

                    match self.build_and_install_plugin(&source_dir, spinner.as_ref(), None) {
                        Ok(_) => {
                            let new_version = self.get_plugin_version(&source_dir)?;
                            let mut updated_metadata = metadata.clone();

                            let (kind, message) = if new_version != metadata.version {
                                updated.push((
                                    name.clone(),
                                    metadata.version.clone(),
                                    new_version.clone(),
                                ));
                                (
                                    StatusKind::Success,
                                    format!(
                                        "{} {}",
                                        plugin_label,
                                        ui.muted_text(&format!(
                                            "{} → {}",
                                            metadata.version, new_version
                                        ))
                                    ),
                                )
                            } else {
                                up_to_date.push((name.clone(), new_version.clone()));
                                (
                                    StatusKind::Info,
                                    format!(
                                        "{} {}",
                                        plugin_label,
                                        ui.muted_text(&format!("already at v{}", new_version))
                                    ),
                                )
                            };

                            updated_metadata.version = new_version;
                            updated_metadata.update_timestamp();
                            self.update_plugin_metadata(name, updated_metadata)?;

                            if let Some(ref pb) = spinner {
                                ui.complete_progress_standalone(pb, kind, message);
                            } else {
                                ui.print_status(kind, message);
                            }
                        }
                        Err(e) => {
                            let err_msg = e.to_string();
                            if let Some(ref pb) = spinner {
                                let msg = format!("{} {}", plugin_label, ui.error_text(&err_msg));
                                ui.complete_progress_standalone(pb, StatusKind::Error, msg);
                            } else {
                                ui.print_status(
                                    StatusKind::Error,
                                    format!("{} {}", plugin_label, ui.error_text(&err_msg)),
                                );
                            }
                            failed.push((name.clone(), err_msg));
                        }
                    }
                }
                PluginSource::Prebuilt { .. } => {
                    let msg = format!(
                        "{} {}",
                        plugin_label,
                        ui.muted_text("prebuilt · run `lla install --prebuilt` to refresh")
                    );
                    if let Some(ref pb) = spinner {
                        ui.complete_progress_standalone(pb, StatusKind::Info, msg);
                    } else {
                        ui.print_status(StatusKind::Info, msg);
                    }
                    prebuilt.push(name.clone());
                }
            }
        }

        // Render update summary
        let mut summary_lines: Vec<String> = Vec::new();

        if !updated.is_empty() {
            summary_lines.push(format!(
                "  {} {}",
                ui.stylize("●", ui.success_color(), true),
                ui.stylize(
                    &format!("Updated ({})", updated.len()),
                    ui.success_color(),
                    true,
                )
            ));
            summary_lines.push(String::new());
            for (name, old_ver, new_ver) in &updated {
                summary_lines.push(format!(
                    "      {}  {}",
                    ui.accent_text(name),
                    ui.muted_text(&format!("v{} → v{}", old_ver, new_ver))
                ));
            }
        }

        if !updated.is_empty() && !up_to_date.is_empty() {
            summary_lines.push(String::new());
        }

        if !up_to_date.is_empty() {
            summary_lines.push(format!(
                "  {} {}",
                ui.muted_text("●"),
                ui.muted_text(&format!("Up to date ({})", up_to_date.len()))
            ));
            summary_lines.push(String::new());
            for (name, ver) in &up_to_date {
                summary_lines.push(format!(
                    "      {}  {}",
                    ui.muted_text(name),
                    ui.muted_text(&format!("v{}", ver))
                ));
            }
        }

        if !failed.is_empty() {
            if !summary_lines.is_empty() {
                summary_lines.push(String::new());
            }
            summary_lines.push(format!(
                "  {} {}",
                ui.stylize("●", ui.error_color(), true),
                ui.stylize(
                    &format!("Failed ({})", failed.len()),
                    ui.error_color(),
                    true,
                )
            ));
            summary_lines.push(String::new());
            for (name, err) in &failed {
                summary_lines.push(format!(
                    "      {}  {}",
                    ui.highlight_text(name),
                    ui.error_text(err)
                ));
            }
        }

        if !summary_lines.is_empty() {
            ui.blank_line();
            let content = summary_lines.join("\n");
            let summary_box = BoxComponent::new(content)
                .style(BoxStyle::Rounded)
                .title(ui.accent_text("Update Summary"))
                .padding(1)
                .render();
            ui.write_stdout(summary_box);
        }

        let has_success = !updated.is_empty() || !up_to_date.is_empty() || !prebuilt.is_empty();
        if has_success {
            Ok(())
        } else if let Some(name) = plugin_name {
            Err(LlaError::Plugin(format!("Failed to update {}", name)))
        } else {
            Err(LlaError::Plugin("No plugins were updated".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CliBinaryTarget, PluginInstaller};
    #[cfg(feature = "dynamic-plugins")]
    use std::fs;
    use std::io::Write;

    #[test]
    fn cli_asset_names_distinguish_gnu_and_musl() {
        let gnu = CliBinaryTarget::for_platform("linux", "aarch64", false).unwrap();
        let musl = CliBinaryTarget::for_platform("linux", "aarch64", true).unwrap();
        let musl_amd64 = CliBinaryTarget::for_platform("linux", "x86_64", true).unwrap();

        assert_eq!(gnu.asset_name(), "lla-linux-arm64");
        assert_eq!(musl.asset_name(), "lla-linux-arm64-musl");
        assert_eq!(musl_amd64.asset_name(), "lla-linux-amd64-musl");
        assert_eq!(musl.human_label(), "Linux musl (arm64)");
    }

    #[test]
    fn cli_asset_names_preserve_macos_and_i686_gnu() {
        let macos = CliBinaryTarget::for_platform("macos", "aarch64", false).unwrap();
        let i686 = CliBinaryTarget::for_platform("linux", "i686", false).unwrap();

        assert_eq!(macos.asset_name(), "lla-macos-arm64");
        assert_eq!(i686.asset_name(), "lla-linux-i686");
    }

    #[test]
    fn static_musl_upgrade_rejects_i686() {
        let error = CliBinaryTarget::for_platform("linux", "i686", true).unwrap_err();
        assert!(error.to_string().contains("static musl CLI upgrades"));
    }

    #[test]
    fn cli_checksum_calculation_remains_available_without_plugins() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"abc").unwrap();

        assert_eq!(
            PluginInstaller::calculate_sha256(file.path()).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    #[cfg(feature = "dynamic-plugins")]
    fn v3_package_checksums_detect_tampering() {
        let root = tempfile::tempdir().unwrap();
        let entrypoint = root.path().join("libexample.so");
        fs::write(&entrypoint, b"plugin").unwrap();
        let checksum = PluginInstaller::calculate_sha256(&entrypoint).unwrap();
        fs::write(
            root.path().join("checksums.toml"),
            format!("[files]\n\"libexample.so\" = \"{}\"\n", checksum),
        )
        .unwrap();

        PluginInstaller::verify_plugin_package(&entrypoint).unwrap();
        fs::write(&entrypoint, b"tampered").unwrap();
        assert!(PluginInstaller::verify_plugin_package(&entrypoint).is_err());
    }

    #[test]
    #[cfg(feature = "dynamic-plugins")]
    fn source_packages_receive_checksum_inventories() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("plugin.toml"),
            b"[plugin]\nentrypoint = \"example\"\n",
        )
        .unwrap();
        let entrypoint_name = format!(
            "{}example{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        );
        let entrypoint = root.path().join(&entrypoint_name);
        fs::write(&entrypoint, b"plugin").unwrap();
        fs::write(root.path().join("config.toml"), b"first_run = false").unwrap();
        fs::write(root.path().join("cache.toml"), b"entries = []").unwrap();

        PluginInstaller::write_package_checksums(root.path()).unwrap();

        let inventory = fs::read_to_string(root.path().join("checksums.toml")).unwrap();
        let inventory: toml::Value = toml::from_str(&inventory).unwrap();
        let files = inventory
            .get("files")
            .and_then(toml::Value::as_table)
            .unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.contains_key("plugin.toml"));
        assert!(files.contains_key(&entrypoint_name));
        assert!(!files.contains_key("config.toml"));
        assert!(!files.contains_key("cache.toml"));

        fs::write(root.path().join("config.toml"), b"first_run = true").unwrap();
        fs::write(root.path().join("cache.toml"), b"entries = [1]").unwrap();
        assert_eq!(
            PluginInstaller::verify_plugin_package(&entrypoint).unwrap(),
            ()
        );
    }

    #[test]
    #[cfg(feature = "dynamic-plugins")]
    fn prebuilt_v3_packages_require_checksum_inventories() {
        let root = tempfile::tempdir().unwrap();
        let entrypoint = root.path().join("libexample.so");
        fs::write(&entrypoint, b"plugin").unwrap();

        assert!(PluginInstaller::verify_plugin_package(&entrypoint).is_err());
    }
}
