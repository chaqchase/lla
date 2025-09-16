use crate::config::Config;
use crate::error::{LlaError, Result};
use dialoguer::Select;
use lla_plugin_utils::ui::components::LlaDialoguerTheme;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
struct JumpStore {
    #[serde(default)]
    bookmarks: Vec<PathBuf>,
    #[serde(default)]
    history: Vec<PathBuf>,
}

impl JumpStore {
    fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("lla")
            .join("jump.json")
    }

    fn load() -> Self {
        let p = Self::path();
        if let Ok(s) = fs::read_to_string(&p) {
            if let Ok(store) = serde_json::from_str::<JumpStore>(&s) {
                return store;
            }
        }
        JumpStore::default()
    }

    fn save(&self) -> std::result::Result<(), String> {
        let p = Self::path();
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {}", e))?;
        }
        let s = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize store: {}", e))?;
        let mut f = fs::File::create(&p).map_err(|e| format!("Failed to open store: {}", e))?;
        f.write_all(s.as_bytes())
            .map_err(|e| format!("Failed to write store: {}", e))
    }
}

fn canonicalize_best_effort(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn is_excluded(path: &Path, config: &Config) -> bool {
    let abs = canonicalize_best_effort(path);
    config.exclude_paths.iter().any(|ex| abs.starts_with(ex))
}

fn sanitize_list<'a>(list: impl IntoIterator<Item = &'a PathBuf>, config: &Config) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in list {
        if p.exists() && !is_excluded(p, config) {
            let abs = canonicalize_best_effort(p);
            if !out.iter().any(|q| q == &abs) {
                out.push(abs);
            }
        }
    }
    out
}

pub fn record_visit(dir: &str, config: &Config) {
    let path = Path::new(dir);
    if !path.is_dir() {
        return;
    }
    if is_excluded(path, config) {
        return;
    }

    let mut store = JumpStore::load();
    let abs = canonicalize_best_effort(path);
    // Remove existing entry if present and push front
    store.history.retain(|p| p != &abs);
    store.history.insert(0, abs);
    // Cap history length
    if store.history.len() > 500 {
        store.history.truncate(500);
    }
    let _ = store.save();
}

pub fn handle_jump(action: &crate::commands::args::JumpAction, config: &Config) -> Result<()> {
    match action {
        crate::commands::args::JumpAction::Prompt => prompt_and_print(config),
        crate::commands::args::JumpAction::Add(path) => add_bookmark(path, config),
        crate::commands::args::JumpAction::Remove(path) => remove_bookmark(path),
        crate::commands::args::JumpAction::List => list_entries(config),
        crate::commands::args::JumpAction::ClearHistory => clear_history(),
        crate::commands::args::JumpAction::Setup(shell) => {
            setup_shell_integration(shell.as_deref())
        }
    }
}

fn add_bookmark(path: &str, config: &Config) -> Result<()> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(LlaError::Other(format!("Path does not exist: {}", path)));
    }
    if is_excluded(p, config) {
        return Err(LlaError::Other(format!(
            "Path is excluded by exclude_paths: {}",
            path
        )));
    }
    let mut store = JumpStore::load();
    let abs = canonicalize_best_effort(p);
    if !store.bookmarks.iter().any(|q| q == &abs) {
        store.bookmarks.push(abs);
    }
    store
        .save()
        .map_err(|e| LlaError::Other(format!("{}", e)))?;
    Ok(())
}

fn remove_bookmark(path: &str) -> Result<()> {
    let p = Path::new(path);
    let abs = canonicalize_best_effort(p);
    let mut store = JumpStore::load();
    store.bookmarks.retain(|q| q != &abs);
    store
        .save()
        .map_err(|e| LlaError::Other(format!("{}", e)))?;
    Ok(())
}

fn list_entries(config: &Config) -> Result<()> {
    let store = JumpStore::load();
    let mut bookmarks = sanitize_list(&store.bookmarks, config);
    let mut history = sanitize_list(&store.history, config);
    // Remove history entries that are bookmarks to avoid duplication
    history.retain(|h| !bookmarks.iter().any(|b| b == h));

    for b in bookmarks.drain(..) {
        println!("★ {}", b.display());
    }
    for h in history.drain(..) {
        println!("  {}", h.display());
    }
    Ok(())
}

fn clear_history() -> Result<()> {
    let mut store = JumpStore::load();
    store.history.clear();
    store
        .save()
        .map_err(|e| LlaError::Other(format!("{}", e)))?;
    Ok(())
}

