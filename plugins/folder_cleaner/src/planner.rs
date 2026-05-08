use crate::{
    config::{FolderCleanerConfig, ProfileConfig, RuleConfig},
    model::{CleanupPlan, OperationKind, PlanAction, ScanReport, ScannedEntry},
    scanner::relative_to,
};
use chrono::Utc;
use glob::Pattern;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn build_plan(
    report: &ScanReport,
    config: &FolderCleanerConfig,
    profile_name: &str,
    profile: &ProfileConfig,
) -> CleanupPlan {
    let id = make_id("plan");
    let mut actions = Vec::new();
    let mut reserved_targets = HashSet::new();
    let destination_dirs = destination_dirs(report, config);

    if profile.organize {
        plan_organization(
            report,
            config,
            &destination_dirs,
            &mut actions,
            &mut reserved_targets,
        );
    }

    if profile.cleanup {
        plan_cleanup(
            report,
            config,
            &id,
            &destination_dirs,
            &mut actions,
            &mut reserved_targets,
        );
    }

    for (index, action) in actions.iter_mut().enumerate() {
        action.id = index + 1;
    }

    CleanupPlan {
        id,
        created_at: Utc::now().to_rfc3339(),
        root: report.root.clone(),
        profile: profile_name.to_string(),
        actions,
    }
}

fn plan_organization(
    report: &ScanReport,
    config: &FolderCleanerConfig,
    destination_dirs: &HashSet<PathBuf>,
    actions: &mut Vec<PlanAction>,
    reserved_targets: &mut HashSet<PathBuf>,
) {
    for entry in report.entries.iter().filter(|entry| entry.is_file) {
        if is_inside_quarantine(&entry.path, &report.root, config) {
            continue;
        }
        if is_inside_destination_dir(&entry.path, destination_dirs) {
            continue;
        }

        let (category, destination) = match category_for(entry, &report.root, &config.rules) {
            Some(value) => value,
            None => ("uncategorized".to_string(), "Uncategorized".to_string()),
        };

        let Some(file_name) = entry.path.file_name() else {
            continue;
        };
        let target = unique_target(
            &report.root.join(destination).join(file_name),
            reserved_targets,
        );

        if target == entry.path {
            continue;
        }

        actions.push(PlanAction {
            id: actions.len() + 1,
            kind: OperationKind::Organize,
            source: entry.path.clone(),
            target,
            reason: format!("organize as {}", category),
            category: Some(category),
            hash: None,
        });
    }
}

fn plan_cleanup(
    report: &ScanReport,
    config: &FolderCleanerConfig,
    plan_id: &str,
    destination_dirs: &HashSet<PathBuf>,
    actions: &mut Vec<PlanAction>,
    reserved_targets: &mut HashSet<PathBuf>,
) {
    let mut cleanup_sources = HashSet::new();
    let moving_sources = actions
        .iter()
        .map(|action| action.source.clone())
        .collect::<HashSet<_>>();
    let protected_dirs = protected_dirs(report, destination_dirs, actions);

    if config.cleanup.temp_files || config.cleanup.os_junk || config.cleanup.old_archives {
        for entry in report.entries.iter().filter(|entry| entry.is_file) {
            if is_inside_quarantine(&entry.path, &report.root, config) {
                continue;
            }

            let reason = cleanup_reason(entry, report, config);
            if let Some(reason) = reason {
                add_quarantine_action(
                    report,
                    config,
                    plan_id,
                    entry,
                    reason,
                    None,
                    actions,
                    reserved_targets,
                    &mut cleanup_sources,
                );
            }
        }
    }

    if config.cleanup.duplicate_detection {
        for (hash, duplicates) in duplicate_groups(report, config) {
            let mut sorted = duplicates;
            sorted.sort_by_key(|entry| {
                if entry.modified_secs == 0 {
                    entry.created_secs
                } else {
                    entry.modified_secs
                }
            });

            for entry in sorted.into_iter().skip(1) {
                add_quarantine_action(
                    report,
                    config,
                    plan_id,
                    entry,
                    "duplicate file; oldest copy kept".to_string(),
                    Some(hash.clone()),
                    actions,
                    reserved_targets,
                    &mut cleanup_sources,
                );
            }
        }
    }

    if config.cleanup.empty_dirs {
        let file_parent_dirs = report
            .entries
            .iter()
            .filter(|entry| entry.is_file)
            .filter_map(|entry| entry.path.parent().map(Path::to_path_buf))
            .collect::<HashSet<_>>();

        let mut dirs = report
            .entries
            .iter()
            .filter(|entry| entry.is_dir)
            .collect::<Vec<_>>();
        dirs.sort_by_key(|entry| std::cmp::Reverse(entry.path.components().count()));

        for entry in dirs {
            if is_inside_quarantine(&entry.path, &report.root, config) {
                continue;
            }
            if protected_dirs.contains(&entry.path) {
                continue;
            }
            if contains_planned_move(&entry.path, &moving_sources) {
                continue;
            }
            if file_parent_dirs.contains(&entry.path) {
                continue;
            }
            if has_child_dir_with_files(&entry.path, &file_parent_dirs) {
                continue;
            }

            add_quarantine_action(
                report,
                config,
                plan_id,
                entry,
                "empty directory".to_string(),
                None,
                actions,
                reserved_targets,
                &mut cleanup_sources,
            );
        }
    }
}

