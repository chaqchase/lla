use crate::commands::args::Args;
use crate::error::{LlaError, Result};
use crate::theme::color_value_to_color;
use crate::utils::color::{get_theme, ColorState};
use colored::{Color, Colorize};
use console::Term;
use dialoguer::MultiSelect;
use flate2::read::GzDecoder;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use libloading::Library;
use lla_plugin_interface::{
    proto::{plugin_message::Message, PluginMessage},
    PluginApi, CURRENT_PLUGIN_API_VERSION,
};
use lla_plugin_utils::ui::components::LlaDialoguerTheme;
use prost::Message as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, Read};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;
use std::time::Duration;
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

#[derive(Clone, Copy)]
enum StatusKind {
    Success,
    Info,
    Error,
}

struct InstallerUi<'a> {
    color_state: &'a ColorState,
}

impl<'a> InstallerUi<'a> {
    fn new(color_state: &'a ColorState) -> Self {
        Self { color_state }
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
        format!("{} {}", self.status_icon(kind), message.as_ref())
    }

    fn print_status(&self, kind: StatusKind, message: impl AsRef<str>) {
        println!("{}", self.format_status(kind, message));
    }

    fn section(&self, title: &str) {
        println!();
        println!("{}", self.accent_text(&format!("▸ {}", title)));
        let underline = "─".repeat(title.chars().count() + 4);
        println!("{}", self.muted_text(&underline));
    }

    fn bullet_line(&self, label: &str, value: &str) {
        let bullet = self.highlight_text("•");
        let label_text = self.highlight_text(&format!("{}:", label));
        println!("{} {} {}", bullet, label_text, value);
    }

    fn name_with_version(&self, name: &str, version: &str) -> String {
        let version_tag = format!("v{}", version);
        format!(
            "{} {}",
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
        ui.section("Installation Summary");

        if self.successful.is_empty() && self.failed.is_empty() {
            ui.print_status(StatusKind::Info, "No plugins processed");
            return;
        }

        if !self.successful.is_empty() {
            println!("{}", ui.muted_text("Installed"));
            for (name, version) in &self.successful {
                let entry = ui.name_with_version(name, version);
                ui.print_status(StatusKind::Success, entry);
            }
            if !self.failed.is_empty() {
                println!();
            }
        }

        if !self.failed.is_empty() {
            println!("{}", ui.muted_text("Needs attention"));
            for (name, error) in &self.failed {
                let entry = format!("{} {}", ui.highlight_text(name), ui.error_text(error));
                ui.print_status(StatusKind::Error, entry);
            }
        }
    }
}

pub struct PluginInstaller {
    plugins_dir: PathBuf,
    color_state: ColorState,
}

impl PluginInstaller {
    pub fn new(plugins_dir: &Path, args: &Args) -> Self {
        PluginInstaller {
            plugins_dir: plugins_dir.to_path_buf(),
            color_state: ColorState::new(args),
        }
    }

    fn ui(&self) -> InstallerUi<'_> {
        InstallerUi::new(&self.color_state)
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

