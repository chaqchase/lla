use arboard::Clipboard;
use colored::Colorize;
use dialoguer::{Input, MultiSelect, Select};
use lla_plugin_interface::{Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, LlaDialoguerTheme},
    BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtToken {
    pub raw: String,
    pub header: serde_json::Value,
    pub payload: serde_json::Value,
    pub signature: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtConfig {
    #[serde(default)]
    pub history: Vec<JwtToken>,
    #[serde(default = "default_max_history")]
    pub max_history_size: usize,
    #[serde(default = "default_true")]
    pub auto_check_expiration: bool,
    #[serde(default = "default_true")]
    pub save_to_history: bool,
    #[serde(default)]
    pub highlight_claims: Vec<String>,
    #[serde(default = "default_colors")]
    pub colors: HashMap<String, String>,
}

fn default_max_history() -> usize {
    50
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
    colors.insert("header".to_string(), "bright_blue".to_string());
    colors.insert("payload".to_string(), "bright_magenta".to_string());
    colors.insert("claim".to_string(), "bright_green".to_string());
    colors.insert("expired".to_string(), "bright_red".to_string());
    colors.insert("valid".to_string(), "bright_green".to_string());
    colors
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            history: Vec::new(),
            max_history_size: default_max_history(),
            auto_check_expiration: true,
            save_to_history: true,
            highlight_claims: vec![
                "sub".to_string(),
                "iss".to_string(),
                "aud".to_string(),
                "exp".to_string(),
                "iat".to_string(),
                "nbf".to_string(),
            ],
            colors: default_colors(),
        }
    }
}

impl PluginConfig for JwtConfig {}

pub struct JwtPlugin {
    base: BasePlugin<JwtConfig>,
}

