use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, ActionArguments, DecoratedEntryExt, Plugin};
use lla_plugin_utils::{
    decode_decorated_entry, map_decorated_entry, run_cli_action, ActionRegistry, DecoratedEntry,
};
use parking_lot::RwLock;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

lazy_static! {
    static ref CACHE: RwLock<HashMap<PathBuf, ProjectInfo>> = RwLock::new(HashMap::new());
    static ref ACTIONS: RwLock<ActionRegistry> = RwLock::new({
        let mut actions = ActionRegistry::new();
        lla_plugin_utils::define_action!(
            actions,
            "inspect",
            "inspect <path>",
            "Inspect the nearest project and print ecosystem and repository health",
            ["lla plugin run project_context inspect -- ."],
            ProjectContextPlugin::inspect_action
        );
        lla_plugin_utils::define_action!(
            actions,
            "refresh",
            "refresh",
            "Clear cached project detection results",
            ["lla plugin run project_context refresh"],
            |_| {
                CACHE.write().clear();
                println!("Project context cache cleared.");
                Ok(())
            }
        );
        lla_plugin_utils::define_action!(
            actions,
            "help",
            "help",
            "Show project context usage",
            ["lla plugin run project_context help"],
            |_| ProjectContextPlugin::help_action()
        );
        actions
    });
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ProjectInfo {
    project_types: Vec<String>,
    root: PathBuf,
    health: String,
    issues: usize,
    lockfile_state: String,
    build_artifacts: Vec<String>,
    toolchains: Vec<String>,
    repository_state: String,
    git_branch: Option<String>,
}

pub struct ProjectContextPlugin;

impl ProjectContextPlugin {
    fn find_root(path: &Path) -> Option<PathBuf> {
        let start = if path.is_dir() { path } else { path.parent()? };
        start
            .ancestors()
            .take(12)
            .find(|candidate| has_project_marker(candidate))
            .map(Path::to_path_buf)
    }

    fn analyze(root: &Path) -> ProjectInfo {
        if let Some(info) = CACHE.read().get(root).cloned() {
            return info;
        }
        let mut project_types = Vec::new();
        let mut expected_locks = Vec::<(&str, Vec<&str>)>::new();
        if root.join("Cargo.toml").is_file() {
            project_types.push("rust".to_string());
            expected_locks.push(("rust", vec!["Cargo.lock"]));
        }
        if root.join("package.json").is_file() {
            project_types.push("node".to_string());
            expected_locks.push((
                "node",
                vec![
                    "package-lock.json",
                    "yarn.lock",
                    "pnpm-lock.yaml",
                    "bun.lockb",
                    "bun.lock",
                ],
            ));
        }
        if ["pyproject.toml", "requirements.txt", "setup.py", "Pipfile"]
            .iter()
            .any(|marker| root.join(marker).is_file())
        {
            project_types.push("python".to_string());
            expected_locks.push((
                "python",
                vec!["uv.lock", "poetry.lock", "Pipfile.lock", "requirements.txt"],
            ));
        }
        if root.join("go.mod").is_file() {
            project_types.push("go".to_string());
            expected_locks.push(("go", vec!["go.sum"]));
        }

        let mut lock_states = Vec::new();
        let mut issues = 0usize;
        for (ecosystem, locks) in expected_locks {
            let present = locks
                .iter()
                .filter(|lock| root.join(lock).is_file())
                .copied()
                .collect::<Vec<_>>();
            match present.len() {
                0 => {
                    issues += 1;
                    lock_states.push(format!("{ecosystem}:missing"));
                }
                1 => lock_states.push(format!("{ecosystem}:{}", present[0])),
                _ => {
                    issues += 1;
                    lock_states.push(format!("{ecosystem}:multiple({})", present.join(",")));
                }
            }
        }

        let build_artifacts = [
            "target",
            "node_modules",
            ".venv",
            "venv",
            "dist",
            "build",
            "__pycache__",
        ]
        .iter()
        .filter(|artifact| root.join(artifact).exists())
        .map(|artifact| (*artifact).to_string())
        .collect::<Vec<_>>();

        let mut toolchains = Vec::new();
        for project_type in &project_types {
            let version = match project_type.as_str() {
                "rust" => command_line("rustc", &["--version"]),
                "node" => {
                    command_line("node", &["--version"]).map(|version| format!("node {version}"))
                }
                "python" => command_line("python3", &["--version"]),
                "go" => command_line("go", &["version"]),
                _ => None,
            };
            toolchains.push(version.unwrap_or_else(|| format!("{project_type}:not-found")));
        }

        let (repository_state, git_branch, dirty_count) = repository_health(root);
        issues = issues.saturating_add(dirty_count);
        let health = if project_types.is_empty() {
            "not-a-project"
        } else if repository_state == "dirty" {
            "dirty"
        } else if issues > 0 {
            "warning"
        } else {
            "healthy"
        }
        .to_string();
        let info = ProjectInfo {
            project_types,
            root: root.to_path_buf(),
            health,
            issues,
            lockfile_state: if lock_states.is_empty() {
                "none".to_string()
            } else {
                lock_states.join("; ")
            },
            build_artifacts,
            toolchains,
            repository_state,
            git_branch,
        };
        CACHE.write().insert(root.to_path_buf(), info.clone());
        info
    }

    fn info_for(path: &Path) -> ProjectInfo {
        Self::find_root(path)
            .map(|root| Self::analyze(&root))
            .unwrap_or_else(|| ProjectInfo {
                root: if path.is_dir() {
                    path.to_path_buf()
                } else {
                    path.parent()
                        .unwrap_or_else(|| Path::new("."))
                        .to_path_buf()
                },
                health: "not-a-project".to_string(),
                lockfile_state: "none".to_string(),
                repository_state: "none".to_string(),
                ..ProjectInfo::default()
            })
    }

    fn decorate(mut entry: DecoratedEntry) -> DecoratedEntry {
        let info = Self::info_for(&entry.path);
        entry.custom_fields.insert(
            "project_type".to_string(),
            if info.project_types.is_empty() {
                "none".to_string()
            } else {
                info.project_types.join("+")
            },
        );
        entry.custom_fields.insert(
            "project_root".to_string(),
            info.root.to_string_lossy().to_string(),
        );
        entry
            .custom_fields
            .insert("project_health".to_string(), info.health);
        entry
            .custom_fields
            .insert("project_issues".to_string(), info.issues.to_string());
        entry
            .custom_fields
            .insert("lockfile_state".to_string(), info.lockfile_state);
        entry.custom_fields.insert(
            "build_artifacts".to_string(),
            if info.build_artifacts.is_empty() {
                "none".to_string()
            } else {
                info.build_artifacts.join(", ")
            },
        );
        entry.custom_fields.insert(
            "toolchains".to_string(),
            if info.toolchains.is_empty() {
                "none".to_string()
            } else {
                info.toolchains.join("; ")
            },
        );
        entry
            .custom_fields
            .insert("repository_state".to_string(), info.repository_state);
        if let Some(branch) = info.git_branch {
            entry.custom_fields.insert("git_branch".to_string(), branch);
        }
        entry
    }

    fn format(entry: &DecoratedEntry, format: &str) -> Option<String> {
        let kind = entry.custom_fields.get("project_type")?;
        let health = entry.custom_fields.get("project_health")?;
        match format {
            "default" => Some(format!("[project: {kind}; {health}]")),
            "long" => Some(format!(
                "Project: {kind}\nRoot: {}\nHealth: {health} ({} issues)\nLocks: {}\nArtifacts: {}\nToolchains: {}\nRepository: {}{}",
                entry.custom_fields.get("project_root")?,
                entry.custom_fields.get("project_issues")?,
                entry.custom_fields.get("lockfile_state")?,
                entry.custom_fields.get("build_artifacts")?,
                entry.custom_fields.get("toolchains")?,
                entry.custom_fields.get("repository_state")?,
                entry
                    .custom_fields
                    .get("git_branch")
                    .map(|branch| format!(" ({branch})"))
                    .unwrap_or_default()
            )),
            _ => None,
        }
    }

    fn inspect_action(args: &[String]) -> Result<(), String> {
        let path = args
            .first()
            .map(Path::new)
            .unwrap_or_else(|| Path::new("."));
        if !path.exists() {
            return Err(format!("Path does not exist: {}", path.display()));
        }
        CACHE.write().clear();
        let info = Self::info_for(path);
        println!("Root: {}", info.root.display());
        println!(
            "Project types: {}",
            if info.project_types.is_empty() {
                "none".to_string()
            } else {
                info.project_types.join(", ")
            }
        );
        println!("Health: {} ({} issues)", info.health, info.issues);
        println!("Locks: {}", info.lockfile_state);
        println!(
            "Artifacts: {}",
            if info.build_artifacts.is_empty() {
                "none".to_string()
            } else {
                info.build_artifacts.join(", ")
            }
        );
        println!(
            "Toolchains: {}",
            if info.toolchains.is_empty() {
                "none".to_string()
            } else {
                info.toolchains.join("; ")
            }
        );
        println!("Repository: {}", info.repository_state);
        if let Some(branch) = info.git_branch {
            println!("Branch: {branch}");
        }
        Ok(())
    }

    fn help_action() -> Result<(), String> {
        println!(
            "project_context\n\n  inspect [path]  Detect Rust, Node, Python, and Go context\n  refresh         Clear cached project state\n  help            Show this help\n\nThe decorator exposes typed project health, lockfile, artifact, toolchain, and Git fields."
        );
        Ok(())
    }
}

fn has_project_marker(path: &Path) -> bool {
    [
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "requirements.txt",
        "setup.py",
        "Pipfile",
        "go.mod",
    ]
    .iter()
    .any(|marker| path.join(marker).is_file())
}

fn command_line(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Some(if stdout.is_empty() { stderr } else { stdout })
}

fn repository_health(root: &Path) -> (String, Option<String>, usize) {
    let Some(output) = command_line(
        "git",
        &[
            "-C",
            &root.to_string_lossy(),
            "status",
            "--porcelain=v1",
            "--branch",
        ],
    ) else {
        return ("not-a-repository".to_string(), None, 0);
    };
    let mut lines = output.lines();
    let branch = lines.next().and_then(|line| {
        line.strip_prefix("## ")
            .and_then(|branch| branch.split("...").next())
            .map(str::to_string)
    });
    let dirty = lines.filter(|line| !line.trim().is_empty()).count();
    (
        if dirty == 0 { "clean" } else { "dirty" }.to_string(),
        branch,
        dirty,
    )
}

impl Plugin for ProjectContextPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        promote_v3_fields(map_decorated_entry(entry, Self::decorate))
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        entries
            .into_iter()
            .map(|entry| promote_v3_fields(map_decorated_entry(entry, Self::decorate)))
            .collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| Self::format(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        run_cli_action(
            &action,
            arguments,
            include_str!("../plugin.toml"),
            |arguments| ACTIONS.read().handle(&action, arguments),
        )
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        lla_plugin_utils::manifest_action_infos(include_str!("../plugin.toml"))
    }
}

