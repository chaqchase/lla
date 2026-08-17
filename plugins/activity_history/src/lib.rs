use lla_plugin_sdk::{
    interface::proto, manifest_action_infos, response, value, ActionArguments, ActionArgumentsExt,
    ActionError, DecoratedEntryExt, Plugin,
};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActivityInfo {
    commits: u64,
    timestamp: u64,
    author: String,
    commit: String,
    days_since: u64,
}

#[derive(Default)]
struct ActivityHistoryPlugin;

impl ActivityHistoryPlugin {
    fn inspect_path(path: &Path) -> Option<ActivityInfo> {
        let path = resolve_path(path)?;
        let root = repository_root(&path)?.canonicalize().ok()?;
        let relative = path.strip_prefix(&root).ok()?;
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&root)
            .arg("log")
            .arg("--format=%H%x1f%an%x1f%ct");
        if path.is_file() {
            command.arg("--follow");
        }
        let output = command
            .arg("--")
            .arg(relative)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_activity(&String::from_utf8_lossy(&output.stdout), now_timestamp())
    }

    fn decorate(mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        if let Some(info) = Self::inspect_path(Path::new(&entry.path)) {
            entry.insert_field(
                "activity_commits",
                value::integer(info.commits as i64),
                info.commits.to_string(),
            );
            entry.insert_field(
                "activity_last_commit",
                value::timestamp(info.timestamp),
                info.timestamp.to_string(),
            );
            entry.insert_field("activity_author", value::string(&info.author), info.author);
            entry.insert_field("activity_commit", value::string(&info.commit), info.commit);
            entry.insert_field(
                "activity_days_since",
                value::integer(info.days_since as i64),
                info.days_since.to_string(),
            );
        }
        entry
    }

    fn inspect(path: PathBuf) -> Result<proto::TypedValue, ActionError> {
        if !path.exists() {
            return Err(ActionError::invalid_argument(
                "path",
                format!("path does not exist: {}", path.display()),
            ));
        }
        let info = Self::inspect_path(&path);
        Ok(value::object([
            ("path".to_string(), value::path(path.to_string_lossy())),
            ("tracked".to_string(), value::boolean(info.is_some())),
            (
                "commits".to_string(),
                value::integer(info.as_ref().map_or(0, |info| info.commits) as i64),
            ),
            (
                "last_commit".to_string(),
                info.as_ref()
                    .map_or_else(value::null, |info| value::timestamp(info.timestamp)),
            ),
            (
                "author".to_string(),
                value::string(info.as_ref().map_or("", |info| info.author.as_str())),
            ),
            (
                "commit".to_string(),
                value::string(info.as_ref().map_or("", |info| info.commit.as_str())),
            ),
            (
                "days_since".to_string(),
                value::integer(info.as_ref().map_or(0, |info| info.days_since) as i64),
            ),
        ]))
    }
}

impl Plugin for ActivityHistoryPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        Self::decorate(entry)
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        entries.into_iter().map(Self::decorate).collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        if format != "timeline" {
            return None;
        }
        let commit = entry.custom_fields.get("activity_commit")?;
        let commits = entry.custom_fields.get("activity_commits")?;
        let author = entry.custom_fields.get("activity_author")?;
        let days = entry.custom_fields.get("activity_days_since")?;
        Some(format!(
            "[commit:{} · {} commits · {}d · {}]",
            short_commit(commit),
            commits,
            days,
            author
        ))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        if action != "inspect" {
            return response::error(ActionError::new(
                "unknown-action",
                format!("plugin does not implement action '{action}'"),
            ));
        }
        let result = arguments
            .path("path")
            .and_then(|path| {
                path.ok_or_else(|| ActionError::invalid_argument("path", "path is required"))
            })
            .and_then(Self::inspect);
        match result {
            Ok(value) => response::value(value),
            Err(error) => response::error(error),
        }
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        manifest_action_infos(include_str!("../plugin.toml"))
    }
}

fn resolve_path(path: &Path) -> Option<PathBuf> {
    resolve_path_from(path, &std::env::current_dir().ok()?)
}

fn resolve_path_from(path: &Path, current_dir: &Path) -> Option<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir.join(path)
    };
    path.canonicalize().ok()
}

fn repository_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    let output = Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string()))
}

fn parse_activity(output: &str, now: u64) -> Option<ActivityInfo> {
    let lines = output
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let latest = lines.first()?;
    let mut fields = latest.splitn(3, '\x1f');
    let commit = fields.next()?.to_string();
    let author = fields.next()?.to_string();
    let timestamp = fields.next()?.parse::<u64>().ok()?;
    Some(ActivityInfo {
        commits: lines.len() as u64,
        timestamp,
        author,
        commit,
        days_since: now.saturating_sub(timestamp) / 86_400,
    })
}

fn short_commit(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

lla_plugin_sdk::export_plugin!(ActivityHistoryPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_latest_activity_and_commit_count() {
        let output = "abcdef123456789\x1fAda\x1f172800\n987654321\x1fLin\x1f86400\n";
        let info = parse_activity(output, 345600).unwrap();
        assert_eq!(info.commits, 2);
        assert_eq!(info.author, "Ada");
        assert_eq!(info.days_since, 2);
        assert_eq!(short_commit(&info.commit), "abcdef1");
    }

    #[test]
    fn rejects_empty_git_history() {
        assert!(parse_activity("", 0).is_none());
    }

    #[test]
    fn resolves_relative_paths_before_comparing_with_repository_root() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("tracked.txt");
        std::fs::write(&file, "tracked").unwrap();

        assert_eq!(
            resolve_path_from(Path::new("tracked.txt"), root.path()),
            file.canonicalize().ok()
        );
    }
}