    fn create_spinner(&self, message: &str) -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        pb.set_style(self.create_progress_style());
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(80));
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
        progress: &ProgressBar,
        ui: &InstallerUi,
    ) -> Result<u64> {
        let response = Self::github_request(agent, url)
            .call()
            .map_err(|err| Self::map_http_error("Failed to download archive", err))?;

        let mut reader = response.into_reader();
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(destination)?;
        let bytes = std::io::copy(&mut reader, &mut file)?;
        let asset_name = destination
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive");
        let size_text = format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0));
        let message = ui.format_status(
            StatusKind::Success,
            format!(
                "Downloaded {} {}",
                ui.highlight_text(asset_name),
                ui.muted_text(&size_text)
            ),
        );
        progress.finish_with_message(message);
        Ok(bytes)
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
                    .map(|ext| ext.eq_ignore_ascii_case(extension))
                    .unwrap_or(false)
            {
                plugins.push(path.to_path_buf());
            }
        }
        plugins.sort();
        Ok(plugins)
    }

    fn load_prebuilt_plugin(path: &Path) -> Result<PrebuiltPlugin> {
        unsafe {
            let library = Library::new(path).map_err(|err| {
                LlaError::Plugin(format!("Failed to load plugin {:?}: {}", path, err))
            })?;

            let create_fn: libloading::Symbol<unsafe fn() -> *mut PluginApi> =
                library.get(b"_plugin_create").map_err(|err| {
                    LlaError::Plugin(format!(
                        "Plugin {:?} is missing the _plugin_create entry point: {}",
                        path, err
                    ))
                })?;

            let api = create_fn();

            if (*api).version != CURRENT_PLUGIN_API_VERSION {
                return Err(LlaError::Plugin(format!(
                    "Plugin {:?} targets API v{} but lla expects v{}",
                    path,
                    (*api).version,
                    CURRENT_PLUGIN_API_VERSION
                )));
            }

            let name = Self::query_plugin_string(
                api,
                PluginMessage {
                    message: Some(Message::GetName(true)),
                },
                "name",
                path,
            )?;

            let version = Self::query_plugin_string(
                api,
                PluginMessage {
                    message: Some(Message::GetVersion(true)),
                },
                "version",
                path,
            )?;

            let description = Self::query_plugin_string(
                api,
                PluginMessage {
                    message: Some(Message::GetDescription(true)),
                },
                "description",
                path,
            )?;

            drop(library);

            Ok(PrebuiltPlugin {
                path: path.to_path_buf(),
                name,
                version,
                description,
            })
        }
    }

    fn select_prebuilt_plugins(&self, plugins: &[PrebuiltPlugin]) -> Result<Vec<PrebuiltPlugin>> {
        if plugins.is_empty() {
            return Err(LlaError::Plugin("No plugins found in archive".to_string()));
        }

        let ui = self.ui();

        if !atty::is(atty::Stream::Stdout) {
            return Ok(plugins.to_vec());
        }

        ui.section("Select Plugins");
        println!("{}", ui.muted_text("Space to toggle, Enter to confirm"));
        println!();

        let theme = LlaDialoguerTheme::default();
        let items: Vec<String> = plugins
            .iter()
            .map(|plugin| {
                let name = ui.name_with_version(&plugin.name, &plugin.version);
                let description = ui.muted_text(&format!("– {}", plugin.description));
                format!("{} {}", name, description)
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

        let destination = self.plugins_dir.join(file_name);
        fs::copy(&plugin.path, &destination).map_err(|err| {
            LlaError::Plugin(format!(
                "Failed to copy plugin {} to {:?}: {}",
                plugin.name, destination, err
            ))
        })?;

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

    unsafe fn send_plugin_request(
        api: *mut PluginApi,
        message: PluginMessage,
    ) -> Result<PluginMessage> {
        let mut buffer = Vec::with_capacity(message.encoded_len());
        message
            .encode(&mut buffer)
            .map_err(|err| LlaError::Plugin(format!("Failed to encode plugin request: {}", err)))?;

        let raw = ((*api).handle_request)(ptr::null_mut(), buffer.as_ptr(), buffer.len());
        let response_vec = Vec::from_raw_parts(raw.ptr, raw.len, raw.capacity);

        PluginMessage::decode(&response_vec[..])
            .map_err(|err| LlaError::Plugin(format!("Failed to decode plugin response: {}", err)))
    }

    unsafe fn query_plugin_string(
        api: *mut PluginApi,
        message: PluginMessage,
        field: &str,
        path: &Path,
    ) -> Result<String> {
        match Self::send_plugin_request(api, message)?.message {
            Some(Message::NameResponse(value)) if field == "name" => Ok(value),
            Some(Message::VersionResponse(value)) if field == "version" => Ok(value),
            Some(Message::DescriptionResponse(value)) if field == "description" => Ok(value),
            _ => Err(LlaError::Plugin(format!(
                "Plugin {:?} did not provide a {}",
                path, field
            ))),
        }
    }

    fn select_plugins(&self, plugin_dirs: &[PathBuf]) -> Result<Vec<PathBuf>> {
        if !atty::is(atty::Stream::Stdout) {
            return Ok(plugin_dirs.to_vec());
        }

        let ui = self.ui();

        let plugin_names: Vec<String> = plugin_dirs
            .iter()
            .map(|p| {
                let name = Self::get_display_name(p);
                let version = self
                    .get_plugin_version(p)
                    .unwrap_or_else(|_| "unknown".to_string());
                ui.name_with_version(&name, &version)
            })
            .collect();

        if plugin_names.is_empty() {
            return Err(LlaError::Plugin("No plugins found".to_string()));
        }

        ui.section("Select Plugins");
        println!("{}", ui.muted_text("Space to toggle, Enter to confirm"));
        println!();

        let theme = LlaDialoguerTheme::default();

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
        ui.section("Prebuilt Plugin Installation");

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

        ui.bullet_line("Release", &release.tag_name);
        ui.bullet_line("Asset", &asset.name);
        if let Some(ref sum) = checksum {
            ui.bullet_line("Checksum", sum);
        }
        println!();

        let temp_dir = tempfile::tempdir()?;
        let archive_path = temp_dir.path().join(&asset.name);
        let extracted_dir = temp_dir.path().join("plugins");

        let download_message = ui.progress_message("Downloading", &asset.name);
        let download_pb = self.create_spinner(&download_message);
        self.download_to_path(
            &agent,
            &asset.browser_download_url,
            &archive_path,
            &download_pb,
            &ui,
        )?;

        if let Some(expected) = checksum.as_deref() {
            let verify_message = ui.progress_message("Verifying", "checksum");
            let verify_pb = self.create_spinner(&verify_message);
            let actual = Self::calculate_sha256(&archive_path)?;
            if actual.eq_ignore_ascii_case(expected) {
                verify_pb.finish_with_message(
                    ui.format_status(StatusKind::Success, ui.muted_text("Checksum verified")),
                );
            } else {
                let mismatch = ui.format_status(
                    StatusKind::Error,
                    format!(
                        "Checksum mismatch (expected {}, got {})",
                        ui.muted_text(expected),
                        ui.error_text(&actual)
                    ),
                );
                verify_pb.finish_with_message(mismatch);
                return Err(LlaError::Plugin(format!(
                    "Checksum verification failed: expected {}, got {}",
                    expected, actual
                )));
            }
        }

        let extract_message = ui.progress_message("Extracting", &asset.name);
        let extract_pb = self.create_spinner(&extract_message);
        Self::extract_archive(&archive_path, &extracted_dir)?;
        extract_pb.finish_with_message(
            ui.format_status(StatusKind::Success, ui.muted_text("Archive extracted")),
        );

        let plugin_paths =
            Self::collect_prebuilt_plugin_files(&extracted_dir, host.library_extension)?;

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
            match Self::load_prebuilt_plugin(&path) {
                Ok(plugin) => plugins.push(plugin),
                Err(err) => return Err(err),
            }
        }

        let selected_plugins = self.select_prebuilt_plugins(&plugins)?;
        let selected_count = selected_plugins.len();
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

        let m = MultiProgress::new();
        let mut summary = InstallSummary::default();

        for plugin in selected_plugins {
            let initial_message = ui.progress_message("Installing", &plugin.name);
            let pb = m.add(self.create_spinner(&initial_message));
            match self.install_prebuilt_plugin(
                &plugin,
                &release.tag_name,
                &asset.name,
                checksum.as_deref(),
            ) {
                Ok(_) => {
                    let message = ui.format_status(
                        StatusKind::Success,
                        ui.name_with_version(&plugin.name, &plugin.version),
                    );
                    pb.finish_with_message(message);
                    summary.add_success(plugin.name.clone(), plugin.version.clone());
                }
                Err(err) => {
                    let error_text = err.to_string();
                    let message = ui.format_status(
                        StatusKind::Error,
                        format!(
                            "{} {}",
                            ui.highlight_text(&plugin.name),
                            ui.error_text(&error_text)
                        ),
                    );
                    pb.finish_with_message(message);
                    summary.add_failure(plugin.name.clone(), error_text);
                }
            }
        }

        m.clear()?;
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

    pub fn install_from_git(&self, url: &str) -> Result<()> {
        let ui = self.ui();
        ui.section("Git Installation");
        ui.bullet_line("Repository", url);
        println!();

        let m = MultiProgress::new();

        let repo_name = url
            .split('/')
            .last()
            .ok_or_else(|| LlaError::Plugin(format!("Invalid GitHub URL: {}", url)))?
            .trim_end_matches(".git");

        let clone_message = ui.progress_message("Cloning", repo_name);
        let pb = m.add(self.create_spinner(&clone_message));

        let temp_dir = tempfile::tempdir()?;
        let mut child = Command::new("git")
            .args(["clone", "--quiet", url])
            .current_dir(&temp_dir)
            .spawn()?;

        let status = child.wait()?;
        if !status.success() {
            let message = ui.format_status(
                StatusKind::Error,
                format!(
                    "{} {}",
                    ui.highlight_text(repo_name),
                    ui.error_text("Clone failed")
                ),
            );
            pb.finish_with_message(message);
            return Err(LlaError::Plugin("Failed to clone repository".to_string()));
        }

        pb.finish_with_message(ui.format_status(
            StatusKind::Success,
            format!("Cloned {}", ui.highlight_text(repo_name)),
        ));

        self.install_plugins(
            &temp_dir.path().join(repo_name),
            Some((repo_name, url)),
            Some(&m),
        )
    }

    pub fn install_from_directory(&self, dir: &str) -> Result<()> {
        let ui = self.ui();
        ui.section("Local Installation");
        ui.bullet_line("Directory", dir);
        println!();

        let m = MultiProgress::new();

        let source_dir = PathBuf::from(dir.trim_end_matches('/'))
            .canonicalize()
            .map_err(|_| LlaError::Plugin(format!("Directory not found: {}", dir)))?;

        if !source_dir.exists() || !source_dir.is_dir() {
            return Err(LlaError::Plugin(format!("Not a valid directory: {}", dir)));
        }

        self.install_plugins(&source_dir, None, Some(&m))
    }

    fn is_workspace_member(&self, plugin_dir: &Path) -> Result<Option<PathBuf>> {
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
        ui.print_status(
            StatusKind::Info,
            format!(
                "{} will be built independently",
                ui.highlight_text(&plugin_name)
            ),
        );
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
                                    if contents.contains("lla_plugin_interface") {
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
                        if contents.contains("lla_plugin_interface") {
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
        let plugin_name = Self::get_display_name(plugin_dir);

        let workspace_info = self.is_workspace_member(plugin_dir)?;
        let ui = self.ui();

        let (build_dir, build_args) = match workspace_info {
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
                    vec!["build", "--release", "-p", &plugin_name],
                )
            }
            None => {
                if let Some(pb) = pb {
                    pb.set_message(ui.progress_message("Building", &plugin_name));
                }
                (plugin_dir.to_path_buf(), vec!["build", "--release"])
            }
        };

        let mut child = Command::new("cargo")
            .args(&build_args)
            .current_dir(&build_dir)
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        if let Some(pb) = pb {
            if let Some(stderr) = child.stderr.take() {
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        if line.contains("Compiling") {
                            pb.set_message(ui.progress_message("Building", &plugin_name));
                        }
                    }
                }
            }
        }

        let status = child.wait()?;
        if !status.success() {
            return Err(LlaError::Plugin("Build failed".to_string()));
        }

        let target_dir = build_dir.join("target").join("release");
        let plugin_files = self.find_plugin_files(&target_dir, &plugin_name)?;

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

        for plugin_file in plugin_files.iter() {
            let dest_path = self.plugins_dir.join(plugin_file.file_name().unwrap());
            fs::copy(plugin_file, &dest_path)?;
        }

        if pb.is_none() {
            ui.print_status(
                StatusKind::Success,
                format!("Installed {}", ui.highlight_text(&plugin_name)),
            );
        }
        Ok(())
    }

    fn install_plugins(
        &self,
        root_dir: &Path,
        repo_info: Option<(&str, &str)>,
        multi_progress: Option<&MultiProgress>,
    ) -> Result<()> {
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

        if multi_progress.is_none() {
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
        }

        for plugin_dir in selected_plugins.iter() {
            let plugin_name = Self::get_display_name(plugin_dir);

            let progress_bar = if let Some(m) = multi_progress {
                let initial = ui.progress_message("Preparing", &plugin_name);
                Some(m.add(self.create_spinner(&initial)))
            } else {
                None
            };

            match self.build_and_install_plugin(plugin_dir, progress_bar.as_ref(), None) {
                Ok(_) => {
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
                        if let Some(ref pb) = progress_bar {
                            let message = ui.format_status(
                                StatusKind::Error,
                                format!(
                                    "{} {}",
                                    ui.highlight_text(&plugin_name),
                                    ui.error_text(&error_text)
                                ),
                            );
                            pb.finish_with_message(message);
                        }
                    } else {
                        summary.add_success(plugin_name.clone(), version.clone());
                        if let Some(ref pb) = progress_bar {
                            let message = ui.format_status(
                                StatusKind::Success,
                                ui.name_with_version(&plugin_name, &version),
                            );
                            pb.finish_with_message(message);
                        }
                    }
                }
                Err(e) => {
                    let error_text = e.to_string();
                    summary.add_failure(plugin_name.clone(), error_text.clone());
                    if let Some(ref pb) = progress_bar {
                        let message = ui.format_status(
                            StatusKind::Error,
                            format!(
                                "{} {}",
                                ui.highlight_text(&plugin_name),
                                ui.error_text(&error_text)
                            ),
                        );
                        pb.finish_with_message(message);
                    }
                }
            }

            if let Some(ref pb) = progress_bar {
                pb.finish_and_clear();
            }
        }

        if let Some(m) = multi_progress {
            m.clear()?;
        }

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
        ui.section("Update Plugins");
        ui.print_status(
            StatusKind::Info,
            format!(
                "Processing {} {}",
                plugins.len(),
                if plugins.len() == 1 {
                    "plugin"
                } else {
                    "plugins"
                }
            ),
        );

        let m = MultiProgress::new();
        let mut success = false;

        for (name, metadata) in plugins {
            let plugin_label = ui.highlight_text(name);
            let pb = m.add(self.create_spinner(&ui.progress_message("Updating", name)));

            match &metadata.source {
                PluginSource::Git { url } => {
                    let temp_dir = match tempfile::tempdir() {
                        Ok(dir) => dir,
                        Err(e) => {
                            let message = ui.format_status(
                                StatusKind::Error,
                                format!(
                                    "{} {}",
                                    plugin_label,
                                    ui.error_text(&format!("temp directory error: {}", e))
                                ),
                            );
                            pb.finish_with_message(message);
                            continue;
                        }
                    };

                    let output = Command::new("git")
                        .args(["clone", "--quiet", url])
                        .current_dir(&temp_dir)
                        .output()?;

                    if !output.status.success() {
                        let message = ui.format_status(
                            StatusKind::Error,
                            format!(
                                "{} {}",
                                plugin_label,
                                ui.error_text("Failed to clone repository")
                            ),
                        );
                        pb.finish_with_message(message);
                        continue;
                    }

                    let repo_name = url
                        .split('/')
                        .last()
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
                        let message = ui.format_status(
                            StatusKind::Error,
                            format!(
                                "{} {}",
                                plugin_label,
                                ui.error_text("Plugin not found in repository")
                            ),
                        );
                        pb.finish_with_message(message);
                        continue;
                    };

                    match self.build_and_install_plugin(plugin_dir, Some(&pb), None) {
                        Ok(_) => {
                            let new_version = self.get_plugin_version(plugin_dir)?;
                            let mut updated_metadata = metadata.clone();

                            let message = if new_version != metadata.version {
                                ui.format_status(
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
                                ui.format_status(
                                    StatusKind::Info,
                                    format!(
                                        "{} {}",
                                        plugin_label,
                                        ui.muted_text(&format!("already at {}", new_version))
                                    ),
                                )
                            };

                            updated_metadata.version = new_version;
                            updated_metadata.update_timestamp();
                            self.update_plugin_metadata(name, updated_metadata)?;
                            success = true;
                            pb.finish_with_message(message);
                        }
                        Err(e) => {
                            let message = ui.format_status(
                                StatusKind::Error,
                                format!("{} {}", plugin_label, ui.error_text(&e.to_string())),
                            );
                            pb.finish_with_message(message);
                        }
                    }
                }
                PluginSource::Local { directory } => {
                    let source_dir = PathBuf::from(directory);

                    if !source_dir.exists() {
                        let message = ui.format_status(
                            StatusKind::Error,
                            format!(
                                "{} {}",
                                plugin_label,
                                ui.error_text("Source directory not found")
                            ),
                        );
                        pb.finish_with_message(message);
                        continue;
                    }

                    match self.build_and_install_plugin(&source_dir, Some(&pb), None) {
                        Ok(_) => {
                            let new_version = self.get_plugin_version(&source_dir)?;
                            let mut updated_metadata = metadata.clone();

                            let message = if new_version != metadata.version {
                                ui.format_status(
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
                                ui.format_status(
                                    StatusKind::Info,
                                    format!(
                                        "{} {}",
                                        plugin_label,
                                        ui.muted_text(&format!("already at {}", new_version))
                                    ),
                                )
                            };

                            updated_metadata.version = new_version;
                            updated_metadata.update_timestamp();
                            self.update_plugin_metadata(name, updated_metadata)?;
                            success = true;
                            pb.finish_with_message(message);
                        }
                        Err(e) => {
                            let message = ui.format_status(
                                StatusKind::Error,
                                format!("{} {}", plugin_label, ui.error_text(&e.to_string())),
                            );
                            pb.finish_with_message(message);
                        }
                    }
                }
                PluginSource::Prebuilt { .. } => {
                    let message = ui.format_status(
                        StatusKind::Info,
                        format!(
                            "{} {}\n  {}",
                            plugin_label,
                            ui.muted_text("uses prebuilt binaries"),
                            ui.muted_text("Run `lla install --prebuilt` to refresh")
                        ),
                    );
                    pb.finish_with_message(message);
                    success = true;
                }
            }
        }

        m.clear()?;

        if success {
            Ok(())
        } else if let Some(name) = plugin_name {
            Err(LlaError::Plugin(format!("Failed to update {}", name)))
        } else {
            Err(LlaError::Plugin("No plugins were updated".to_string()))
        }
    }
}
