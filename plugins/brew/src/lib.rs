use colored::Colorize;
use dialoguer::{Input, Select};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use indicatif::{ProgressBar, ProgressStyle};
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, LlaDialoguerTheme},
    ui::interactive_suggest,
    BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;

const FORMULA_API_URL: &str = "https://formulae.brew.sh/api/formula.json";
const CASK_API_URL: &str = "https://formulae.brew.sh/api/cask.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrewConfig {
    #[serde(default)]
    pub custom_brew_path: Option<String>,
    #[serde(default = "default_true")]
    pub greedy_upgrades: bool,
    #[serde(default = "default_true")]
    pub show_caveats: bool,
    #[serde(default = "default_colors")]
    pub colors: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_cyan".to_string());
    colors.insert("warning".to_string(), "bright_yellow".to_string());
    colors.insert("error".to_string(), "bright_red".to_string());
    colors.insert("formula".to_string(), "bright_blue".to_string());
    colors.insert("cask".to_string(), "bright_magenta".to_string());
    colors.insert("version".to_string(), "bright_yellow".to_string());
    colors.insert("outdated".to_string(), "bright_red".to_string());
    colors
}

impl Default for BrewConfig {
    fn default() -> Self {
        Self {
            custom_brew_path: None,
            greedy_upgrades: true,
            show_caveats: true,
            colors: default_colors(),
        }
    }
}

