use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, value, ActionArguments, DecoratedEntryExt, Plugin};
use lla_plugin_utils::{
    config::PluginConfig,
    decode_decorated_entry, run_cli_action,
    ui::components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
    ActionRegistry, BasePlugin, ConfigurablePlugin,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

lazy_static! {
    static ref SPINNER: RwLock<Spinner> = RwLock::new(Spinner::new());
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            ["lla plugin run last_git_commit help"],
            |_| {
                let mut help = HelpFormatter::new("Last Git Commit Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Shows information about the last Git commit for files.".to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string()).add_command(
                    "help".to_string(),
                    "Show this help information".to_string(),
                    vec!["lla plugin run last_git_commit help".to_string()],
                );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic commit information".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show detailed commit information including author".to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&CommitConfig::default().colors))
                        .style(BoxStyle::Minimal)
                        .padding(2)
                        .render()
                );
                Ok(())
            }
        );

        registry
    });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("hash".to_string(), "bright_yellow".to_string());
    colors.insert("author".to_string(), "bright_cyan".to_string());
    colors.insert("time".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

impl Default for CommitConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for CommitConfig {}

pub struct LastGitCommitPlugin {
    base: BasePlugin<CommitConfig>,
    commit_cache: lla_plugin_utils::PersistentCache<Option<CommitInfo>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CommitInfo {
    hash: String,
    author: String,
    time: String,
}

impl LastGitCommitPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
            commit_cache: lla_plugin_utils::PersistentCache::for_plugin(
                plugin_name,
                "commit-cache.toml",
                1,
                50_000,
            ),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[LastGitCommitPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn git_root(path: &Path) -> Option<PathBuf> {
        let start = if path.is_dir() { path } else { path.parent()? };
        start
            .ancestors()
            .find(|directory| directory.join(".git").exists())
            .map(|directory| {
                directory
                    .canonicalize()
                    .unwrap_or_else(|_| directory.to_path_buf())
            })
    }

    fn decorate_batch_entries(
        &mut self,
        mut entries: Vec<proto::DecoratedEntry>,
    ) -> Vec<proto::DecoratedEntry> {
        let mut repositories = BTreeMap::<PathBuf, Vec<(usize, String, String)>>::new();
        for (index, entry) in entries.iter_mut().enumerate() {
            let path = Path::new(&entry.path);
            let Some(root) = Self::git_root(path) else {
                continue;
            };
            let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let relative = absolute
                .strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let cache_key = format!("{}\0{relative}", root.to_string_lossy());
            repositories
                .entry(root)
                .or_default()
                .push((index, relative, cache_key));
        }

        for (root, requested) in repositories {
            let Some(fingerprint) = git_head_fingerprint(&root) else {
                continue;
            };
            let mut misses = Vec::new();
            for (index, relative, cache_key) in &requested {
                match self.commit_cache.get(cache_key, &fingerprint) {
                    Some(Some(info)) => decorate_commit_entry(&mut entries[*index], &info),
                    Some(None) => {}
                    None => misses.push((*index, relative.clone(), cache_key.clone())),
                }
            }
            if misses.is_empty() {
                continue;
            }
            let paths = misses
                .iter()
                .map(|(_, relative, _)| relative.clone())
                .collect::<Vec<_>>();
            let found = batched_git_log(&root, &paths);
            for (index, relative, cache_key) in misses {
                let info = found.get(&relative).cloned();
                if let Some(info) = info.as_ref() {
                    decorate_commit_entry(&mut entries[index], info);
                }
                self.commit_cache.insert(cache_key, &fingerprint, info);
            }
        }
        let _ = self.commit_cache.persist();
        entries
    }

    fn format_relative_time(value: &str) -> String {
        let Some(timestamp) = value.parse::<u64>().ok() else {
            return value.to_string();
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let elapsed = now.saturating_sub(timestamp);
        if elapsed < 60 {
            format!("{} seconds ago", elapsed)
        } else if elapsed < 3_600 {
            format!("{} minutes ago", elapsed / 60)
        } else if elapsed < 86_400 {
            format!("{} hours ago", elapsed / 3_600)
        } else {
            format!("{} days ago", elapsed / 86_400)
        }
    }

    fn format_commit_info(
        &self,
        entry: &lla_plugin_utils::DecoratedEntry,
        format: &str,
    ) -> Option<String> {
        let colors = &self.base.config().colors;
        let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

        if let (Some(hash), Some(author), Some(time)) = (
            entry.custom_fields.get("commit_hash"),
            entry.custom_fields.get("commit_author"),
            entry.custom_fields.get("commit_time"),
        ) {
            let time_display = Self::format_relative_time(time);
            match format {
                "long" => {
                    let key_color = colors
                        .get("info")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let hash_color = colors
                        .get("hash")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let kv = KeyValue::new("Commit", hash)
                        .key_color(&key_color)
                        .value_color(&hash_color)
                        .key_width(12);
                    list.add_item(kv.render());

                    let author_color = colors
                        .get("author")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let kv = KeyValue::new("Author", author)
                        .key_color(&key_color)
                        .value_color(&author_color)
                        .key_width(12);
                    list.add_item(kv.render());

                    let time_color = colors
                        .get("time")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let kv = KeyValue::new("Time", &time_display)
                        .key_color(&key_color)
                        .value_color(&time_color)
                        .key_width(12);
                    list.add_item(kv.render());
                }
                "default" => {
                    let key_color = colors
                        .get("info")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let hash_color = colors
                        .get("hash")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let kv = KeyValue::new("Commit", format!("{} {}", hash, time_display))
                        .key_color(&key_color)
                        .value_color(&hash_color)
                        .key_width(12);
                    list.add_item(kv.render());
                }
                _ => return None,
            }

            Some(format!("\n{}", list.render()))
        } else {
            None
        }
    }
}

impl Plugin for LastGitCommitPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        self.decorate_batch(vec![entry], "default")
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        let spinner = SPINNER.write();
        spinner.set_status("Checking last commit...".to_string());
        let entries = self.decorate_batch_entries(entries);
        spinner.finish();
        entries
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| self.format_commit_info(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        run_cli_action(
            &action,
            arguments,
            include_str!("../plugin.toml"),
            |arguments| ACTION_REGISTRY.read().handle(&action, arguments),
        )
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        lla_plugin_utils::manifest_action_infos(include_str!("../plugin.toml"))
    }
}

fn decorate_commit_entry(entry: &mut proto::DecoratedEntry, info: &CommitInfo) {
    entry.insert_field("commit_hash", value::string(&info.hash), info.hash.clone());
    entry.insert_field(
        "commit_author",
        value::string(&info.author),
        info.author.clone(),
    );
    let timestamp = info.time.parse::<u64>().unwrap_or_default();
    entry.insert_field(
        "commit_time",
        value::timestamp(timestamp),
        info.time.clone(),
    );
}

fn batched_git_log(root: &Path, requested: &[String]) -> HashMap<String, CommitInfo> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args([
            "log",
            "--format=commit%x09%h%x09%an%x09%at",
            "--name-only",
            "--no-renames",
            "--",
        ])
        .args(
            requested
                .iter()
                .map(|path| if path.is_empty() { "." } else { path }),
        );
    let Ok(output) = command.output() else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }
    let mut found = HashMap::new();
    let mut current = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(metadata) = line.strip_prefix("commit\t") {
            let mut fields = metadata.splitn(3, '\t');
            current = match (fields.next(), fields.next(), fields.next()) {
                (Some(hash), Some(author), Some(time)) => Some(CommitInfo {
                    hash: hash.to_string(),
                    author: author.to_string(),
                    time: time.to_string(),
                }),
                _ => None,
            };
            continue;
        }
        let changed = line.trim();
        if changed.is_empty() {
            continue;
        }
        let Some(info) = current.as_ref() else {
            continue;
        };
        for requested_path in requested {
            if found.contains_key(requested_path) {
                continue;
            }
            let prefix = format!("{requested_path}/");
            if requested_path.is_empty()
                || changed == requested_path
                || changed.starts_with(&prefix)
            {
                found.insert(requested_path.clone(), info.clone());
            }
        }
    }
    found
}

