use arboard::Clipboard;
use colored::Colorize;
use dialoguer::{Input, MultiSelect, Select};
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleInfo {
    pub size: u64,
    pub gzip: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmConfig {
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default = "default_registry")]
    pub registry: String,
    #[serde(default = "default_package_manager")]
    pub package_manager: String,
    #[serde(default = "default_colors")]
    pub colors: HashMap<String, String>,
}

fn default_registry() -> String {
    "https://registry.npmjs.org".to_string()
}

fn default_package_manager() -> String {
    "npm".to_string()
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_cyan".to_string());
    colors.insert("warning".to_string(), "bright_yellow".to_string());
    colors.insert("error".to_string(), "bright_red".to_string());
    colors.insert("package".to_string(), "bright_blue".to_string());
    colors.insert("version".to_string(), "bright_magenta".to_string());
    colors
}

impl Default for NpmConfig {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            registry: default_registry(),
            package_manager: default_package_manager(),
            colors: default_colors(),
        }
    }
}

impl PluginConfig for NpmConfig {}

pub struct NpmPlugin {
    base: BasePlugin<NpmConfig>,
}

impl NpmPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[NpmPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn search_package(&self, package_name: &str) -> Result<PackageInfo, String> {
        let url = format!("{}/{}", self.base.config().registry, package_name);
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        let response = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .map_err(|e| format!("Failed to fetch package: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Package not found: {}", package_name));
        }

        let json: serde_json::Value = response
            .json()
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let latest_version = json["dist-tags"]["latest"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        let version_info = &json["versions"][&latest_version];

        Ok(PackageInfo {
            name: json["name"].as_str().unwrap_or(package_name).to_string(),
            version: latest_version,
            description: json["description"]
                .as_str()
                .unwrap_or("No description")
                .to_string(),
            author: version_info["author"]["name"]
                .as_str()
                .or_else(|| json["author"]["name"].as_str())
                .map(|s| s.to_string()),
            license: version_info["license"]
                .as_str()
                .or_else(|| json["license"].as_str())
                .map(|s| s.to_string()),
            homepage: json["homepage"].as_str().map(|s| s.to_string()),
            repository: json["repository"]["url"].as_str().map(|s| s.to_string()),
        })
    }

    fn get_bundle_size(&self, package_name: &str) -> Result<BundleInfo, String> {
        let url = format!("https://bundlephobia.com/api/size?package={}", package_name);
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        let response = client
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to fetch bundle info: {}", e))?;

        if !response.status().is_success() {
            return Err("Bundle size information not available".to_string());
        }

        let json: serde_json::Value = response
            .json()
            .map_err(|e| format!("Failed to parse bundle info: {}", e))?;

        Ok(BundleInfo {
            size: json["size"].as_u64().unwrap_or(0),
            gzip: json["gzip"].as_u64().unwrap_or(0),
        })
    }

    fn format_size(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;

        if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    fn get_install_command(&self, package_name: &str) -> String {
        let pm = &self.base.config().package_manager;
        match pm.as_str() {
            "yarn" => format!("yarn add {}", package_name),
            "pnpm" => format!("pnpm add {}", package_name),
            "bun" => format!("bun add {}", package_name),
            _ => format!("npm install {}", package_name), // default to npm
        }
    }

    fn display_package_info(&self, package: &PackageInfo, bundle: Option<&BundleInfo>) {
        println!("\n{}", "─".repeat(60));
        println!(
            "{} {} {}",
            "📦".bright_cyan(),
            package.name.bright_blue().bold(),
            format!("v{}", package.version).bright_magenta()
        );
        println!("{}", "─".repeat(60));

        println!("{} {}", "Description:".bright_yellow(), package.description);

        if let Some(author) = &package.author {
            println!("{} {}", "Author:".bright_yellow(), author);
        }

        if let Some(license) = &package.license {
            println!("{} {}", "License:".bright_yellow(), license);
        }

        if let Some(homepage) = &package.homepage {
            println!("{} {}", "Homepage:".bright_yellow(), homepage);
        }

        if let Some(repository) = &package.repository {
            println!("{} {}", "Repository:".bright_yellow(), repository);
        }

        if let Some(bundle) = bundle {
            println!("\n{}", "Bundle Size:".bright_cyan().bold());
            println!(
                "  {} {}",
                "Minified:".bright_yellow(),
                Self::format_size(bundle.size).bright_green()
            );
            println!(
                "  {} {}",
                "Gzipped:".bright_yellow(),
                Self::format_size(bundle.gzip).bright_green()
            );
        }

        println!("{}\n", "─".repeat(60));
    }

    fn search_packages(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let package_name: String = Input::with_theme(&theme)
            .with_prompt("Enter package name")
            .interact_text()
            .map_err(|e| format!("Failed to get input: {}", e))?;

        if package_name.trim().is_empty() {
            return Err("Package name cannot be empty".to_string());
        }

        println!(
            "{} Searching for package: {}",
            "🔍".bright_cyan(),
            package_name
        );

        let package = self.search_package(&package_name)?;
        let bundle = self.get_bundle_size(&package_name).ok();

        if bundle.is_none() {
            println!(
                "{} Could not fetch bundle size information",
                "⚠️ ".bright_yellow()
            );
        }

        self.display_package_info(&package, bundle.as_ref());

        let actions = vec![
            "📋 Copy install command",
            "⭐ Add to favorites",
            "🌐 Open npm page",
            "← Back",
        ];

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Choose action", "⚡".bright_cyan()))
            .items(&actions)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        match selection {
            0 => {
                let install_cmd = self.get_install_command(&package.name);
                match Clipboard::new() {
                    Ok(mut clipboard) => {
                        clipboard
                            .set_text(&install_cmd)
                            .map_err(|e| format!("Failed to copy: {}", e))?;
                        println!(
                            "{} Copied to clipboard: {}",
                            "Success:".bright_green(),
                            install_cmd.bright_yellow()
                        );
                    }
                    Err(e) => return Err(format!("Failed to access clipboard: {}", e)),
                }
            }
            1 => {
                let config = self.base.config_mut();
                if !config.favorites.contains(&package.name) {
                    config.favorites.push(package.name.clone());
                    self.base.save_config()?;
                    println!(
                        "{} Added {} to favorites!",
                        "Success:".bright_green(),
                        package.name.bright_blue()
                    );
                } else {
                    println!(
                        "{} {} is already in favorites",
                        "Info:".bright_cyan(),
                        package.name.bright_blue()
                    );
                }
            }
            2 => {
                let npm_url = format!("https://www.npmjs.com/package/{}", package.name);
                #[cfg(target_os = "macos")]
                let open_command = "open";
                #[cfg(target_os = "linux")]
                let open_command = "xdg-open";
                #[cfg(target_os = "windows")]
                let open_command = "start";

                std::process::Command::new(open_command)
                    .arg(&npm_url)
                    .spawn()
                    .map_err(|e| format!("Failed to open browser: {}", e))?;

                println!("{} Opened npm page in browser", "Success:".bright_green());
            }
            3 => {}
            _ => unreachable!(),
        }

        Ok(())
    }

    fn view_favorites(&mut self) -> Result<(), String> {
        let favorites = self.base.config().favorites.clone();

        if favorites.is_empty() {
            println!("{} No favorites yet", "Info:".bright_cyan());
            return Ok(());
        }

        let theme = LlaDialoguerTheme::default();

        let items: Vec<String> = favorites
            .iter()
            .map(|name| format!("{} {}", "📦".bright_cyan(), name.bright_blue()))
            .collect();

        let actions = vec![
            "📦 View package details",
            "📋 Copy install commands",
            "🗑️  Remove from favorites",
            "← Back",
        ];

        let action_selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Choose action", "⚡".bright_cyan()))
            .items(&actions)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        match action_selection {
            0 => {
                let selection = Select::with_theme(&theme)
                    .with_prompt(format!("{} Select package", "📦".bright_cyan()))
                    .items(&items)
                    .default(0)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                let package_name = &favorites[selection];
                println!(
                    "{} Fetching details for {}",
                    "🔍".bright_cyan(),
                    package_name
                );

                let package = self.search_package(package_name)?;
                let bundle = self.get_bundle_size(package_name).ok();

                if bundle.is_none() {
                    println!(
                        "{} Could not fetch bundle size information",
                        "⚠️ ".bright_yellow()
                    );
                }

                self.display_package_info(&package, bundle.as_ref());
            }
            1 => {
                let selection = MultiSelect::with_theme(&theme)
                    .with_prompt(format!("{} Select packages", "📋".bright_cyan()))
                    .items(&items)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                if selection.is_empty() {
                    println!("{} No packages selected", "Info:".bright_blue());
                    return Ok(());
                }

                let commands: Vec<String> = selection
                    .iter()
                    .map(|&i| self.get_install_command(&favorites[i]))
                    .collect();
                let content = commands.join("\n");

                match Clipboard::new() {
                    Ok(mut clipboard) => {
                        clipboard
                            .set_text(&content)
                            .map_err(|e| format!("Failed to copy: {}", e))?;
                        println!(
                            "{} Copied {} install commands to clipboard!",
                            "Success:".bright_green(),
                            selection.len()
                        );
                    }
                    Err(e) => return Err(format!("Failed to access clipboard: {}", e)),
                }
            }
            2 => {
                let selection = MultiSelect::with_theme(&theme)
                    .with_prompt(format!("{} Select packages to remove", "🗑️ ".bright_cyan()))
                    .items(&items)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                if selection.is_empty() {
                    println!("{} No packages selected", "Info:".bright_blue());
                    return Ok(());
                }

                let config = self.base.config_mut();
                let mut indices = selection;
                indices.sort_unstable_by(|a, b| b.cmp(a));
                for &i in &indices {
                    config.favorites.remove(i);
                }

                self.base.save_config()?;
                println!(
                    "{} Removed {} packages from favorites!",
                    "Success:".bright_green(),
                    indices.len()
                );
            }
            3 => {}
            _ => unreachable!(),
        }

        Ok(())
    }

    fn configure_preferences(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let package_managers = vec!["npm", "yarn", "pnpm", "bun"];
        let current_pm = &self.base.config().package_manager;
        let current_index = package_managers
            .iter()
            .position(|&pm| pm == current_pm)
            .unwrap_or(0);

        let selection = Select::with_theme(&theme)
            .with_prompt(format!(
                "{} Select package manager (current: {})",
                "⚙️ ".bright_cyan(),
                current_pm.bright_yellow()
            ))
            .items(&package_managers)
            .default(current_index)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        let new_pm = package_managers[selection];

        if new_pm != current_pm {
            self.base.config_mut().package_manager = new_pm.to_string();
            self.base.save_config()?;
            println!(
                "{} Package manager set to: {}",
                "Success:".bright_green(),
                new_pm.bright_yellow()
            );
        } else {
            println!("{} Package manager unchanged", "Info:".bright_cyan());
        }

        Ok(())
    }

    fn show_help(&self) -> Result<(), String> {
        let colors = self.base.config().colors.clone();

        let mut help = HelpFormatter::new("NPM Plugin".to_string());

        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Search npm packages, view bundle sizes, and manage favorites.".to_string(),
            vec![],
        );

        help.add_section("Actions".to_string())
            .add_command(
                "search".to_string(),
                "Search for npm packages with bundlephobia info".to_string(),
                vec!["search".to_string()],
            )
            .add_command(
                "favorites".to_string(),
                "View and manage favorite packages".to_string(),
                vec!["favorites".to_string()],
            )
            .add_command(
                "preferences".to_string(),
                "Configure package manager (npm, yarn, pnpm, bun)".to_string(),
                vec!["preferences".to_string()],
            )
            .add_command(
                "help".to_string(),
                "Show this help information".to_string(),
                vec!["help".to_string()],
            );

        println!(
            "{}",
            BoxComponent::new(help.render(&colors))
                .style(BoxStyle::Minimal)
                .padding(1)
                .render()
        );

        Ok(())
    }
}