impl PluginConfig for BrewConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Formula {
    pub name: String,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub versions: Option<Versions>,
    #[serde(default)]
    pub installed: Vec<InstalledVersion>,
    #[serde(default)]
    pub outdated: bool,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cask {
    pub token: String,
    #[serde(default)]
    pub name: Vec<String>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub installed: Option<String>,
    #[serde(default)]
    pub outdated: Option<bool>,
    #[serde(default)]
    pub auto_updates: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versions {
    pub stable: Option<String>,
    pub head: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledVersion {
    pub version: String,
    #[serde(default)]
    pub installed_as_dependency: bool,
    #[serde(default)]
    pub installed_on_request: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutdatedInfo {
    pub formulae: Vec<OutdatedFormula>,
    pub casks: Vec<OutdatedCask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutdatedFormula {
    pub name: String,
    pub current_version: String,
    #[serde(default)]
    pub installed_versions: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutdatedCask {
    pub name: String,
    pub current_version: String,
    #[serde(default)]
    pub installed_versions: Vec<String>,
}

pub struct BrewPlugin {
    base: BasePlugin<BrewConfig>,
    http: Client,
    brew_prefix: PathBuf,
    catalog_formulae: Option<Vec<Formula>>,
    catalog_casks: Option<Vec<Cask>>,
    catalog_fetched_at: Option<Instant>,
}

impl BrewPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        let brew_prefix = Self::detect_brew_prefix();

        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
            http: client,
            brew_prefix,
            catalog_formulae: None,
            catalog_casks: None,
            catalog_fetched_at: None,
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[BrewPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn detect_brew_prefix() -> PathBuf {
        // Try to get from brew --prefix
        if let Ok(output) = Command::new("brew").arg("--prefix").output() {
            if output.status.success() {
                let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return PathBuf::from(prefix);
            }
        }

        // Fallback based on architecture
        #[cfg(target_arch = "aarch64")]
        return PathBuf::from("/opt/homebrew");

        #[cfg(not(target_arch = "aarch64"))]
        return PathBuf::from("/usr/local");
    }

    fn brew_executable(&self) -> PathBuf {
        if let Some(custom_path) = &self.base.config().custom_brew_path {
            PathBuf::from(custom_path)
        } else {
            self.brew_prefix.join("bin/brew")
        }
    }

    fn exec_brew(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new(self.brew_executable())
            .args(args)
            .output()
            .map_err(|e| format!("Failed to execute brew: {}", e))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.is_empty() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(stderr.to_string())
            }
        }
    }

    fn clear_screen() {
        let _ = console::Term::stdout().clear_screen();
    }

    fn pause(&self, prompt: &str) {
        let theme = LlaDialoguerTheme::default();
        let _ = Input::<String>::with_theme(&theme)
            .with_prompt(prompt)
            .allow_empty(true)
            .interact_text();
    }

    fn render_header(&self, title: &str, subtitle: &str) {
        let content = format!(
            "{}\n{}",
            subtitle.bright_black(),
            format!("brew: {}", title).bright_cyan()
        );
        println!(
            "{}",
            BoxComponent::new(content)
                .title("🍺 Homebrew".bright_white().bold().to_string())
                .style(BoxStyle::Rounded)
                .padding(1)
                .render()
        );
    }

    fn ensure_catalog_loaded(&mut self, force: bool) -> Result<(), String> {
        let stale = self
            .catalog_fetched_at
            .map(|t| t.elapsed() > Duration::from_secs(60 * 60))
            .unwrap_or(true);

        if !force && self.catalog_formulae.is_some() && self.catalog_casks.is_some() && !stale {
            return Ok(());
        }

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message("Fetching Homebrew catalog (formulae + casks)...");
        pb.enable_steady_tick(Duration::from_millis(80));

        let formulae: Vec<Formula> = self
            .http
            .get(FORMULA_API_URL)
            .send()
            .map_err(|e| format!("Failed to fetch formula catalog: {}", e))?
            .json()
            .map_err(|e| format!("Failed to parse formula catalog: {}", e))?;

        let casks: Vec<Cask> = self
            .http
            .get(CASK_API_URL)
            .send()
            .map_err(|e| format!("Failed to fetch cask catalog: {}", e))?
            .json()
            .map_err(|e| format!("Failed to parse cask catalog: {}", e))?;

        pb.finish_and_clear();

        self.catalog_formulae = Some(formulae);
        self.catalog_casks = Some(casks);
        self.catalog_fetched_at = Some(Instant::now());
        Ok(())
    }

    fn search_tui(&mut self) -> Result<(), String> {
        self.ensure_catalog_loaded(false)?;

        let query = interactive_suggest("Search Homebrew:", None, |q| self.catalog_suggestions(q))?;
        if query.trim().is_empty() {
            return Ok(());
        }

        let results = self.search_catalog_results(&query, 30);
        if results.is_empty() {
            println!(
                "{}",
                BoxComponent::new(format!(
                    "{}\n{}",
                    "No matches found.".bright_yellow(),
                    "Tip: try a shorter query or different keywords.".bright_black()
                ))
                .title("🔍 Search".bright_white().bold().to_string())
                .style(BoxStyle::Minimal)
                .padding(1)
                .render()
            );
            self.pause("Press Enter to return");
            return Ok(());
        }

        let theme = LlaDialoguerTheme::default();
        let items: Vec<String> = results
            .iter()
            .map(|r| {
                let tag = match r.kind.as_str() {
                    "cask" => "CASK".bright_magenta(),
                    _ => "FORM".bright_blue(),
                };
                let ver = r
                    .version
                    .clone()
                    .unwrap_or_else(|| "?".to_string())
                    .bright_black();
                let desc = r.desc.clone().unwrap_or_default();
                format!(
                    "{}  {}  {}  {}",
                    tag,
                    r.name.bright_white(),
                    ver,
                    desc.bright_black()
                )
            })
            .collect();

        let mut menu = items.clone();
        menu.push("← Back".to_string());
        let selection = Select::with_theme(&theme)
            .with_prompt("Select a package")
            .items(&menu)
            .default(0)
            .interact_opt()
            .map_err(|e| format!("Failed to show selector: {}", e))?;

        let Some(idx) = selection else {
            return Ok(());
        };
        if idx == menu.len() - 1 {
            return Ok(());
        }

        let picked = &results[idx];
        self.package_actions_tui(picked)
    }

    fn package_actions_tui(&mut self, pkg: &CatalogHit) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();
        let actions = vec![
            "📋 Info",
            "📥 Install",
            "⬆️  Upgrade",
            "🗑️  Uninstall",
            "← Back",
        ];
        let selection = Select::with_theme(&theme)
            .with_prompt(format!(
                "{} {}",
                "Package:".bright_cyan(),
                pkg.name.bright_white()
            ))
            .items(&actions)
            .default(0)
            .interact_opt()
            .map_err(|e| format!("Failed to show selector: {}", e))?;

        let Some(idx) = selection else {
            return Ok(());
        };
        match idx {
            0 => self.package_info(&vec![pkg.name.clone()]),
            1 => {
                let mut args = vec![pkg.name.clone()];
                if pkg.kind == "cask" {
                    args.push("--cask".to_string());
                }
                self.install_package(&args)
            }
            2 => self.upgrade_packages(&vec![pkg.name.clone()]),
            3 => {
                let mut args = vec![pkg.name.clone()];
                if pkg.kind == "cask" {
                    args.push("--cask".to_string());
                }
                self.uninstall_package(&args)
            }
            _ => Ok(()),
        }
    }

    fn catalog_suggestions(&mut self, q: &str) -> Result<Vec<String>, String> {
        // Fast, in-memory suggestion list for interactive_suggest.
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let hits = self.search_catalog_results(&q, 8);
        Ok(hits
            .into_iter()
            .map(|h| match h.kind.as_str() {
                "cask" => format!("{} (cask)", h.name),
                _ => h.name,
            })
            .collect())
    }

    fn search_catalog_results(&self, query: &str, limit: usize) -> Vec<CatalogHit> {
        let matcher = SkimMatcherV2::default();
        let q = query.to_lowercase();

        let mut out: Vec<CatalogHit> = Vec::new();

        if let Some(formulae) = self.catalog_formulae.as_ref() {
            for f in formulae {
                let name_score = matcher.fuzzy_match(&f.name.to_lowercase(), &q).unwrap_or(0);
                let desc_score = f
                    .desc
                    .as_ref()
                    .and_then(|d| matcher.fuzzy_match(&d.to_lowercase(), &q))
                    .unwrap_or(0);
                let score = name_score.max(desc_score);
                if score > 0 {
                    out.push(CatalogHit {
                        kind: "formula".to_string(),
                        name: f.name.clone(),
                        version: f.versions.as_ref().and_then(|v| v.stable.clone()),
                        desc: f.desc.clone(),
                        score,
                    });
                }
            }
        }

        if let Some(casks) = self.catalog_casks.as_ref() {
            for c in casks {
                let token_score = matcher
                    .fuzzy_match(&c.token.to_lowercase(), &q)
                    .unwrap_or(0);
                let name_score = c
                    .name
                    .first()
                    .and_then(|n| matcher.fuzzy_match(&n.to_lowercase(), &q))
                    .unwrap_or(0);
                let desc_score = c
                    .desc
                    .as_ref()
                    .and_then(|d| matcher.fuzzy_match(&d.to_lowercase(), &q))
                    .unwrap_or(0);
                let score = token_score.max(name_score).max(desc_score);
                if score > 0 {
                    out.push(CatalogHit {
                        kind: "cask".to_string(),
                        name: c.token.clone(),
                        version: c.version.clone(),
                        desc: c.desc.clone(),
                        score,
                    });
                }
            }
        }

        out.sort_by(|a, b| b.score.cmp(&a.score));
        out.truncate(limit);
        out
    }

    fn list_installed(&self) -> Result<(), String> {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message("Fetching installed packages...");
        pb.enable_steady_tick(Duration::from_millis(100));

        let output = self.exec_brew(&["info", "--json=v2", "--installed"])?;
        pb.finish_and_clear();

        let info: Value = serde_json::from_str(&output)
            .map_err(|e| format!("Failed to parse brew info: {}", e))?;

        println!(
            "\n{} {}",
            "📦".bright_cyan(),
            "Installed Packages".bright_cyan()
        );
        println!("{}", "─".repeat(70).bright_black());

        // Formulae
        if let Some(formulae) = info.get("formulae").and_then(|f| f.as_array()) {
            println!(
                "\n{} {} ({})",
                "🍺".bright_blue(),
                "Formulae".bright_blue(),
                formulae.len().to_string().bright_yellow()
            );

            for formula in formulae.iter().take(50) {
                let name = formula
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown");
                let version = formula
                    .get("installed")
                    .and_then(|i| i.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.get("version"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let outdated = formula
                    .get("outdated")
                    .and_then(|o| o.as_bool())
                    .unwrap_or(false);
                let pinned = formula
                    .get("pinned")
                    .and_then(|p| p.as_bool())
                    .unwrap_or(false);

                let mut status_badges = Vec::new();
                if outdated {
                    status_badges.push("outdated".bright_red().to_string());
                }
                if pinned {
                    status_badges.push("pinned".bright_yellow().to_string());
                }

                let status_str = if status_badges.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", status_badges.join(", "))
                };

                println!(
                    "   {} {} {}{}",
                    "•".bright_cyan(),
                    name.bright_white(),
                    version.bright_black(),
                    status_str
                );
            }
        }

        // Casks
        if let Some(casks) = info.get("casks").and_then(|c| c.as_array()) {
            println!(
                "\n{} {} ({})",
                "🖥️ ".bright_magenta(),
                "Casks".bright_magenta(),
                casks.len().to_string().bright_yellow()
            );

            for cask in casks.iter().take(50) {
                let token = cask
                    .get("token")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown");
                let name = cask
                    .get("name")
                    .and_then(|n| n.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|n| n.as_str())
                    .unwrap_or(token);
                let version = cask
                    .get("installed")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let outdated = cask
                    .get("outdated")
                    .and_then(|o| o.as_bool())
                    .unwrap_or(false);

                let status_str = if outdated {
                    format!(" [{}]", "outdated".bright_red())
                } else {
                    String::new()
                };

                println!(
                    "   {} {} ({}) {}{}",
                    "•".bright_cyan(),
                    name.bright_white(),
                    token.bright_black(),
                    version.bright_black(),
                    status_str
                );
            }
        }

        println!("{}", "─".repeat(70).bright_black());
        Ok(())
    }

    fn list_outdated(&self) -> Result<(), String> {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message("Checking for updates...");
        pb.enable_steady_tick(Duration::from_millis(100));

        // First update
        let _ = self.exec_brew(&["update"]);

        let output = self.exec_brew(&["outdated", "--json=v2"])?;
        pb.finish_and_clear();

        let info: OutdatedInfo = serde_json::from_str(&output)
            .map_err(|e| format!("Failed to parse outdated info: {}", e))?;

        println!(
            "\n{} {}",
            "🔄".bright_cyan(),
            "Outdated Packages".bright_cyan()
        );
        println!("{}", "─".repeat(70).bright_black());

        if info.formulae.is_empty() && info.casks.is_empty() {
            println!(
                "\n   {} {}",
                "✓".bright_green(),
                "All packages are up to date!".bright_green()
            );
        } else {
            // Formulae
            if !info.formulae.is_empty() {
                println!(
                    "\n{} {} ({})",
                    "🍺".bright_blue(),
                    "Outdated Formulae".bright_blue(),
                    info.formulae.len().to_string().bright_yellow()
                );

                for formula in &info.formulae {
                    let pinned_str = if formula.pinned { " [pinned]" } else { "" };
                    println!(
                        "   {} {} {} → {}{}",
                        "•".bright_cyan(),
                        formula.name.bright_white(),
                        formula.installed_versions.join(", ").bright_red(),
                        formula.current_version.bright_green(),
                        pinned_str.bright_yellow()
                    );
                }
            }

            // Casks
            if !info.casks.is_empty() {
                println!(
                    "\n{} {} ({})",
                    "🖥️ ".bright_magenta(),
                    "Outdated Casks".bright_magenta(),
                    info.casks.len().to_string().bright_yellow()
                );

                for cask in &info.casks {
                    println!(
                        "   {} {} {} → {}",
                        "•".bright_cyan(),
                        cask.name.bright_white(),
                        cask.installed_versions.join(", ").bright_red(),
                        cask.current_version.bright_green()
                    );
                }
            }
        }

        println!("{}", "─".repeat(70).bright_black());
        Ok(())
    }

    fn search_packages(&mut self, query: Option<&str>) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let search_query = if let Some(q) = query {
            q.to_string()
        } else {
            Input::with_theme(&theme)
                .with_prompt("Search for package")
                .interact_text()
                .map_err(|e| format!("Failed to get input: {}", e))?
        };

        if search_query.trim().is_empty() {
            return Err("Search query cannot be empty".to_string());
        }

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message("Searching packages...");
        pb.enable_steady_tick(Duration::from_millis(100));

        // Fetch formulae
        let formulae: Vec<Formula> = self
            .http
            .get(FORMULA_API_URL)
            .send()
            .and_then(|r| r.json())
            .unwrap_or_default();

        // Fetch casks
        let casks: Vec<Cask> = self
            .http
            .get(CASK_API_URL)
            .send()
            .and_then(|r| r.json())
            .unwrap_or_default();

        pb.finish_and_clear();

        let matcher = SkimMatcherV2::default();
        let query_lower = search_query.to_lowercase();

        // Filter formulae
        let mut matched_formulae: Vec<(&Formula, i64)> = formulae
            .iter()
            .filter_map(|f| {
                let name_score = matcher
                    .fuzzy_match(&f.name.to_lowercase(), &query_lower)
                    .unwrap_or(0);
                let desc_score = f
                    .desc
                    .as_ref()
                    .and_then(|d| matcher.fuzzy_match(&d.to_lowercase(), &query_lower))
                    .unwrap_or(0);

                let best_score = name_score.max(desc_score);
                if best_score > 0 {
                    Some((f, best_score))
                } else {
                    None
                }
            })
            .collect();

        // Filter casks
        let mut matched_casks: Vec<(&Cask, i64)> = casks
            .iter()
            .filter_map(|c| {
                let token_score = matcher
                    .fuzzy_match(&c.token.to_lowercase(), &query_lower)
                    .unwrap_or(0);
                let name_score = c
                    .name
                    .first()
                    .and_then(|n| matcher.fuzzy_match(&n.to_lowercase(), &query_lower))
                    .unwrap_or(0);
                let desc_score = c
                    .desc
                    .as_ref()
                    .and_then(|d| matcher.fuzzy_match(&d.to_lowercase(), &query_lower))
                    .unwrap_or(0);

                let best_score = token_score.max(name_score).max(desc_score);
                if best_score > 0 {
                    Some((c, best_score))
                } else {
                    None
                }
            })
            .collect();

        // Sort by score
        matched_formulae.sort_by(|a, b| b.1.cmp(&a.1));
        matched_casks.sort_by(|a, b| b.1.cmp(&a.1));

        println!(
            "\n{} Search Results for '{}'",
            "🔍".bright_cyan(),
            search_query.bright_yellow()
        );
        println!("{}", "─".repeat(70).bright_black());

        // Show formulae
        if !matched_formulae.is_empty() {
            println!(
                "\n{} {} ({})",
                "🍺".bright_blue(),
                "Formulae".bright_blue(),
                matched_formulae.len().to_string().bright_yellow()
            );

            for (formula, _) in matched_formulae.iter().take(15) {
                let version = formula
                    .versions
                    .as_ref()
                    .and_then(|v| v.stable.as_ref())
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                let desc = formula.desc.as_deref().unwrap_or("");

                println!(
                    "   {} {} {} - {}",
                    "•".bright_cyan(),
                    formula.name.bright_white(),
                    version.bright_black(),
                    desc.bright_black()
                );
            }

            if matched_formulae.len() > 15 {
                println!(
                    "   {} ... and {} more",
                    "".bright_black(),
                    (matched_formulae.len() - 15).to_string().bright_yellow()
                );
            }
        }

        // Show casks
        if !matched_casks.is_empty() {
            println!(
                "\n{} {} ({})",
                "🖥️ ".bright_magenta(),
                "Casks".bright_magenta(),
                matched_casks.len().to_string().bright_yellow()
            );

            for (cask, _) in matched_casks.iter().take(15) {
                let name = cask.name.first().map(|s| s.as_str()).unwrap_or(&cask.token);
                let version = cask.version.as_deref().unwrap_or("?");
                let desc = cask.desc.as_deref().unwrap_or("");

                println!(
                    "   {} {} ({}) {} - {}",
                    "•".bright_cyan(),
                    name.bright_white(),
                    cask.token.bright_black(),
                    version.bright_black(),
                    desc.bright_black()
                );
            }

            if matched_casks.len() > 15 {
                println!(
                    "   {} ... and {} more",
                    "".bright_black(),
                    (matched_casks.len() - 15).to_string().bright_yellow()
                );
            }
        }

        if matched_formulae.is_empty() && matched_casks.is_empty() {
            println!(
                "\n   {} No packages found matching '{}'",
                "ℹ️ ".bright_cyan(),
                search_query.bright_yellow()
            );
        }

        println!("{}", "─".repeat(70).bright_black());
        Ok(())
    }

    fn install_package(&self, args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Please specify a package to install".to_string());
        }

        let package = &args[0];
        let is_cask = args.iter().any(|a| a == "--cask");

        println!(
            "\n{} Installing {}{}...",
            "📦".bright_cyan(),
            package.bright_yellow(),
            if is_cask { " (cask)" } else { "" }
        );

        let mut brew_args = vec!["install"];
        if is_cask {
            brew_args.push("--cask");
        }
        brew_args.push(package);

        let output = self.exec_brew(&brew_args)?;
        println!("{}", output);

        println!(
            "{} {} installed successfully!",
            "✓".bright_green(),
            package.bright_yellow()
        );
        Ok(())
    }

    fn uninstall_package(&self, args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Please specify a package to uninstall".to_string());
        }

        let package = &args[0];
        let is_cask = args.iter().any(|a| a == "--cask");

        println!(
            "\n{} Uninstalling {}{}...",
            "🗑️ ".bright_cyan(),
            package.bright_yellow(),
            if is_cask { " (cask)" } else { "" }
        );

        let mut brew_args = vec!["uninstall"];
        if is_cask {
            brew_args.push("--cask");
        }
        brew_args.push(package);

        let output = self.exec_brew(&brew_args)?;
        println!("{}", output);

        println!(
            "{} {} uninstalled successfully!",
            "✓".bright_green(),
            package.bright_yellow()
        );
        Ok(())
    }

    fn upgrade_packages(&self, args: &[String]) -> Result<(), String> {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message("Updating Homebrew...");
        pb.enable_steady_tick(Duration::from_millis(100));

        let _ = self.exec_brew(&["update"]);
        pb.finish_and_clear();

        if args.is_empty() {
            println!("\n{} Upgrading all packages...", "🔄".bright_cyan());

            let mut upgrade_args = vec!["upgrade"];
            if self.base.config().greedy_upgrades {
                upgrade_args.push("--greedy");
            }

            let output = self.exec_brew(&upgrade_args)?;
            println!("{}", output);
        } else {
            let package = &args[0];
            println!(
                "\n{} Upgrading {}...",
                "🔄".bright_cyan(),
                package.bright_yellow()
            );

            let output = self.exec_brew(&["upgrade", package])?;
            println!("{}", output);
        }

        println!("{} Upgrade completed!", "✓".bright_green());
        Ok(())
    }

    fn cleanup(&self) -> Result<(), String> {
        println!(
            "\n{} Cleaning up old versions and cache...",
            "🧹".bright_cyan()
        );

        let output = self.exec_brew(&["cleanup", "--prune=all"])?;
        println!("{}", output);

        println!("{} Cleanup completed!", "✓".bright_green());
        Ok(())
    }

    fn run_doctor(&self) -> Result<(), String> {
        println!("\n{} Running Homebrew diagnostics...", "🩺".bright_cyan());
        println!("{}", "─".repeat(70).bright_black());

        match self.exec_brew(&["doctor"]) {
            Ok(output) => {
                if output.trim().is_empty() || output.contains("ready to brew") {
                    println!("\n   {} Your system is ready to brew!", "✓".bright_green());
                } else {
                    println!("{}", output);
                }
            }
            Err(e) => {
                println!("{}", e);
            }
        }

        println!("{}", "─".repeat(70).bright_black());
        Ok(())
    }

    fn package_info(&self, args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Please specify a package name".to_string());
        }

        let package = &args[0];

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message(format!("Fetching info for {}...", package));
        pb.enable_steady_tick(Duration::from_millis(100));

        let output = self.exec_brew(&["info", "--json=v2", package])?;
        pb.finish_and_clear();

        let info: Value =
            serde_json::from_str(&output).map_err(|e| format!("Failed to parse info: {}", e))?;

        println!(
            "\n{} Package Info: {}",
            "📋".bright_cyan(),
            package.bright_yellow()
        );
        println!("{}", "─".repeat(70).bright_black());

        // Check if it's a formula
        if let Some(formulae) = info.get("formulae").and_then(|f| f.as_array()) {
            if let Some(formula) = formulae.first() {
                let mut list = List::new().style(BoxStyle::Minimal).key_width(15);

                if let Some(name) = formula.get("name").and_then(|n| n.as_str()) {
                    list.add_item(
                        KeyValue::new("Name", name)
                            .key_color("bright_cyan")
                            .value_color("bright_white")
                            .key_width(15)
                            .render(),
                    );
                }

                if let Some(desc) = formula.get("desc").and_then(|d| d.as_str()) {
                    list.add_item(
                        KeyValue::new("Description", desc)
                            .key_color("bright_cyan")
                            .value_color("bright_white")
                            .key_width(15)
                            .render(),
                    );
                }

                if let Some(version) = formula
                    .get("versions")
                    .and_then(|v| v.get("stable"))
                    .and_then(|s| s.as_str())
                {
                    list.add_item(
                        KeyValue::new("Version", version)
                            .key_color("bright_cyan")
                            .value_color("bright_yellow")
                            .key_width(15)
                            .render(),
                    );
                }

                if let Some(homepage) = formula.get("homepage").and_then(|h| h.as_str()) {
                    list.add_item(
                        KeyValue::new("Homepage", homepage)
                            .key_color("bright_cyan")
                            .value_color("bright_blue")
                            .key_width(15)
                            .render(),
                    );
                }

                if let Some(license) = formula.get("license").and_then(|l| l.as_str()) {
                    list.add_item(
                        KeyValue::new("License", license)
                            .key_color("bright_cyan")
                            .value_color("bright_white")
                            .key_width(15)
                            .render(),
                    );
                }

                let installed = formula
                    .get("installed")
                    .and_then(|i| i.as_array())
                    .map(|arr| !arr.is_empty())
                    .unwrap_or(false);

                list.add_item(
                    KeyValue::new("Installed", if installed { "Yes ✓" } else { "No" })
                        .key_color("bright_cyan")
                        .value_color(if installed {
                            "bright_green"
                        } else {
                            "bright_black"
                        })
                        .key_width(15)
                        .render(),
                );

                println!("\n{}", list.render());

                // Caveats
                if self.base.config().show_caveats {
                    if let Some(caveats) = formula.get("caveats").and_then(|c| c.as_str()) {
                        println!("\n{} {}", "⚠️ ".bright_yellow(), "Caveats:".bright_yellow());
                        println!("{}", caveats.bright_black());
                    }
                }
            }
        }

        // Check if it's a cask
        if let Some(casks) = info.get("casks").and_then(|c| c.as_array()) {
            if let Some(cask) = casks.first() {
                let mut list = List::new().style(BoxStyle::Minimal).key_width(15);

                if let Some(names) = cask.get("name").and_then(|n| n.as_array()) {
                    if let Some(name) = names.first().and_then(|n| n.as_str()) {
                        list.add_item(
                            KeyValue::new("Name", name)
                                .key_color("bright_cyan")
                                .value_color("bright_white")
                                .key_width(15)
                                .render(),
                        );
                    }
                }

                if let Some(token) = cask.get("token").and_then(|t| t.as_str()) {
                    list.add_item(
                        KeyValue::new("Token", token)
                            .key_color("bright_cyan")
                            .value_color("bright_magenta")
                            .key_width(15)
                            .render(),
                    );
                }

                if let Some(desc) = cask.get("desc").and_then(|d| d.as_str()) {
                    list.add_item(
                        KeyValue::new("Description", desc)
                            .key_color("bright_cyan")
                            .value_color("bright_white")
                            .key_width(15)
                            .render(),
                    );
                }

                if let Some(version) = cask.get("version").and_then(|v| v.as_str()) {
                    list.add_item(
                        KeyValue::new("Version", version)
                            .key_color("bright_cyan")
                            .value_color("bright_yellow")
                            .key_width(15)
                            .render(),
                    );
                }

                if let Some(homepage) = cask.get("homepage").and_then(|h| h.as_str()) {
                    list.add_item(
                        KeyValue::new("Homepage", homepage)
                            .key_color("bright_cyan")
                            .value_color("bright_blue")
                            .key_width(15)
                            .render(),
                    );
                }

                let installed = cask.get("installed").and_then(|i| i.as_str()).is_some();

                list.add_item(
                    KeyValue::new("Installed", if installed { "Yes ✓" } else { "No" })
                        .key_color("bright_cyan")
                        .value_color(if installed {
                            "bright_green"
                        } else {
                            "bright_black"
                        })
                        .key_width(15)
                        .render(),
                );

                println!("\n{}", list.render());

                // Caveats
                if self.base.config().show_caveats {
                    if let Some(caveats) = cask.get("caveats").and_then(|c| c.as_str()) {
                        println!("\n{} {}", "⚠️ ".bright_yellow(), "Caveats:".bright_yellow());
                        println!("{}", caveats.bright_black());
                    }
                }
            }
        }

        println!("{}", "─".repeat(70).bright_black());
        Ok(())
    }