fn git_head_fingerprint(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let source = std::fs::read_to_string(dot_git).ok()?;
        let relative = source.trim().strip_prefix("gitdir: ")?;
        let candidate = Path::new(relative);
        if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            root.join(candidate)
        }
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let target = head
        .trim()
        .strip_prefix("ref: ")
        .and_then(|reference| std::fs::read_to_string(git_dir.join(reference)).ok())
        .unwrap_or_default();
    let packed =
        lla_plugin_utils::file_fingerprint(&git_dir.join("packed-refs")).unwrap_or_default();
    Some(format!("{}:{}:{packed}", head.trim(), target.trim()))
}

impl Default for LastGitCommitPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for LastGitCommitPlugin {
    type Config = CommitConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

lla_plugin_sdk::export_plugin!(LastGitCommitPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_commit_timestamps_keep_a_relative_human_display() {
        let two_minutes_ago = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(120);
        assert_eq!(
            LastGitCommitPlugin::format_relative_time(&two_minutes_ago.to_string()),
            "2 minutes ago"
        );
        assert_eq!(
            LastGitCommitPlugin::format_relative_time("legacy"),
            "legacy"
        );
    }

    #[test]
    fn one_history_walk_resolves_each_requested_path() {
        let root = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        };
        if !run(&["init", "--quiet"]) {
            return;
        }
        run(&["config", "user.email", "tests@example.com"]);
        run(&["config", "user.name", "lla tests"]);
        std::fs::write(root.path().join("first.txt"), "first").unwrap();
        run(&["add", "first.txt"]);
        run(&["commit", "--quiet", "-m", "first"]);
        std::fs::write(root.path().join("second.txt"), "second").unwrap();
        run(&["add", "second.txt"]);
        run(&["commit", "--quiet", "-m", "second"]);

        let found = batched_git_log(
            root.path(),
            &["first.txt".to_string(), "second.txt".to_string()],
        );
        assert_eq!(found.len(), 2);
        assert_ne!(found["first.txt"].hash, found["second.txt"].hash);
    }
}