fn add_quarantine_action(
    report: &ScanReport,
    config: &FolderCleanerConfig,
    plan_id: &str,
    entry: &ScannedEntry,
    reason: String,
    hash: Option<String>,
    actions: &mut Vec<PlanAction>,
    reserved_targets: &mut HashSet<PathBuf>,
    cleanup_sources: &mut HashSet<PathBuf>,
) {
    if !cleanup_sources.insert(entry.path.clone()) {
        return;
    }

    actions.retain(|action| action.source != entry.path);
    let relative = relative_to(&report.root, &entry.path);
    let target = unique_target(
        &report
            .root
            .join(&config.safety.quarantine_dir)
            .join(plan_id)
            .join(relative),
        reserved_targets,
    );

    actions.push(PlanAction {
        id: actions.len() + 1,
        kind: OperationKind::Quarantine,
        source: entry.path.clone(),
        target,
        reason,
        category: None,
        hash,
    });
}

fn destination_dirs(report: &ScanReport, config: &FolderCleanerConfig) -> HashSet<PathBuf> {
    let mut dirs = config
        .rules
        .iter()
        .map(|rule| report.root.join(&rule.destination))
        .collect::<HashSet<_>>();
    dirs.insert(report.root.join("Uncategorized"));
    dirs.insert(report.root.join(&config.safety.quarantine_dir));
    dirs
}

fn is_inside_destination_dir(path: &Path, destination_dirs: &HashSet<PathBuf>) -> bool {
    destination_dirs.iter().any(|dir| path.starts_with(dir))
}

fn protected_dirs(
    report: &ScanReport,
    destination_dirs: &HashSet<PathBuf>,
    actions: &[PlanAction],
) -> HashSet<PathBuf> {
    let mut protected = destination_dirs.clone();

    for action in actions {
        if let Some(parent) = action.source.parent() {
            protect_dir_and_ancestors(parent, &report.root, &mut protected);
        }
        if let Some(parent) = action.target.parent() {
            protect_dir_and_ancestors(parent, &report.root, &mut protected);
        }
    }

    protected
}

fn protect_dir_and_ancestors(dir: &Path, root: &Path, protected: &mut HashSet<PathBuf>) {
    let mut current = Some(dir);
    while let Some(path) = current {
        if !path.starts_with(root) {
            break;
        }
        protected.insert(path.to_path_buf());
        if path == root {
            break;
        }
        current = path.parent();
    }
}

pub fn category_for(
    entry: &ScannedEntry,
    root: &Path,
    rules: &[RuleConfig],
) -> Option<(String, String)> {
    let extension = entry
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.trim_start_matches('.').to_lowercase());
    let file_name = entry.path.file_name()?.to_string_lossy();
    let relative = relative_to(root, &entry.path);
    let relative_str = relative.to_string_lossy();

    for rule in rules {
        let ext_match = extension
            .as_ref()
            .map(|extension| {
                rule.extensions.iter().any(|rule_ext| {
                    rule_ext
                        .trim_start_matches('.')
                        .eq_ignore_ascii_case(extension)
                })
            })
            .unwrap_or(false);

        let file_match = rule
            .filename_patterns
            .iter()
            .any(|pattern| glob_match(pattern, &file_name));
        let path_match = rule
            .path_patterns
            .iter()
            .any(|pattern| glob_match(pattern, &relative_str));

        if ext_match || file_match || path_match {
            return Some((rule.category.clone(), rule.destination.clone()));
        }
    }

    None
}