    fn interactive_menu(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        loop {
            Self::clear_screen();
            self.render_header(
                "Menu",
                "Manage packages, upgrades, and search with a mini TUI.",
            );

            let options = vec![
                "🔎 Search & act (TUI)",
                "📦 Installed",
                "🔄 Outdated",
                "⬆️  Upgrade all",
                "📥 Install…",
                "🗑️  Uninstall…",
                "📋 Info…",
                "🧹 Cleanup",
                "🩺 Doctor",
                "❓ Help",
                "← Exit",
            ];

            let selection = Select::with_theme(&theme)
                .with_prompt("Choose an action")
                .items(&options)
                .default(0)
                .interact_opt()
                .map_err(|e| format!("Failed to show menu: {}", e))?;

            let Some(choice) = selection else {
                return Ok(());
            };

            let result = match choice {
                0 => self.search_tui(),
                1 => self.list_installed(),
                2 => self.list_outdated(),
                3 => self.upgrade_packages(&[]),
                4 => {
                    let package: String = Input::with_theme(&theme)
                        .with_prompt("Package to install (use --cask in args if needed)")
                        .interact_text()
                        .map_err(|e| format!("Failed to get input: {}", e))?;
                    if package.trim().is_empty() {
                        Ok(())
                    } else {
                        self.install_package(&[package])
                    }
                }
                5 => {
                    let package: String = Input::with_theme(&theme)
                        .with_prompt("Package to uninstall (use --cask in args if needed)")
                        .interact_text()
                        .map_err(|e| format!("Failed to get input: {}", e))?;
                    if package.trim().is_empty() {
                        Ok(())
                    } else {
                        self.uninstall_package(&[package])
                    }
                }
                6 => {
                    let package: String = Input::with_theme(&theme)
                        .with_prompt("Package name")
                        .interact_text()
                        .map_err(|e| format!("Failed to get input: {}", e))?;
                    if package.trim().is_empty() {
                        Ok(())
                    } else {
                        self.package_info(&[package])
                    }
                }
                7 => self.cleanup(),
                8 => self.run_doctor(),
                9 => self.show_help(),
                _ => return Ok(()),
            };

            if let Err(e) = result {
                println!(
                    "{}",
                    BoxComponent::new(format!("{}", e.bright_red()))
                        .title("✗ Error".bright_red().bold().to_string())
                        .style(BoxStyle::Minimal)
                        .padding(1)
                        .render()
                );
            }
            self.pause("Press Enter to return to the menu");
        }
    }