impl Plugin for NpmPlugin {
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
                    PluginRequest::FormatField(_entry, _format) => {
                        PluginResponse::FormattedField(None)
                    }
                    PluginRequest::PerformAction(action, _args) => {
                        let result = match action.as_str() {
                            "search" => self.search_packages(),
                            "favorites" => self.view_favorites(),
                            "preferences" => self.configure_preferences(),
                            "help" => self.show_help(),
                            _ => Err(format!("Unknown action: {}", action)),
                        };
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::GetAvailableActions => {
                        use lla_plugin_interface::ActionInfo;
                        PluginResponse::AvailableActions(vec![
                            ActionInfo {
                                name: "search".to_string(),
                                usage: "search".to_string(),
                                description: "Search npm packages".to_string(),
                                examples: vec!["lla plugin npm search".to_string()],
                            },
                            ActionInfo {
                                name: "favorites".to_string(),
                                usage: "favorites".to_string(),
                                description: "View favorite packages".to_string(),
                                examples: vec!["lla plugin npm favorites".to_string()],
                            },
                            ActionInfo {
                                name: "preferences".to_string(),
                                usage: "preferences".to_string(),
                                description: "Configure preferences".to_string(),
                                examples: vec!["lla plugin npm preferences".to_string()],
                            },
                            ActionInfo {
                                name: "help".to_string(),
                                usage: "help".to_string(),
                                description: "Show help information".to_string(),
                                examples: vec!["lla plugin npm help".to_string()],
                            },
                        ])
                    }
                };
                self.encode_response(response)
            }
            Err(e) => self.encode_error(&e),
        }
    }
}

impl Default for NpmPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for NpmPlugin {
    type Config = NpmConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for NpmPlugin {}

lla_plugin_interface::declare_plugin!(NpmPlugin);