pub fn unique_target(target: &Path, reserved_targets: &mut HashSet<PathBuf>) -> PathBuf {
    if !target.exists() && !reserved_targets.contains(target) {
        reserved_targets.insert(target.to_path_buf());
        return target.to_path_buf();
    }

    let parent = target.parent().unwrap_or_else(|| Path::new(""));
    let stem = target
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("item");
    let extension = target.extension().and_then(|ext| ext.to_str());

    for index in 1.. {
        let file_name = match extension {
            Some(extension) => format!("{} ({}).{}", stem, index, extension),
            None => format!("{} ({})", stem, index),
        };
        let candidate = parent.join(file_name);
        if !candidate.exists() && !reserved_targets.contains(&candidate) {
            reserved_targets.insert(candidate.clone());
            return candidate;
        }
    }

    unreachable!("unbounded collision loop should always return")
}

fn cleanup_reason(
    entry: &ScannedEntry,
    report: &ScanReport,
    config: &FolderCleanerConfig,
) -> Option<String> {
    let name = entry.path.file_name()?.to_string_lossy().to_lowercase();
    let extension = entry
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
        .unwrap_or_default();

    if config.cleanup.os_junk && matches!(name.as_str(), ".ds_store" | "thumbs.db" | "desktop.ini")
    {
        return Some("os metadata junk".to_string());
    }

    if config.cleanup.temp_files
        && (name.ends_with('~')
            || name.ends_with(".tmp")
            || name.ends_with(".temp")
            || name.ends_with(".bak")
            || name.ends_with(".swp")
            || name.ends_with(".download")
            || name.ends_with(".part")
            || name.ends_with(".crdownload")
            || name.starts_with("~$"))
    {
        return Some("temporary or backup file".to_string());
    }

    if config.cleanup.old_archives && is_archive_extension(&extension) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let age_secs = now.saturating_sub(entry.modified_secs);
        if age_secs >= config.cleanup.old_archive_days * 24 * 60 * 60 {
            return Some(format!(
                "archive older than {} days",
                config.cleanup.old_archive_days
            ));
        }
    }

    if entry.path.starts_with(report.root.join(".Trash")) {
        return Some("trash folder content".to_string());
    }

    None
}

fn duplicate_groups<'a>(
    report: &'a ScanReport,
    config: &FolderCleanerConfig,
) -> HashMap<String, Vec<&'a ScannedEntry>> {
    let mut by_size: HashMap<u64, Vec<&ScannedEntry>> = HashMap::new();
    for entry in report.entries.iter().filter(|entry| entry.is_file) {
        if entry.size == 0 || entry.size > config.cleanup.duplicate_max_bytes {
            continue;
        }
        by_size.entry(entry.size).or_default().push(entry);
    }

    let mut by_hash: HashMap<String, Vec<&ScannedEntry>> = HashMap::new();
    for entries in by_size.values().filter(|entries| entries.len() > 1) {
        for entry in entries {
            if let Some(hash) = file_hash(&entry.path) {
                by_hash.entry(hash).or_default().push(*entry);
            }
        }
    }

    by_hash
        .into_iter()
        .filter(|(_, entries)| entries.len() > 1)
        .collect()
}

pub fn file_hash(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(bytes) => hasher.update(&buffer[..bytes]),
            Err(_) => return None,
        }
    }

    Some(format!("{:x}", hasher.finalize()))
}

fn glob_match(pattern: &str, value: &str) -> bool {
    Pattern::new(pattern)
        .map(|pattern| pattern.matches(value))
        .unwrap_or_else(|_| value.contains(pattern))
}

fn is_archive_extension(extension: &str) -> bool {
    matches!(
        extension,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz"
    )
}

fn is_inside_quarantine(path: &Path, root: &Path, config: &FolderCleanerConfig) -> bool {
    path.starts_with(root.join(&config.safety.quarantine_dir))
}

fn has_child_dir_with_files(dir: &Path, file_parent_dirs: &HashSet<PathBuf>) -> bool {
    file_parent_dirs
        .iter()
        .any(|parent| parent.starts_with(dir))
}

fn contains_planned_move(dir: &Path, moving_sources: &HashSet<PathBuf>) -> bool {
    moving_sources.iter().any(|source| source.starts_with(dir))
}