    fn show_help(&self) -> Result<(), String> {
        let colors = self.base.config().colors.clone();

        let mut help = HelpFormatter::new("Homebrew Plugin".to_string());

        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Manage Homebrew packages - install, uninstall, upgrade, and search packages."
                .to_string(),
            vec![],
        );

        help.add_section("Actions".to_string())
            .add_command(
                "list".to_string(),
                "List installed packages".to_string(),
                vec!["lla plugin brew list".to_string()],
            )
            .add_command(
                "search <query>".to_string(),
                "Search for packages".to_string(),
                vec!["lla plugin brew search git".to_string()],
            )
            .add_command(
                "outdated".to_string(),
                "List outdated packages".to_string(),
                vec!["lla plugin brew outdated".to_string()],
            )
            .add_command(
                "install <package>".to_string(),
                "Install a package".to_string(),
                vec![
                    "lla plugin brew install wget".to_string(),
                    "lla plugin brew install firefox --cask".to_string(),
                ],
            )
            .add_command(
                "uninstall <package>".to_string(),
                "Uninstall a package".to_string(),
                vec!["lla plugin brew uninstall wget".to_string()],
            )
            .add_command(
                "upgrade [package]".to_string(),
                "Upgrade packages (all if no package specified)".to_string(),
                vec![
                    "lla plugin brew upgrade".to_string(),
                    "lla plugin brew upgrade wget".to_string(),
                ],
            )
            .add_command(
                "info <package>".to_string(),
                "Show package information".to_string(),
                vec!["lla plugin brew info git".to_string()],
            )
            .add_command(
                "cleanup".to_string(),
                "Remove old versions and clear cache".to_string(),
                vec!["lla plugin brew cleanup".to_string()],
            )
            .add_command(
                "doctor".to_string(),
                "Check system for potential problems".to_string(),
                vec!["lla plugin brew doctor".to_string()],
            )
            .add_command(
                "menu".to_string(),
                "Interactive menu".to_string(),
                vec!["lla plugin brew menu".to_string()],
            )
            .add_command(
                "help".to_string(),
                "Show this help information".to_string(),
                vec!["lla plugin brew help".to_string()],
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

impl Plugin for BrewPlugin {
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
                    PluginRequest::PerformAction(action, args) => {
                        let result = match action.as_str() {
                            "list" => self.list_installed(),
                            "search" => {
                                let query = args.first().map(|s| s.as_str());
                                self.search_packages(query)
                            }
                            "outdated" => self.list_outdated(),
                            "install" => self.install_package(&args),
                            "uninstall" | "remove" => self.uninstall_package(&args),
                            "upgrade" => self.upgrade_packages(&args),
                            "info" => self.package_info(&args),
                            "cleanup" => self.cleanup(),
                            "doctor" => self.run_doctor(),
                            "menu" => self.interactive_menu(),
                            "help" => self.show_help(),
                            _ => Err(format!(
                                "Unknown action: '{}'\n\n\
                                Available actions:\n  \
                                • list       - List installed packages\n  \
                                • search     - Search for packages\n  \
                                • outdated   - Check for updates\n  \
                                • install    - Install a package\n  \
                                • uninstall  - Uninstall a package\n  \
                                • upgrade    - Upgrade packages\n  \
                                • info       - Show package information\n  \
                                • cleanup    - Remove old versions\n  \
                                • doctor     - Run diagnostics\n  \
                                • menu       - Interactive menu\n  \
                                • help       - Show help\n\n\
                                Example: lla plugin brew list",
                                action
                            )),
                        };
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::GetAvailableActions => {
                        use lla_plugin_interface::ActionInfo;
                        PluginResponse::AvailableActions(vec![
                            ActionInfo {
                                name: "list".to_string(),
                                usage: "list".to_string(),
                                description: "List installed packages".to_string(),
                                examples: vec!["lla plugin brew list".to_string()],
                            },
                            ActionInfo {
                                name: "search".to_string(),
                                usage: "search <query>".to_string(),
                                description: "Search for packages".to_string(),
                                examples: vec!["lla plugin brew search git".to_string()],
                            },
                            ActionInfo {
                                name: "outdated".to_string(),
                                usage: "outdated".to_string(),
                                description: "List outdated packages".to_string(),
                                examples: vec!["lla plugin brew outdated".to_string()],
                            },
                            ActionInfo {
                                name: "install".to_string(),
                                usage: "install <package> [--cask]".to_string(),
                                description: "Install a package".to_string(),
                                examples: vec!["lla plugin brew install wget".to_string()],
                            },
                            ActionInfo {
                                name: "uninstall".to_string(),
                                usage: "uninstall <package>".to_string(),
                                description: "Uninstall a package".to_string(),
                                examples: vec!["lla plugin brew uninstall wget".to_string()],
                            },
                            ActionInfo {
                                name: "upgrade".to_string(),
                                usage: "upgrade [package]".to_string(),
                                description: "Upgrade packages".to_string(),
                                examples: vec!["lla plugin brew upgrade".to_string()],
                            },
                            ActionInfo {
                                name: "info".to_string(),
                                usage: "info <package>".to_string(),
                                description: "Show package information".to_string(),
                                examples: vec!["lla plugin brew info git".to_string()],
                            },
                            ActionInfo {
                                name: "cleanup".to_string(),
                                usage: "cleanup".to_string(),
                                description: "Cleanup old versions".to_string(),
                                examples: vec!["lla plugin brew cleanup".to_string()],
                            },
                            ActionInfo {
                                name: "doctor".to_string(),
                                usage: "doctor".to_string(),
                                description: "Check for problems".to_string(),
                                examples: vec!["lla plugin brew doctor".to_string()],
                            },
                            ActionInfo {
                                name: "menu".to_string(),
                                usage: "menu".to_string(),
                                description: "Interactive menu".to_string(),
                                examples: vec!["lla plugin brew menu".to_string()],
                            },
                            ActionInfo {
                                name: "help".to_string(),
                                usage: "help".to_string(),
                                description: "Show help information".to_string(),
                                examples: vec!["lla plugin brew help".to_string()],
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

impl Default for BrewPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for BrewPlugin {
    type Config = BrewConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for BrewPlugin {}

lla_plugin_interface::declare_plugin!(BrewPlugin);

#[derive(Debug, Clone)]
struct CatalogHit {
    kind: String, // "formula" | "cask"
    name: String,
    version: Option<String>,
    desc: Option<String>,
    score: i64,
}
