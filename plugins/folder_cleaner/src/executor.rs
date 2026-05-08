use crate::{
    model::{CleanupPlan, ManifestAction, OperationKind, PlanAction, RunManifest},
    paths,
    planner::{make_id, unique_target},
};
use chrono::Utc;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

pub fn save_plan(plan: &CleanupPlan) -> Result<PathBuf, String> {
    validate_plan_shape(&plan.actions)?;
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

pub fn selected_actions<'a>(
    plan: &'a CleanupPlan,
    selected_ids: Option<&HashSet<usize>>,
) -> Vec<&'a PlanAction> {
    plan.actions
        .iter()
        .filter(|action| {
            selected_ids
                .map(|ids| ids.contains(&action.id))
                .unwrap_or(true)
        })
        .collect()
}

pub fn preflight_plan(
    plan: &CleanupPlan,
    selected_ids: Option<&HashSet<usize>>,
) -> Result<(), Vec<String>> {
    let actions = selected_actions(plan, selected_ids);
    let mut errors = validate_plan_shape_for_refs(&actions);

    for action in actions {
        if !action.source.exists() {
            errors.push(format!(
                "Source no longer exists: {}",
                action.source.display()
            ));
        }
        if action.target.exists() {
            errors.push(format!(
                "Target already exists: {}",
                action.target.display()
            ));
        }
        if action.source == action.target {
            errors.push(format!(
                "Source and target are identical: {}",
                action.source.display()
            ));
        }
        if action.target.starts_with(&action.source) {
            errors.push(format!(
                "Target is inside source, which could recursively move data: {} -> {}",
                action.source.display(),
                action.target.display()
            ));
        }

        if let Some(parent) = action.target.parent() {
            match nearest_existing_parent(parent) {
                Some(existing_parent) => {
                    if fs::metadata(&existing_parent)
                        .map(|metadata| metadata.permissions().readonly())
                        .unwrap_or(false)
                    {
                        errors.push(format!(
                            "Nearest target parent is read-only: {}",
                            existing_parent.display()
                        ));
                    }
                }
                None => {
                    errors.push(format!(
                        "No existing parent found for target: {}",
                        action.target.display()
                    ));
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub(crate) fn execute_plan_inner(
    plan: &CleanupPlan,
    selected_ids: Option<&HashSet<usize>>,
    persist: bool,
) -> Result<RunManifest, String> {
    if let Err(errors) = preflight_plan(plan, selected_ids) {
        return Err(format!("Preflight failed:\n  - {}", errors.join("\n  - ")));
    }

    let run_id = make_id("run");
    let actions = selected_actions(plan, selected_ids);
    let mut manifest = RunManifest {
        id: run_id.clone(),
        plan_id: plan.id.clone(),
        created_at: Utc::now().to_rfc3339(),
        root: plan.root.clone(),
        actions: actions
            .iter()
            .map(|action| ManifestAction {
                operation: action.kind.clone(),
                source: action.source.clone(),
                target: action.target.clone(),
                reason: action.reason.clone(),
                hash: action.hash.clone(),
                completed: false,
                restored: false,
            })
            .collect(),
    };

    if persist {
        save_run(&manifest)?;
    }

    for (idx, action) in actions.iter().enumerate() {
        execute_action(action)?;
        if let Some(manifest_action) = manifest.actions.get_mut(idx) {
            manifest_action.completed = true;
        }
        if persist {
            save_run(&manifest)?;
        }
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

pub fn list_plans() -> Result<Vec<CleanupPlan>, String> {
    let dir = paths::plans_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("Failed to list plans: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to list plans: {}", e))?;
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if let Ok(content) = fs::read_to_string(entry.path()) {
            if let Ok(plan) = serde_json::from_str::<CleanupPlan>(&content) {
                plans.push(plan);
            }
        }
    }
    plans.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(plans)
}

pub fn restore_run(run_id: &str) -> Result<(RunManifest, usize), String> {
    let mut manifest = load_run(run_id)?;
    let mut restored = 0usize;
    let mut reserved = HashSet::new();

    for action in manifest.actions.iter_mut().rev() {
        if !action.completed || action.restored || !action.target.exists() {
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
                .filter(|action| action.completed)
                .filter(|action| !action.restored)
                .filter(|action| action.target.exists()),
        );
    }
    Ok(items)
}

#[derive(Debug, Clone, Default)]
pub struct DoctorReport {
    pub run_id: String,
    pub total_actions: usize,
    pub completed_actions: usize,
    pub restored_actions: usize,
    pub restorable_actions: usize,
    pub pending_actions: Vec<ManifestAction>,
    pub missing_targets: Vec<ManifestAction>,
}

impl DoctorReport {
    pub fn is_healthy(&self) -> bool {
        self.pending_actions.is_empty() && self.missing_targets.is_empty()
    }
}

pub fn inspect_run(run: &RunManifest) -> DoctorReport {
    let mut report = DoctorReport {
        run_id: run.id.clone(),
        total_actions: run.actions.len(),
        completed_actions: run.actions.iter().filter(|action| action.completed).count(),
        restored_actions: run.actions.iter().filter(|action| action.restored).count(),
        restorable_actions: run
            .actions
            .iter()
            .filter(|action| action.completed && !action.restored && action.target.exists())
            .count(),
        pending_actions: Vec::new(),
        missing_targets: Vec::new(),
    };

    for action in &run.actions {
        if !action.completed {
            report.pending_actions.push(action.clone());
        } else if !action.restored && !action.target.exists() {
            report.missing_targets.push(action.clone());
        }
    }

    report
}

pub fn inspect_runs(run_id: Option<&str>) -> Result<Vec<DoctorReport>, String> {
    let runs = match run_id {
        Some(run_id) => vec![load_run(run_id)?],
        None => list_runs()?,
    };
    Ok(runs.iter().map(inspect_run).collect())
}

pub fn orphaned_quarantine_items() -> Result<Vec<PathBuf>, String> {
    let mut known_targets = HashSet::new();
    for run in list_runs()? {
        for action in run.actions {
            if action.operation == OperationKind::Quarantine {
                known_targets.insert(action.target);
            }
        }
    }

    let mut orphans = Vec::new();
    let roots = quarantine_roots()?;
    for root in roots {
        collect_orphaned_files(&root, &known_targets, &mut orphans)?;
    }
    orphans.sort();
    Ok(orphans)
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

fn validate_plan_shape(actions: &[PlanAction]) -> Result<(), String> {
    let refs = actions.iter().collect::<Vec<_>>();
    let errors = validate_plan_shape_for_refs(&refs);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("Invalid plan:\n  - {}", errors.join("\n  - ")))
    }
}

fn validate_plan_shape_for_refs(actions: &[&PlanAction]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut sources: HashMap<&Path, usize> = HashMap::new();
    let mut targets: HashMap<&Path, usize> = HashMap::new();

    for action in actions {
        *sources.entry(action.source.as_path()).or_insert(0) += 1;
        *targets.entry(action.target.as_path()).or_insert(0) += 1;
    }

    for (source, count) in sources {
        if count > 1 {
            errors.push(format!(
                "Source appears in multiple actions: {}",
                source.display()
            ));
        }
    }

    for (target, count) in targets {
        if count > 1 {
            errors.push(format!(
                "Target appears in multiple actions: {}",
                target.display()
            ));
        }
    }

    let source_set = actions
        .iter()
        .map(|action| action.source.as_path())
        .collect::<HashSet<_>>();
    for action in actions {
        if source_set.contains(action.target.as_path()) {
            errors.push(format!(
                "Target is also another action source: {}",
                action.target.display()
            ));
        }
    }

    errors
}

fn nearest_existing_parent(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn quarantine_roots() -> Result<Vec<PathBuf>, String> {
    let mut roots = HashSet::new();
    for run in list_runs()? {
        for action in run.actions {
            if action.operation == OperationKind::Quarantine {
                if let Some(root) = quarantine_root_for(&action.target) {
                    roots.insert(root);
                }
            }
        }
    }
    Ok(roots.into_iter().collect())
}

fn quarantine_root_for(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if ancestor.file_name().and_then(|name| name.to_str()) == Some(".lla-quarantine") {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

fn collect_orphaned_files(
    root: &Path,
    known_targets: &HashSet<PathBuf>,
    orphans: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|e| format!("Failed to inspect quarantine: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to inspect quarantine: {}", e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_orphaned_files(&path, known_targets, orphans)?;
        } else if !known_targets.contains(&path) {
            orphans.push(path);
        }
    }
    Ok(())
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
                completed: true,
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
        assert!(manifest.actions.first().unwrap().completed);

        let mut restored_manifest = manifest.clone();
        let action = restored_manifest.actions.first_mut().unwrap();
        std::fs::rename(&action.target, &action.source).unwrap();
        action.restored = true;
        assert!(file.exists());
    }

    #[test]
    fn preflight_rejects_missing_sources_without_moving_files() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing.tmp");
        let plan = CleanupPlan {
            id: "plan-test".to_string(),
            created_at: "now".to_string(),
            root: temp.path().to_path_buf(),
            profile: "downloads".to_string(),
            actions: vec![PlanAction {
                id: 1,
                kind: OperationKind::Quarantine,
                source: missing,
                target: temp.path().join(".lla-quarantine").join("missing.tmp"),
                reason: "temporary".to_string(),
                category: None,
                hash: None,
            }],
        };

        assert!(preflight_plan(&plan, None).is_err());
        assert!(!temp.path().join(".lla-quarantine").exists());
    }

    #[test]
    fn preflight_rejects_conflicting_source_chains() {
        let temp = tempfile::tempdir().unwrap();
        let file_a = temp.path().join("a.txt");
        let file_b = temp.path().join("b.txt");
        std::fs::write(&file_a, b"a").unwrap();
        std::fs::write(&file_b, b"b").unwrap();

        let plan = CleanupPlan {
            id: "plan-test".to_string(),
            created_at: "now".to_string(),
            root: temp.path().to_path_buf(),
            profile: "downloads".to_string(),
            actions: vec![
                PlanAction {
                    id: 1,
                    kind: OperationKind::Organize,
                    source: file_a.clone(),
                    target: file_b.clone(),
                    reason: "organize".to_string(),
                    category: Some("documents".to_string()),
                    hash: None,
                },
                PlanAction {
                    id: 2,
                    kind: OperationKind::Organize,
                    source: file_b,
                    target: temp.path().join("Documents").join("b.txt"),
                    reason: "organize".to_string(),
                    category: Some("documents".to_string()),
                    hash: None,
                },
            ],
        };

        assert!(preflight_plan(&plan, None).is_err());
        assert!(file_a.exists());
    }

    #[test]
    fn doctor_detects_partial_and_missing_targets() {
        let temp = tempfile::tempdir().unwrap();
        let run = RunManifest {
            id: "run-test".to_string(),
            plan_id: "plan-test".to_string(),
            created_at: "now".to_string(),
            root: temp.path().to_path_buf(),
            actions: vec![
                ManifestAction {
                    operation: OperationKind::Organize,
                    source: temp.path().join("a.txt"),
                    target: temp.path().join("Documents").join("a.txt"),
                    reason: "organize".to_string(),
                    hash: None,
                    completed: false,
                    restored: false,
                },
                ManifestAction {
                    operation: OperationKind::Quarantine,
                    source: temp.path().join("b.tmp"),
                    target: temp.path().join(".lla-quarantine").join("b.tmp"),
                    reason: "temporary".to_string(),
                    hash: None,
                    completed: true,
                    restored: false,
                },
            ],
        };

        let report = inspect_run(&run);
        assert_eq!(report.pending_actions.len(), 1);
        assert_eq!(report.missing_targets.len(), 1);
        assert!(!report.is_healthy());
    }
}