pub fn make_id(prefix: &str) -> String {
    format!("{}-{}", prefix, Utc::now().format("%Y%m%d%H%M%S%3f"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FolderCleanerConfig, RuleConfig};

    fn entry(path: &Path) -> ScannedEntry {
        ScannedEntry {
            path: path.to_path_buf(),
            is_dir: false,
            is_file: true,
            size: 1,
            modified_secs: 1,
            created_secs: 1,
        }
    }

    fn dir_entry(path: &Path) -> ScannedEntry {
        ScannedEntry {
            path: path.to_path_buf(),
            is_dir: true,
            is_file: false,
            size: 0,
            modified_secs: 1,
            created_secs: 1,
        }
    }

    #[test]
    fn category_matching_uses_extensions_case_insensitively() {
        let root = Path::new("/tmp/mess");
        let rules = vec![RuleConfig {
            category: "images".to_string(),
            destination: "Images".to_string(),
            extensions: vec!["jpg".to_string()],
            filename_patterns: Vec::new(),
            path_patterns: Vec::new(),
        }];

        assert_eq!(
            category_for(&entry(Path::new("/tmp/mess/Photo.JPG")), root, &rules)
                .map(|value| value.0),
            Some("images".to_string())
        );
    }

    #[test]
    fn collision_targets_are_renamed() {
        let temp = tempfile::tempdir().unwrap();
        let existing = temp.path().join("report.pdf");
        std::fs::write(&existing, b"already here").unwrap();
        let mut reserved = HashSet::new();

        let target = unique_target(&existing, &mut reserved);
        assert_eq!(target.file_name().unwrap(), "report (1).pdf");
    }

    #[test]
    fn duplicate_files_are_quarantined_except_oldest() {
        let temp = tempfile::tempdir().unwrap();
        let older = temp.path().join("a.txt");
        let newer = temp.path().join("b.txt");
        std::fs::write(&older, b"same").unwrap();
        std::fs::write(&newer, b"same").unwrap();

        let config = FolderCleanerConfig::default();
        let report = ScanReport {
            root: temp.path().to_path_buf(),
            ignored: 0,
            entries: vec![
                ScannedEntry {
                    path: older,
                    is_dir: false,
                    is_file: true,
                    size: 4,
                    modified_secs: 1,
                    created_secs: 1,
                },
                ScannedEntry {
                    path: newer.clone(),
                    is_dir: false,
                    is_file: true,
                    size: 4,
                    modified_secs: 2,
                    created_secs: 2,
                },
            ],
        };

        let profile = ProfileConfig {
            organize: false,
            cleanup: true,
            recursive: None,
            max_depth: None,
            include_hidden: None,
        };
        let plan = build_plan(&report, &config, "downloads", &profile);
        assert!(plan
            .actions
            .iter()
            .any(|action| { action.kind == OperationKind::Quarantine && action.source == newer }));
    }

    #[test]
    fn generated_image_destination_is_not_quarantined_as_empty() {
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("photo.avif");
        let images_dir = temp.path().join("Images");
        std::fs::write(&image, b"image").unwrap();
        std::fs::create_dir(&images_dir).unwrap();

        let config = FolderCleanerConfig::default();
        let report = ScanReport {
            root: temp.path().to_path_buf(),
            ignored: 0,
            entries: vec![entry(&image), dir_entry(&images_dir)],
        };
        let profile = ProfileConfig::default();
        let plan = build_plan(&report, &config, "downloads", &profile);

        assert!(plan.actions.iter().any(|action| {
            action.kind == OperationKind::Organize && action.target == images_dir.join("photo.avif")
        }));
        assert!(!plan.actions.iter().any(|action| {
            action.kind == OperationKind::Quarantine && action.source == images_dir
        }));
    }

    #[test]
    fn parent_dirs_with_planned_moves_are_not_quarantined() {
        let temp = tempfile::tempdir().unwrap();
        let loose_dir = temp.path().join("loose");
        let image = loose_dir.join("photo.jpg");
        std::fs::create_dir(&loose_dir).unwrap();
        std::fs::write(&image, b"image").unwrap();

        let config = FolderCleanerConfig::default();
        let report = ScanReport {
            root: temp.path().to_path_buf(),
            ignored: 0,
            entries: vec![dir_entry(&loose_dir), entry(&image)],
        };
        let profile = ProfileConfig::default();
        let plan = build_plan(&report, &config, "downloads", &profile);

        assert!(plan.actions.iter().any(|action| {
            action.kind == OperationKind::Organize
                && action.source == image
                && action.target == temp.path().join("Images").join("photo.jpg")
        }));
        assert!(!plan.actions.iter().any(|action| {
            action.kind == OperationKind::Quarantine && action.source == loose_dir
        }));
    }
}