impl JwtPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[JwtPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn decode_jwt(&self, token: &str) -> Result<JwtToken, String> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(
                "Invalid JWT format. Expected format: header.payload.signature".to_string(),
            );
        }

        let header = self.decode_base64_json(parts[0])?;
        let payload = self.decode_base64_json(parts[1])?;
        let signature = parts[2].to_string();

        Ok(JwtToken {
            raw: token.to_string(),
            header,
            payload,
            signature,
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        })
    }

    fn decode_base64_json(&self, encoded: &str) -> Result<serde_json::Value, String> {
        use base64::{engine::general_purpose, Engine as _};

        // JWT uses base64url encoding
        let padded = match encoded.len() % 4 {
            2 => format!("{}==", encoded),
            3 => format!("{}=", encoded),
            _ => encoded.to_string(),
        };

        let replaced = padded.replace('-', "+").replace('_', "/");
        let decoded = general_purpose::STANDARD
            .decode(&replaced)
            .map_err(|e| format!("Failed to decode base64: {}", e))?;

        let json: serde_json::Value =
            serde_json::from_slice(&decoded).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        Ok(json)
    }

    fn check_expiration(&self, payload: &serde_json::Value) -> Option<(bool, String)> {
        if let Some(exp) = payload.get("exp").and_then(|v| v.as_i64()) {
            let exp_time = chrono::DateTime::from_timestamp(exp, 0)?;
            let now = chrono::Utc::now();
            let is_expired = now > exp_time;

            let time_diff = if is_expired {
                let duration = now.signed_duration_since(exp_time);
                format!("Expired {} ago", Self::format_duration(duration))
            } else {
                let duration = exp_time.signed_duration_since(now);
                format!("Expires in {}", Self::format_duration(duration))
            };

            Some((is_expired, time_diff))
        } else {
            None
        }
    }

    fn format_duration(duration: chrono::Duration) -> String {
        let abs_duration = duration.abs();
        if abs_duration.num_days() > 0 {
            format!("{} days", abs_duration.num_days())
        } else if abs_duration.num_hours() > 0 {
            format!("{} hours", abs_duration.num_hours())
        } else if abs_duration.num_minutes() > 0 {
            format!("{} minutes", abs_duration.num_minutes())
        } else {
            format!("{} seconds", abs_duration.num_seconds())
        }
    }

    fn format_json_pretty(&self, value: &serde_json::Value, indent: usize) -> String {
        let indent_str = "  ".repeat(indent);
        let next_indent = indent + 1;

        match value {
            serde_json::Value::Object(map) => {
                let mut lines = vec!["{".to_string()];
                let highlight_claims = &self.base.config().highlight_claims;

                for (i, (key, val)) in map.iter().enumerate() {
                    let is_last = i == map.len() - 1;
                    let comma = if is_last { "" } else { "," };

                    let key_colored = if highlight_claims.contains(key) {
                        key.bright_yellow().bold()
                    } else {
                        key.bright_cyan()
                    };

                    let formatted_val = self.format_json_value(val);
                    lines.push(format!(
                        "{}{}: {}{}",
                        "  ".repeat(next_indent),
                        key_colored,
                        formatted_val,
                        comma
                    ));
                }
                lines.push(format!("{}}}", indent_str));
                lines.join("\n")
            }
            _ => self.format_json_value(value).to_string(),
        }
    }

    fn format_json_value(&self, value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(s) => format!("\"{}\"", s.bright_green()),
            serde_json::Value::Number(n) => n.to_string().bright_magenta().to_string(),
            serde_json::Value::Bool(b) => b.to_string().bright_blue().to_string(),
            serde_json::Value::Null => "null".bright_black().to_string(),
            serde_json::Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| self.format_json_value(v)).collect();
                format!("[{}]", items.join(", "))
            }
            serde_json::Value::Object(_) => serde_json::to_string_pretty(value).unwrap_or_default(),
        }
    }

    fn display_jwt(&self, jwt: &JwtToken) {
        println!("\n{}", "═".repeat(80).bright_cyan());
        println!("{}", "  🔐 JWT TOKEN DETAILS".bright_cyan().bold());
        println!("{}", "═".repeat(80).bright_cyan());

        // Header Section
        println!(
            "\n{}",
            "┌─ HEADER ─────────────────────────────────────────────────────────────┐"
                .bright_blue()
        );
        println!("{}", self.format_json_pretty(&jwt.header, 0));
        println!(
            "{}",
            "└──────────────────────────────────────────────────────────────────────┘"
                .bright_blue()
        );

        // Payload Section
        println!(
            "\n{}",
            "┌─ PAYLOAD ────────────────────────────────────────────────────────────┐"
                .bright_magenta()
        );
        println!("{}", self.format_json_pretty(&jwt.payload, 0));
        println!(
            "{}",
            "└──────────────────────────────────────────────────────────────────────┘"
                .bright_magenta()
        );

        // Expiration Check
        if self.base.config().auto_check_expiration {
            if let Some((is_expired, message)) = self.check_expiration(&jwt.payload) {
                let status_icon = if is_expired { "❌" } else { "✅" };
                let status_color = if is_expired {
                    message.bright_red()
                } else {
                    message.bright_green()
                };
                println!("\n{} {}", status_icon, status_color);
            }
        }

        // Signature Section
        println!(
            "\n{}",
            "┌─ SIGNATURE ──────────────────────────────────────────────────────────┐"
                .bright_yellow()
        );
        println!("  {}", jwt.signature.bright_black());
        println!(
            "{}",
            "└──────────────────────────────────────────────────────────────────────┘"
                .bright_yellow()
        );

        println!("\n{}", "═".repeat(80).bright_cyan());
    }

    fn add_to_history(&mut self, jwt: JwtToken) {
        if !self.base.config().save_to_history {
            return;
        }

        let config = self.base.config_mut();

        // Remove duplicate if exists
        config.history.retain(|j| j.raw != jwt.raw);

        // Add new entry at the beginning
        config.history.insert(0, jwt);

        // Trim history if needed
        if config.history.len() > config.max_history_size {
            config.history.truncate(config.max_history_size);
        }

        if let Err(e) = self.base.save_config() {
            eprintln!("Failed to save history: {}", e);
        }
    }

    fn get_jwt_input(&self) -> Result<String, String> {
        let theme = LlaDialoguerTheme::default();

        let options = vec![
            "📝 Paste JWT token",
            "📋 Use clipboard",
            "📜 Select from history",
        ];

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Choose input method", "🔐".bright_cyan()))
            .items(&options)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show menu: {}", e))?;

        match selection {
            0 => {
                let token: String = Input::with_theme(&theme)
                    .with_prompt("Paste JWT token")
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;

                if token.trim().is_empty() {
                    return Err("Token cannot be empty".to_string());
                }

                Ok(token.trim().to_string())
            }
            1 => {
                let mut clipboard =
                    Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;
                let token = clipboard
                    .get_text()
                    .map_err(|e| format!("Failed to get clipboard text: {}", e))?;

                if token.trim().is_empty() {
                    return Err("Clipboard is empty".to_string());
                }

                Ok(token.trim().to_string())
            }
            2 => {
                let history = &self.base.config().history;
                if history.is_empty() {
                    return Err("No JWT tokens in history".to_string());
                }

                let items: Vec<String> = history
                    .iter()
                    .enumerate()
                    .map(|(i, jwt)| {
                        let preview = jwt.raw.chars().take(50).collect::<String>();
                        let exp_status =
                            if let Some((expired, _)) = self.check_expiration(&jwt.payload) {
                                if expired {
                                    "❌"
                                } else {
                                    "✅"
                                }
                            } else {
                                "⚪"
                            };
                        format!(
                            "{} {} {}... {}",
                            exp_status,
                            format!("#{}", i + 1).bright_black(),
                            preview,
                            format!("({})", jwt.timestamp).bright_black()
                        )
                    })
                    .collect();

                let selection = Select::with_theme(&theme)
                    .with_prompt(format!("{} Select JWT from history", "📜".bright_cyan()))
                    .items(&items)
                    .default(0)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                Ok(history[selection].raw.clone())
            }
            _ => unreachable!(),
        }
    }

    fn view_decoded_jwt(&mut self) -> Result<(), String> {
        let token = self.get_jwt_input()?;
        let jwt = self.decode_jwt(&token)?;

        self.display_jwt(&jwt);

        let theme = LlaDialoguerTheme::default();
        let actions = vec![
            "📋 Copy header",
            "📋 Copy payload",
            "📋 Copy full token",
            "📋 Copy specific claim",
            "💾 Save to history",
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
                let content = serde_json::to_string_pretty(&jwt.header)
                    .unwrap_or_else(|_| jwt.header.to_string());
                self.copy_to_clipboard(&content)?;
                println!("{} Header copied to clipboard!", "Success:".bright_green());
            }
            1 => {
                let content = serde_json::to_string_pretty(&jwt.payload)
                    .unwrap_or_else(|_| jwt.payload.to_string());
                self.copy_to_clipboard(&content)?;
                println!("{} Payload copied to clipboard!", "Success:".bright_green());
            }
            2 => {
                self.copy_to_clipboard(&jwt.raw)?;
                println!(
                    "{} Full token copied to clipboard!",
                    "Success:".bright_green()
                );
            }
            3 => {
                if let Some(obj) = jwt.payload.as_object() {
                    let claims: Vec<String> = obj.keys().cloned().collect();

                    let claim_selection = Select::with_theme(&theme)
                        .with_prompt("Select claim to copy")
                        .items(&claims)
                        .interact()
                        .map_err(|e| format!("Failed to show selection: {}", e))?;

                    let claim_name = &claims[claim_selection];
                    let claim_value = obj.get(claim_name).unwrap();
                    let content = serde_json::to_string_pretty(claim_value)
                        .unwrap_or_else(|_| claim_value.to_string());

                    self.copy_to_clipboard(&content)?;
                    println!(
                        "{} Claim '{}' copied to clipboard!",
                        "Success:".bright_green(),
                        claim_name.bright_yellow()
                    );
                } else {
                    return Err("Payload is not an object".to_string());
                }
            }
            4 => {
                self.add_to_history(jwt);
                println!("{} JWT saved to history!", "Success:".bright_green());
            }
            5 => {}
            _ => unreachable!(),
        }

        Ok(())
    }

    fn search_decoded_jwt(&mut self) -> Result<(), String> {
        let token = self.get_jwt_input()?;
        let jwt = self.decode_jwt(&token)?;

        let theme = LlaDialoguerTheme::default();
        let search_term: String = Input::with_theme(&theme)
            .with_prompt("Enter search term (regex supported)")
            .interact_text()
            .map_err(|e| format!("Failed to get input: {}", e))?;

        if search_term.trim().is_empty() {
            return Err("Search term cannot be empty".to_string());
        }

        let regex =
            Regex::new(&search_term).map_err(|e| format!("Invalid regex pattern: {}", e))?;

        let mut matches = Vec::new();

        // Search in header
        self.search_in_json(&jwt.header, &regex, "header", &mut matches);

        // Search in payload
        self.search_in_json(&jwt.payload, &regex, "payload", &mut matches);

        if matches.is_empty() {
            println!(
                "{} No matches found for '{}'",
                "Info:".bright_cyan(),
                search_term.bright_yellow()
            );
            return Ok(());
        }

        println!("\n{}", "═".repeat(80).bright_cyan());
        println!(
            "{} Found {} matches for '{}'",
            "🔍".bright_cyan(),
            matches.len().to_string().bright_green().bold(),
            search_term.bright_yellow()
        );
        println!("{}", "═".repeat(80).bright_cyan());

        for (location, key, value) in &matches {
            let location_color = if location == "header" {
                location.bright_blue()
            } else {
                location.bright_magenta()
            };

            println!(
                "\n{} {} » {}",
                "►".bright_cyan(),
                location_color,
                key.bright_yellow()
            );
            println!("  {}", self.format_json_value(value));
        }

        println!("\n{}", "═".repeat(80).bright_cyan());

        // Copy options
        let actions = vec![
            "📋 Copy all matches",
            "📋 Copy specific match",
            "💾 Save JWT to history",
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
                let content = matches
                    .iter()
                    .map(|(loc, key, val)| {
                        format!(
                            "[{}] {}: {}",
                            loc,
                            key,
                            serde_json::to_string_pretty(val).unwrap_or_else(|_| val.to_string())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                self.copy_to_clipboard(&content)?;
                println!(
                    "{} {} matches copied to clipboard!",
                    "Success:".bright_green(),
                    matches.len()
                );
            }
            1 => {
                let items: Vec<String> = matches
                    .iter()
                    .map(|(loc, key, val)| {
                        format!("[{}] {} = {}", loc, key, self.format_json_value(val))
                    })
                    .collect();

                let match_selection = Select::with_theme(&theme)
                    .with_prompt("Select match to copy")
                    .items(&items)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                let (_, _, value) = &matches[match_selection];
                let content =
                    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
                self.copy_to_clipboard(&content)?;
                println!("{} Match copied to clipboard!", "Success:".bright_green());
            }
            2 => {
                self.add_to_history(jwt);
                println!("{} JWT saved to history!", "Success:".bright_green());
            }
            3 => {}
            _ => unreachable!(),
        }

        Ok(())
    }

    fn search_in_json(
        &self,
        value: &serde_json::Value,
        regex: &Regex,
        location: &str,
        matches: &mut Vec<(String, String, serde_json::Value)>,
    ) {
        if let Some(obj) = value.as_object() {
            for (key, val) in obj {
                // Check if key matches
                if regex.is_match(key) {
                    matches.push((location.to_string(), key.clone(), val.clone()));
                }

                // Check if value matches (convert to string first)
                let val_str = match val {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => serde_json::to_string(val).unwrap_or_default(),
                };

                if regex.is_match(&val_str) {
                    matches.push((location.to_string(), key.clone(), val.clone()));
                }

                // Recursively search nested objects
                if val.is_object() || val.is_array() {
                    self.search_in_json(val, regex, location, matches);
                }
            }
        } else if let Some(arr) = value.as_array() {
            for val in arr {
                self.search_in_json(val, regex, location, matches);
            }
        }
    }

    fn copy_to_clipboard(&self, content: &str) -> Result<(), String> {
        let mut clipboard =
            Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;
        clipboard
            .set_text(content)
            .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;
        Ok(())
    }

    fn manage_history(&mut self) -> Result<(), String> {
        let history = self.base.config().history.clone();

        if history.is_empty() {
            println!("{} No JWT tokens in history", "Info:".bright_cyan());
            return Ok(());
        }

        let theme = LlaDialoguerTheme::default();

        let items: Vec<String> = history
            .iter()
            .enumerate()
            .map(|(i, jwt)| {
                let preview = jwt.raw.chars().take(40).collect::<String>();
                let exp_status = if let Some((expired, msg)) = self.check_expiration(&jwt.payload) {
                    if expired {
                        format!("❌ {}", msg).bright_red().to_string()
                    } else {
                        format!("✅ {}", msg).bright_green().to_string()
                    }
                } else {
                    "⚪ No expiration".bright_black().to_string()
                };
                format!(
                    "{} {}... | {} | {}",
                    format!("#{}", i + 1).bright_black(),
                    preview,
                    exp_status,
                    jwt.timestamp.bright_black()
                )
            })
            .collect();

        let actions = vec![
            "👁️  View token details",
            "📋 Copy token",
            "🗑️  Remove from history",
            "🧹 Clear all history",
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
                let token_selection = Select::with_theme(&theme)
                    .with_prompt(format!("{} Select token", "👁️ ".bright_cyan()))
                    .items(&items)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                let jwt = &history[token_selection];
                self.display_jwt(jwt);
            }
            1 => {
                let token_selection = MultiSelect::with_theme(&theme)
                    .with_prompt(format!("{} Select tokens to copy", "📋".bright_cyan()))
                    .items(&items)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                if token_selection.is_empty() {
                    println!("{} No tokens selected", "Info:".bright_blue());
                    return Ok(());
                }

                let tokens: Vec<String> = token_selection
                    .iter()
                    .map(|&i| history[i].raw.clone())
                    .collect();
                let content = tokens.join("\n\n");
                self.copy_to_clipboard(&content)?;
                println!(
                    "{} {} tokens copied to clipboard!",
                    "Success:".bright_green(),
                    token_selection.len()
                );
            }
            2 => {
                let token_selection = MultiSelect::with_theme(&theme)
                    .with_prompt(format!("{} Select tokens to remove", "🗑️ ".bright_cyan()))
                    .items(&items)
                    .interact()
                    .map_err(|e| format!("Failed to show selection: {}", e))?;

                if token_selection.is_empty() {
                    println!("{} No tokens selected", "Info:".bright_blue());
                    return Ok(());
                }

                let config = self.base.config_mut();
                let mut indices = token_selection;
                indices.sort_unstable_by(|a, b| b.cmp(a));
                for &i in &indices {
                    config.history.remove(i);
                }

                self.base.save_config()?;
                println!(
                    "{} {} tokens removed!",
                    "Success:".bright_green(),
                    indices.len()
                );
            }
            3 => {
                let confirm: bool = dialoguer::Confirm::with_theme(&theme)
                    .with_prompt("Are you sure you want to clear all history?")
                    .default(false)
                    .interact()
                    .map_err(|e| format!("Failed to get confirmation: {}", e))?;

                if confirm {
                    self.base.config_mut().history.clear();
                    self.base.save_config()?;
                    println!("{} All history cleared!", "Success:".bright_green());
                } else {
                    println!("{} Operation cancelled", "Info:".bright_blue());
                }
            }
            4 => {}
            _ => unreachable!(),
        }

        Ok(())
    }

    fn configure_preferences(&mut self) -> Result<(), String> {
        let theme = LlaDialoguerTheme::default();

        let options = vec![
            format!(
                "Auto-check expiration: {}",
                if self.base.config().auto_check_expiration {
                    "✓ Enabled".bright_green()
                } else {
                    "✗ Disabled".bright_red()
                }
            ),
            format!(
                "Save to history: {}",
                if self.base.config().save_to_history {
                    "✓ Enabled".bright_green()
                } else {
                    "✗ Disabled".bright_red()
                }
            ),
            format!(
                "Max history size: {}",
                self.base
                    .config()
                    .max_history_size
                    .to_string()
                    .bright_yellow()
            ),
            "Manage highlighted claims".to_string(),
            "← Back".to_string(),
        ];

        let selection = Select::with_theme(&theme)
            .with_prompt(format!("{} Configure Preferences", "⚙️ ".bright_cyan()))
            .items(&options)
            .default(0)
            .interact()
            .map_err(|e| format!("Failed to show preferences: {}", e))?;

        match selection {
            0 => {
                let new_value = {
                    let config = self.base.config_mut();
                    config.auto_check_expiration = !config.auto_check_expiration;
                    config.auto_check_expiration
                };
                self.base.save_config()?;
                println!(
                    "{} Auto-check expiration: {}",
                    "Success:".bright_green(),
                    if new_value { "enabled" } else { "disabled" }
                );
            }
            1 => {
                let new_value = {
                    let config = self.base.config_mut();
                    config.save_to_history = !config.save_to_history;
                    config.save_to_history
                };
                self.base.save_config()?;
                println!(
                    "{} Save to history: {}",
                    "Success:".bright_green(),
                    if new_value { "enabled" } else { "disabled" }
                );
            }
            2 => {
                let input: usize = Input::with_theme(&theme)
                    .with_prompt("Enter max history size")
                    .default(self.base.config().max_history_size)
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;

                let config = self.base.config_mut();
                config.max_history_size = input;
                if config.history.len() > config.max_history_size {
                    config.history.truncate(config.max_history_size);
                }

                self.base.save_config()?;
                println!(
                    "{} Max history size set to: {}",
                    "Success:".bright_green(),
                    input
                );
            }
            3 => {
                let current_claims = self.base.config().highlight_claims.clone();
                println!("\n{} Current highlighted claims:", "Info:".bright_cyan());
                for claim in &current_claims {
                    println!("  • {}", claim.bright_yellow());
                }

                let input: String = Input::with_theme(&theme)
                    .with_prompt("Enter claims to highlight (comma-separated)")
                    .default(current_claims.join(", "))
                    .interact_text()
                    .map_err(|e| format!("Failed to get input: {}", e))?;

                let claims: Vec<String> = input
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                self.base.config_mut().highlight_claims = claims.clone();
                self.base.save_config()?;

                println!("{} Highlighted claims updated:", "Success:".bright_green());
                for claim in &claims {
                    println!("  • {}", claim.bright_yellow());
                }
            }
            4 => {}
            _ => unreachable!(),
        }

        Ok(())
    }

    fn show_help(&self) -> Result<(), String> {
        let colors = self.base.config().colors.clone();

        let mut help = HelpFormatter::new("JWT Plugin".to_string());

        help.add_section("Description".to_string()).add_command(
            "".to_string(),
            "Decode, analyze, and search JWT tokens with beautiful formatting.".to_string(),
            vec![],
        );

        help.add_section("Actions".to_string())
            .add_command(
                "decode".to_string(),
                "View decoded JWT with formatted output".to_string(),
                vec!["decode".to_string()],
            )
            .add_command(
                "search".to_string(),
                "Search JWT contents with regex support".to_string(),
                vec!["search".to_string()],
            )
            .add_command(
                "history".to_string(),
                "Manage JWT token history".to_string(),
                vec!["history".to_string()],
            )
            .add_command(
                "preferences".to_string(),
                "Configure plugin settings".to_string(),
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

impl Plugin for JwtPlugin {
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
                            "decode" => self.view_decoded_jwt(),
                            "search" => self.search_decoded_jwt(),
                            "history" => self.manage_history(),
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
                                name: "decode".to_string(),
                                usage: "decode".to_string(),
                                description: "View decoded JWT tokens".to_string(),
                                examples: vec!["lla plugin jwt decode".to_string()],
                            },
                            ActionInfo {
                                name: "search".to_string(),
                                usage: "search".to_string(),
                                description: "Search decoded JWT tokens".to_string(),
                                examples: vec!["lla plugin jwt search".to_string()],
                            },
                            ActionInfo {
                                name: "history".to_string(),
                                usage: "history".to_string(),
                                description: "Manage token history".to_string(),
                                examples: vec!["lla plugin jwt history".to_string()],
                            },
                            ActionInfo {
                                name: "preferences".to_string(),
                                usage: "preferences".to_string(),
                                description: "Configure preferences".to_string(),
                                examples: vec!["lla plugin jwt preferences".to_string()],
                            },
                            ActionInfo {
                                name: "help".to_string(),
                                usage: "help".to_string(),
                                description: "Show help information".to_string(),
                                examples: vec!["lla plugin jwt help".to_string()],
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

impl Default for JwtPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for JwtPlugin {
    type Config = JwtConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for JwtPlugin {}

lla_plugin_interface::declare_plugin!(JwtPlugin);