fn promote_v3_fields(mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
    for name in [
        "project_type",
        "project_health",
        "lockfile_state",
        "build_artifacts",
        "toolchains",
        "repository_state",
        "git_branch",
    ] {
        entry.promote_string_field(name);
    }
    entry.promote_path_field("project_root");
    entry.promote_integer_field("project_issues");
    entry
}

lla_plugin_sdk::export_plugin!(ProjectContextPlugin);

impl Default for ProjectContextPlugin {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_multi_ecosystem_projects_locks_and_artifacts() {
        CACHE.write().clear();
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Cargo.toml"), "[package]\nname='demo'").unwrap();
        fs::write(root.path().join("Cargo.lock"), "").unwrap();
        fs::write(root.path().join("package.json"), "{}").unwrap();
        fs::create_dir(root.path().join("target")).unwrap();

        let info = ProjectContextPlugin::analyze(root.path());
        assert_eq!(info.project_types, vec!["rust", "node"]);
        assert!(info.lockfile_state.contains("rust:Cargo.lock"));
        assert!(info.lockfile_state.contains("node:missing"));
        assert_eq!(info.build_artifacts, vec!["target"]);
        assert!(info.issues >= 1);
    }

    #[test]
    fn nearest_parent_project_is_used_for_files() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("go.mod"), "module example.test/demo").unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        let file = root.path().join("src/main.go");
        fs::write(&file, "package main").unwrap();
        assert_eq!(
            ProjectContextPlugin::find_root(&file),
            Some(root.path().to_path_buf())
        );
    }

    #[test]
    fn detects_python_and_go_projects_with_locks() {
        CACHE.write().clear();
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("pyproject.toml"), "[project]\nname='demo'").unwrap();
        fs::write(root.path().join("uv.lock"), "").unwrap();
        fs::write(root.path().join("go.mod"), "module example.test/demo").unwrap();
        fs::write(root.path().join("go.sum"), "").unwrap();

        let info = ProjectContextPlugin::analyze(root.path());
        assert_eq!(info.project_types, vec!["python", "go"]);
        assert!(info.lockfile_state.contains("python:uv.lock"));
        assert!(info.lockfile_state.contains("go:go.sum"));
    }

    #[test]
    fn repository_health_reports_dirty_worktrees() {
        let root = tempfile::tempdir().unwrap();
        let initialized = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root.path())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !initialized {
            return;
        }
        fs::write(root.path().join("untracked.txt"), "dirty").unwrap();

        let (state, _branch, dirty) = repository_health(root.path());
        assert_eq!(state, "dirty");
        assert_eq!(dirty, 1);
    }
}
