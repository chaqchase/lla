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
use std::{collections::HashMap, path::Path, process::Command, time::Duration};

lazy_static! {
    static ref SPINNER: RwLock<Spinner> = RwLock::new(Spinner::new());
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            ["lla plugin run git_status help"],
            |_| {
                let mut help = HelpFormatter::new("Git Status Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Shows Git repository status information for files and directories."
                        .to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string()).add_command(
                    "help".to_string(),
                    "Show this help information".to_string(),
                    vec!["lla plugin run git_status help".to_string()],
                );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic Git status information".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show detailed Git status information including branch and commit details"
                            .to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&GitConfig::default().colors))
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
pub struct GitConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("clean".to_string(), "bright_green".to_string());
    colors.insert("modified".to_string(), "bright_yellow".to_string());
    colors.insert("staged".to_string(), "bright_green".to_string());
    colors.insert("untracked".to_string(), "bright_blue".to_string());
    colors.insert("conflict".to_string(), "bright_red".to_string());
    colors.insert("branch".to_string(), "bright_cyan".to_string());
    colors.insert("commit".to_string(), "bright_yellow".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for GitConfig {}

pub struct GitStatusPlugin {
    base: BasePlugin<GitConfig>,
    repo_cache: lla_plugin_utils::PersistentCache<RepoStaticInfo>,
    status_cache: lla_plugin_utils::PersistentCache<Vec<(String, String)>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RepoStaticInfo {
    branch: String,
    commit: String,
}

impl GitStatusPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
            repo_cache: lla_plugin_utils::PersistentCache::for_plugin(
                plugin_name,
                "repository-cache.toml",
                1,
                2_000,
            ),
            status_cache: lla_plugin_utils::PersistentCache::for_plugin(
                plugin_name,
                "status-cache.toml",
                2,
                2_000,
            ),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[GitStatusPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn git_root(path: &Path) -> Option<std::path::PathBuf> {
        let start = if path.is_dir() { path } else { path.parent()? };
        let mut current_dir = Some(start);
        while let Some(dir) = current_dir {
            if dir.join(".git").exists() {
                return Some(dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()));
            }
            current_dir = dir.parent();
        }
        None
    }

    fn static_info(&mut self, root: &Path) -> Option<RepoStaticInfo> {
        let key = lla_plugin_utils::canonical_cache_key(root);
        let fingerprint = git_head_fingerprint(root)?;
        if let Some(info) = self.repo_cache.get(&key, &fingerprint) {
            return Some(info);
        }
        let branch_output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(root)
            .output()
            .ok()?;
        let commit_output = Command::new("git")
            .args(["log", "-1", "--format=%h %s"])
            .current_dir(root)
            .output()
            .ok()?;
        let branch = String::from_utf8(branch_output.stdout)
            .ok()?
            .trim()
            .to_string();
        let commit = String::from_utf8(commit_output.stdout)
            .ok()?
            .trim()
            .to_string();
        let info = RepoStaticInfo { branch, commit };
        self.repo_cache.insert(key, fingerprint, info.clone());
        Some(info)
    }

    fn repository_statuses(&mut self, root: &Path) -> Option<Vec<(String, String)>> {
        let key = lla_plugin_utils::canonical_cache_key(root);
        let fingerprint = git_worktree_fingerprint(root)?;
        if let Some(statuses) =
            self.status_cache
                .get_fresh_matching(&key, Some(&fingerprint), Duration::from_secs(2))
        {
            return Some(statuses);
        }
        let statuses = read_repository_statuses(root)?;
        self.status_cache.insert(key, fingerprint, statuses.clone());
        Some(statuses)
    }

    fn decorate_batch_entries(
        &mut self,
        mut entries: Vec<proto::DecoratedEntry>,
    ) -> Vec<proto::DecoratedEntry> {
        let mut repositories = std::collections::BTreeMap::<std::path::PathBuf, Vec<usize>>::new();
        for (index, entry) in entries.iter().enumerate() {
            if let Some(root) = Self::git_root(Path::new(&entry.path)) {
                repositories.entry(root).or_default().push(index);
            }
        }

        for (root, indices) in repositories {
            let Some(static_info) = self.static_info(&root) else {
                continue;
            };
            let Some(statuses) = self.repository_statuses(&root) else {
                continue;
            };
            for index in indices {
                let path = Path::new(&entries[index].path);
                let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                let relative = absolute
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let prefix = format!("{relative}/");
                let relevant = statuses
                    .iter()
                    .filter(|(_, status_path)| {
                        relative.is_empty()
                            || status_path == &relative
                            || status_path.starts_with(&prefix)
                    })
                    .map(|(status, path)| format!("{status} {path}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                decorate_git_entry(
                    &mut entries[index],
                    &relevant,
                    &static_info.branch,
                    &static_info.commit,
                );
            }
        }
        let _ = self.repo_cache.persist();
        let _ = self.status_cache.persist();
        entries
    }

    fn format_git_status(status: &str) -> (String, usize, usize, usize, usize) {
        let mut staged = 0;
        let mut modified = 0;
        let mut untracked = 0;
        let mut conflicts = 0;

        let mut formatted_entries = Vec::new();

        for line in status.lines() {
            let status_chars: Vec<char> = line.chars().take(2).collect();
            let index_status = status_chars.first().copied().unwrap_or(' ');
            let worktree_status = status_chars.get(1).copied().unwrap_or(' ');

            match (index_status, worktree_status) {
                ('M', ' ') => {
                    staged += 1;
                    formatted_entries.push("staged");
                }
                (' ', 'M') => {
                    modified += 1;
                    formatted_entries.push("modified");
                }
                ('M', 'M') => {
                    staged += 1;
                    modified += 1;
                    formatted_entries.push("staged & modified");
                }
                ('A', ' ') => {
                    staged += 1;
                    formatted_entries.push("new file");
                }
                ('D', ' ') | (' ', 'D') => {
                    modified += 1;
                    formatted_entries.push("deleted");
                }
                ('R', _) => {
                    staged += 1;
                    formatted_entries.push("renamed");
                }
                ('C', _) => {
                    staged += 1;
                    formatted_entries.push("copied");
                }
                ('U', _) | (_, 'U') => {
                    conflicts += 1;
                    formatted_entries.push("conflict");
                }
                ('?', '?') => {
                    untracked += 1;
                    formatted_entries.push("untracked");
                }
                _ => {}
            }
        }

        let status_summary = if formatted_entries.is_empty() {
            "clean".to_string()
        } else {
            formatted_entries.join(", ")
        };

        (status_summary, staged, modified, untracked, conflicts)
    }

    fn format_git_info(
        &self,
        entry: &lla_plugin_utils::DecoratedEntry,
        format: &str,
    ) -> Option<String> {
        let colors = &self.base.config().colors;
        let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

        if let (Some(status), Some(branch), Some(commit)) = (
            entry.custom_fields.get("git_status"),
            entry.custom_fields.get("git_branch"),
            entry.custom_fields.get("git_commit"),
        ) {
            match format {
                "long" => {
                    let key_color = colors
                        .get("info")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let value_color = colors
                        .get("branch")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let kv = KeyValue::new("Branch", branch)
                        .key_color(&key_color)
                        .value_color(&value_color)
                        .key_width(12);
                    list.add_item(kv.render());

                    let commit_parts: Vec<&str> = commit.split_whitespace().collect();
                    if let Some((hash, msg)) = commit_parts.split_first() {
                        let key_color = colors
                            .get("info")
                            .unwrap_or(&"white".to_string())
                            .to_string();
                        let value_color = colors
                            .get("commit")
                            .unwrap_or(&"white".to_string())
                            .to_string();
                        let kv = KeyValue::new("Commit", format!("{} {}", hash, msg.join(" ")))
                            .key_color(&key_color)
                            .value_color(&value_color)
                            .key_width(12);
                        list.add_item(kv.render());
                    }

                    let mut status_items = Vec::new();
                    if let Some(staged) = entry.custom_fields.get("git_staged") {
                        if let Ok(count) = staged.parse::<usize>() {
                            if count > 0 {
                                status_items.push(format!("{} staged", count));
                            }
                        }
                    }
                    if let Some(modified) = entry.custom_fields.get("git_modified") {
                        if let Ok(count) = modified.parse::<usize>() {
                            if count > 0 {
                                status_items.push(format!("{} modified", count));
                            }
                        }
                    }
                    if let Some(untracked) = entry.custom_fields.get("git_untracked") {
                        if let Ok(count) = untracked.parse::<usize>() {
                            if count > 0 {
                                status_items.push(format!("{} untracked", count));
                            }
                        }
                    }
                    if let Some(conflicts) = entry.custom_fields.get("git_conflicts") {
                        if let Ok(count) = conflicts.parse::<usize>() {
                            if count > 0 {
                                status_items.push(format!("{} conflicts", count));
                            }
                        }
                    }

                    let status_text = if status_items.is_empty() {
                        "working tree clean".to_string()
                    } else {
                        status_items.join(", ")
                    };

                    let key_color = colors
                        .get("info")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let value_color = if status_items.is_empty() {
                        colors
                            .get("clean")
                            .unwrap_or(&"white".to_string())
                            .to_string()
                    } else {
                        colors
                            .get("modified")
                            .unwrap_or(&"white".to_string())
                            .to_string()
                    };
                    let kv = KeyValue::new("Status", status_text)
                        .key_color(&key_color)
                        .value_color(&value_color)
                        .key_width(12);
                    list.add_item(kv.render());
                }
                "default" => {
                    let key_color = colors
                        .get("info")
                        .unwrap_or(&"white".to_string())
                        .to_string();
                    let value_color = if status == "clean" {
                        colors
                            .get("clean")
                            .unwrap_or(&"white".to_string())
                            .to_string()
                    } else {
                        colors
                            .get("modified")
                            .unwrap_or(&"white".to_string())
                            .to_string()
                    };
                    let kv = KeyValue::new("Git", status)
                        .key_color(&key_color)
                        .value_color(&value_color)
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

impl Plugin for GitStatusPlugin {
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
        spinner.set_status("Checking Git status...".to_string());
        let entries = self.decorate_batch_entries(entries);
        spinner.finish();
        entries
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| self.format_git_info(&entry, &format))
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

fn decorate_git_entry(entry: &mut proto::DecoratedEntry, status: &str, branch: &str, commit: &str) {
    let (summary, staged, modified, untracked, conflicts) =
        GitStatusPlugin::format_git_status(status);
    entry.insert_field("git_status", value::string(&summary), summary);
    entry.insert_field("git_branch", value::string(branch), branch);
    entry
        .custom_fields
        .insert("git_commit".to_string(), commit.to_string());
    entry.insert_field(
        "git_staged",
        value::integer(staged as i64),
        staged.to_string(),
    );
    entry.insert_field(
        "git_modified",
        value::integer(modified as i64),
        modified.to_string(),
    );
    entry
        .custom_fields
        .insert("git_untracked".to_string(), untracked.to_string());
    entry
        .custom_fields
        .insert("git_conflicts".to_string(), conflicts.to_string());
}

fn read_repository_statuses(root: &Path) -> Option<Vec<(String, String)>> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| parse_status_z(&output.stdout))
}

fn git_worktree_fingerprint(root: &Path) -> Option<String> {
    let git_dir = git_directory(root)?;
    let head = git_head_fingerprint(root)?;
    let index = lla_plugin_utils::file_fingerprint(&git_dir.join("index")).unwrap_or_default();
    let root_metadata = lla_plugin_utils::file_fingerprint(root).unwrap_or_default();
    Some(format!("{head}:{index}:{root_metadata}"))
}

fn parse_status_z(bytes: &[u8]) -> Vec<(String, String)> {
    let mut records = Vec::new();
    let mut fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    while let Some(field) = fields.next() {
        if field.len() < 4 {
            continue;
        }
        let status = String::from_utf8_lossy(&field[..2]).into_owned();
        let path = String::from_utf8_lossy(&field[3..]).into_owned();
        if matches!(field[0], b'R' | b'C') {
            let _ = fields.next();
        }
        records.push((status, path));
    }
    records
}

fn git_head_fingerprint(root: &Path) -> Option<String> {
    let git_dir = git_directory(root)?;
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

fn git_directory(root: &Path) -> Option<std::path::PathBuf> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let source = std::fs::read_to_string(dot_git).ok()?;
    let relative = source.trim().strip_prefix("gitdir: ")?;
    let candidate = Path::new(relative);
    Some(if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    })
}

impl Default for GitStatusPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for GitStatusPlugin {
    type Config = GitConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

lla_plugin_sdk::export_plugin!(GitStatusPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_zero_delimited_statuses_and_rename_pairs() {
        let statuses = parse_status_z(b" M src/lib.rs\0?? new.txt\0R  new.rs\0old.rs\0");
        assert_eq!(
            statuses,
            vec![
                (" M".to_string(), "src/lib.rs".to_string()),
                ("??".to_string(), "new.txt".to_string()),
                ("R ".to_string(), "new.rs".to_string()),
            ]
        );
    }

    #[test]
    fn status_summary_counts_untracked_entries() {
        let (summary, _, _, untracked, _) = GitStatusPlugin::format_git_status("?? new.txt");
        assert_eq!(summary, "untracked");
        assert_eq!(untracked, 1);
    }
}
