use crate::{
    model::{CleanupPlan, ManifestAction, OperationKind, PlanAction, RunManifest},
    paths,
    planner::{make_id, unique_target},
};
use chrono::Utc;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

pub fn save_plan(plan: &CleanupPlan) -> Result<PathBuf, String> {
    fs::create_dir_all(paths::plans_dir())
        .map_err(|e| format!("Failed to create plans directory: {}", e))?;
    let path = paths::plan_path(&plan.id);
    let content = serde_json::to_string_pretty(plan)
        .map_err(|e| format!("Failed to serialize plan: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("Failed to write plan: {}", e))?;
    Ok(path)
}

pub fn load_plan(plan_id: &str) -> Result<CleanupPlan, String> {
    let path = paths::plan_path(plan_id);
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read plan '{}': {}", plan_id, e))?;
    serde_json::from_str(&content).map_err(|e| format!("Failed to parse plan '{}': {}", plan_id, e))
}

pub fn execute_plan(
    plan: &CleanupPlan,
    selected_ids: Option<&HashSet<usize>>,
) -> Result<RunManifest, String> {
    execute_plan_inner(plan, selected_ids, true)
}

fn execute_plan_inner(
    plan: &CleanupPlan,
    selected_ids: Option<&HashSet<usize>>,
    persist: bool,
) -> Result<RunManifest, String> {
    let run_id = make_id("run");
    let mut manifest = RunManifest {
        id: run_id.clone(),
        plan_id: plan.id.clone(),
        created_at: Utc::now().to_rfc3339(),
        root: plan.root.clone(),
        actions: Vec::new(),
    };

    for action in &plan.actions {
        if selected_ids
            .map(|ids| !ids.contains(&action.id))
            .unwrap_or(false)
        {
            continue;
        }

        execute_action(action)?;
        manifest.actions.push(ManifestAction {
            operation: action.kind.clone(),
            source: action.source.clone(),
            target: action.target.clone(),
            reason: action.reason.clone(),
            hash: action.hash.clone(),
            restored: false,
        });
    }

    if persist {
        save_run(&manifest)?;
    }
    Ok(manifest)
}

fn execute_action(action: &PlanAction) -> Result<(), String> {
    if !action.source.exists() {
        return Err(format!(
            "Source no longer exists: {}",
            action.source.display()
        ));
    }

    if let Some(parent) = action.target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create '{}': {}", parent.display(), e))?;
    }

    fs::rename(&action.source, &action.target).map_err(|e| {
        format!(
            "Failed to move '{}' to '{}': {}",
            action.source.display(),
            action.target.display(),
            e
        )
    })
}

pub fn save_run(manifest: &RunManifest) -> Result<PathBuf, String> {
    fs::create_dir_all(paths::runs_dir())
        .map_err(|e| format!("Failed to create runs directory: {}", e))?;
    let path = paths::run_path(&manifest.id);
    let content = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Failed to serialize run manifest: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("Failed to write run manifest: {}", e))?;
    Ok(path)
}

pub fn load_run(run_id: &str) -> Result<RunManifest, String> {
    let path = paths::run_path(run_id);
    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read run '{}': {}", run_id, e))?;
    serde_json::from_str(&content).map_err(|e| format!("Failed to parse run '{}': {}", run_id, e))
}

pub fn restore_run(run_id: &str) -> Result<(RunManifest, usize), String> {
    let mut manifest = load_run(run_id)?;
    let mut restored = 0usize;
    let mut reserved = HashSet::new();

    for action in manifest.actions.iter_mut().rev() {
        if action.restored || !action.target.exists() {
            continue;
        }

        let restore_target = if action.source.exists() {
            unique_target(&action.source, &mut reserved)
        } else {
            action.source.clone()
        };

        if let Some(parent) = restore_target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create '{}': {}", parent.display(), e))?;
        }

        fs::rename(&action.target, &restore_target).map_err(|e| {
            format!(
                "Failed to restore '{}' to '{}': {}",
                action.target.display(),
                restore_target.display(),
                e
            )
        })?;
        action.restored = true;
        restored += 1;
    }

    save_run(&manifest)?;
    Ok((manifest, restored))
}