fn prompt_and_print(config: &Config) -> Result<()> {
    let store = JumpStore::load();
    let bookmarks = sanitize_list(&store.bookmarks, config);
    let mut history = sanitize_list(&store.history, config);
    history.retain(|h| !bookmarks.iter().any(|b| b == h));

    // Build display list: favorites first, then recent
    let mut items: Vec<(String, PathBuf)> = Vec::new();
    for p in bookmarks.iter() {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let label = format!("★ {} — {}", name, p.display());
        items.push((label, p.clone()));
    }
    for p in history.iter() {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let label = format!("{} — {}", name, p.display());
        items.push((label, p.clone()));
    }

    if items.is_empty() {
        return Err(LlaError::Other(
            "No bookmarks or history yet. Add one with: lla jump --add <PATH>".to_string(),
        ));
    }

    // Interactive selection
    let theme = LlaDialoguerTheme::default();
    let idx = Select::with_theme(&theme)
        .with_prompt("Jump to directory")
        .items(&items.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>())
        .default(0)
        .interact_opt()?;

    if let Some(i) = idx {
        let path = &items[i].1;
        println!("{}", path.display());
    }
    Ok(())
}

fn setup_shell_integration(shell_override: Option<&str>) -> Result<()> {
    use std::env;

    println!("🚀 Setting up lla jump shell integration...\n");

    // Detect shell (override > env variables > SHELL)
    let shell = if let Some(s) = shell_override {
        s.to_string()
    } else if env::var("FISH_VERSION").is_ok() {
        "fish".to_string()
    } else if env::var("ZSH_VERSION").is_ok() {
        "zsh".to_string()
    } else if env::var("BASH_VERSION").is_ok() {
        "bash".to_string()
    } else {
        env::var("SHELL")
            .unwrap_or_default()
            .split('/')
            .last()
            .unwrap_or("unknown")
            .to_string()
    };

    match shell.as_str() {
        "bash" => setup_bash(),
        "zsh" => setup_zsh(),
        "fish" => setup_fish(),
        _ => {
            println!("❌ Unsupported shell: {}", shell);
            println!("   Supported shells: bash, zsh, fish");
            println!("   Please set up manually using the documentation.");
            return Ok(());
        }
    }
}

fn setup_bash() -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let home = dirs::home_dir()
        .ok_or_else(|| LlaError::Other("Could not determine home directory".to_string()))?;

    let bashrc_path = home.join(".bashrc");
    let function_content = r#"
# lla jump function - added by lla jump --setup
j() {
    local dir=$(lla jump)
    if [ -n "$dir" ] && [ -d "$dir" ]; then
        cd "$dir"
    fi
}
"#;

    // Check if function already exists
    if bashrc_path.exists() {
        let content = fs::read_to_string(&bashrc_path)?;
        if content.contains("lla jump function") {
            println!("✅ Shell integration already set up in ~/.bashrc");
            println!("   Use 'j' to jump to directories interactively!");
            return Ok(());
        }
    }

    // Append function to .bashrc
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&bashrc_path)?;

    file.write_all(function_content.as_bytes())?;

    println!("✅ Added shell integration to ~/.bashrc");
    println!("   Run 'source ~/.bashrc' or restart your terminal");
    println!("   Then use 'j' to jump to directories interactively!");

    Ok(())
}

fn setup_zsh() -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let home = dirs::home_dir()
        .ok_or_else(|| LlaError::Other("Could not determine home directory".to_string()))?;

    let zshrc_path = home.join(".zshrc");
    let function_content = r#"
# lla jump function - added by lla jump --setup
j() {
    local dir=$(lla jump)
    if [ -n "$dir" ] && [ -d "$dir" ]; then
        cd "$dir"
    fi
}
"#;

    // Check if function already exists
    if zshrc_path.exists() {
        let content = fs::read_to_string(&zshrc_path)?;
        if content.contains("lla jump function") {
            println!("✅ Shell integration already set up in ~/.zshrc");
            println!("   Use 'j' to jump to directories interactively!");
            return Ok(());
        }
    }

    // Append function to .zshrc
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&zshrc_path)?;

    file.write_all(function_content.as_bytes())?;

    println!("✅ Added shell integration to ~/.zshrc");
    println!("   Run 'source ~/.zshrc' or restart your terminal");
    println!("   Then use 'j' to jump to directories interactively!");

    Ok(())
}

fn setup_fish() -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let home = dirs::home_dir()
        .ok_or_else(|| LlaError::Other("Could not determine home directory".to_string()))?;

    let config_dir = home.join(".config").join("fish");
    let config_path = config_dir.join("config.fish");

    let function_content = r#"
# lla jump function - added by lla jump --setup
function j
    set dir (lla jump)
    if test -n "$dir" -a -d "$dir"
        cd "$dir"
    end
end
"#;

    // Ensure config directory exists
    fs::create_dir_all(&config_dir)?;

    // Check if function already exists
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        if content.contains("lla jump function") {
            println!("✅ Shell integration already set up in ~/.config/fish/config.fish");
            println!("   Use 'j' to jump to directories interactively!");
            return Ok(());
        }
    }

    // Append function to config.fish
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config_path)?;

    file.write_all(function_content.as_bytes())?;

    println!("✅ Added shell integration to ~/.config/fish/config.fish");
    println!("   Restart your terminal or run 'source ~/.config/fish/config.fish'");
    println!("   Then use 'j' to jump to directories interactively!");

    Ok(())
}