pub fn list_runs() -> Result<Vec<RunManifest>, String> {
    let dir = paths::runs_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut runs = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("Failed to list runs: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to list runs: {}", e))?;
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if let Ok(content) = fs::read_to_string(entry.path()) {
            if let Ok(run) = serde_json::from_str::<RunManifest>(&content) {
                runs.push(run);
            }
        }
    }
    runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(runs)
}

pub fn quarantine_items() -> Result<Vec<ManifestAction>, String> {
    let mut items = Vec::new();
    for run in list_runs()? {
        items.extend(
            run.actions
                .into_iter()
                .filter(|action| action.operation == OperationKind::Quarantine)
                .filter(|action| !action.restored)
                .filter(|action| action.target.exists()),
        );
    }
    Ok(items)
}

pub fn empty_old_quarantine(days: u64) -> Result<usize, String> {
    let cutoff_secs = days * 24 * 60 * 60;
    let now = std::time::SystemTime::now();
    let mut removed = 0usize;

    for item in quarantine_items()? {
        let metadata = match fs::metadata(&item.target) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let age = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);

        if age < cutoff_secs {
            continue;
        }

        remove_path(&item.target)?;
        removed += 1;
    }

    Ok(removed)
}

fn remove_path(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|e| format!("Failed to remove '{}': {}", path.display(), e))
    } else {
        fs::remove_file(path).map_err(|e| format!("Failed to remove '{}': {}", path.display(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OperationKind;

    #[test]
    fn manifest_round_trip_parses_restore_state() {
        let manifest = RunManifest {
            id: "run-test".to_string(),
            plan_id: "plan-test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            root: PathBuf::from("/tmp/root"),
            actions: vec![ManifestAction {
                operation: OperationKind::Quarantine,
                source: PathBuf::from("/tmp/root/a.tmp"),
                target: PathBuf::from("/tmp/root/.lla-quarantine/a.tmp"),
                reason: "temporary".to_string(),
                hash: None,
                restored: false,
            }],
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: RunManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.actions.len(), 1);
        assert!(!parsed.actions[0].restored);
    }

    #[test]
    fn preview_only_plan_does_not_move_files() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("loose.txt");
        std::fs::write(&file, b"hello").unwrap();
        let plan = CleanupPlan {
            id: "plan-test".to_string(),
            created_at: "now".to_string(),
            root: temp.path().to_path_buf(),
            profile: "downloads".to_string(),
            actions: vec![PlanAction {
                id: 1,
                kind: OperationKind::Organize,
                source: file.clone(),
                target: temp.path().join("Documents").join("loose.txt"),
                reason: "organize".to_string(),
                category: Some("documents".to_string()),
                hash: None,
            }],
        };

        assert!(file.exists());
        assert_eq!(plan.actions.len(), 1);
        assert!(!plan.actions[0].target.exists());
    }

    #[test]
    fn execute_and_restore_moves_files_back() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("old.tmp");
        let quarantine = temp.path().join(".lla-quarantine").join("old.tmp");
        std::fs::write(&file, b"temp").unwrap();

        let plan = CleanupPlan {
            id: "plan-test".to_string(),
            created_at: "now".to_string(),
            root: temp.path().to_path_buf(),
            profile: "downloads".to_string(),
            actions: vec![PlanAction {
                id: 1,
                kind: OperationKind::Quarantine,
                source: file.clone(),
                target: quarantine.clone(),
                reason: "temporary".to_string(),
                category: None,
                hash: None,
            }],
        };

        let manifest = execute_plan_inner(&plan, None, false).unwrap();
        assert!(!file.exists());
        assert!(quarantine.exists());

        let mut restored_manifest = manifest.clone();
        let action = restored_manifest.actions.first_mut().unwrap();
        std::fs::rename(&action.target, &action.source).unwrap();
        action.restored = true;
        assert!(file.exists());
    }
}
