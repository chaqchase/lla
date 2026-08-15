use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrashRecord {
    pub id: String,
    pub original_path: PathBuf,
    pub stored_path: PathBuf,
    pub deleted_at: String,
    pub deleted_at_epoch: i64,
    pub size: u64,
    pub is_dir: bool,
}

#[derive(Clone, Debug)]
pub struct TrashStore {
    root: PathBuf,
}

impl TrashStore {
    pub fn for_plugin_data() -> Self {
        let data_root = std::env::var_os("LLA_PLUGIN_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".config")
                    .join("lla")
                    .join("plugins")
            });
        Self::new(data_root.join("trash"))
    }

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put(&self, path: &Path) -> Result<TrashRecord, String> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("Cannot trash '{}': {error}", path.display()))?;
        let original_path = absolute_path(path)?;
        if original_path == self.root || self.root.starts_with(&original_path) {
            return Err("Refusing to trash the trash store or one of its ancestors".to_string());
        }
        let file_name = path
            .file_name()
            .ok_or_else(|| format!("Cannot trash '{}': path has no file name", path.display()))?
            .to_string_lossy();
        let id = make_id();
        let stored_path = self.items_dir().join(format!("{id}--{file_name}"));
        fs::create_dir_all(self.items_dir())
            .map_err(|error| format!("Failed to create trash storage: {error}"))?;
        fs::create_dir_all(self.records_dir())
            .map_err(|error| format!("Failed to create trash metadata storage: {error}"))?;
        let deleted_at = Utc::now();
        let record = TrashRecord {
            id: id.clone(),
            original_path,
            stored_path: stored_path.clone(),
            deleted_at: deleted_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            deleted_at_epoch: deleted_at.timestamp(),
            size: path_size(path).unwrap_or(metadata.len()),
            is_dir: metadata.is_dir(),
        };
        self.write_record(&record)?;
        if let Err(error) = move_path(path, &stored_path) {
            let _ = fs::remove_file(self.record_path(&record.id));
            return Err(error);
        }
        Ok(record)
    }

    pub fn list(&self) -> Result<Vec<TrashRecord>, String> {
        if !self.records_dir().is_dir() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(self.records_dir())
            .map_err(|error| format!("Failed to list trash metadata: {error}"))?
        {
            let entry = entry.map_err(|error| format!("Failed to list trash metadata: {error}"))?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let source = fs::read_to_string(entry.path()).map_err(|error| {
                format!(
                    "Failed to read trash record '{}': {error}",
                    entry.path().display()
                )
            })?;
            let record = serde_json::from_str::<TrashRecord>(&source).map_err(|error| {
                format!(
                    "Failed to parse trash record '{}': {error}",
                    entry.path().display()
                )
            })?;
            records.push(record);
        }
        records.sort_by(|left, right| right.deleted_at_epoch.cmp(&left.deleted_at_epoch));
        Ok(records)
    }

    pub fn restore(&self, id: &str) -> Result<PathBuf, String> {
        validate_id(id)?;
        let record = self.read_record(id)?;
        if !path_exists(&record.stored_path) {
            return Err(format!("Trashed content for '{id}' is missing"));
        }
        let destination = unique_restore_path(&record.original_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to recreate '{}': {error}", parent.display()))?;
        }
        move_path(&record.stored_path, &destination)?;
        fs::remove_file(self.record_path(id))
            .map_err(|error| format!("Restored data but failed to remove its record: {error}"))?;
        Ok(destination)
    }

    pub fn empty_older_than(&self, days: u64) -> Result<usize, String> {
        let cutoff = Utc::now().timestamp() - (days as i64).saturating_mul(86_400);
        let mut removed = 0;
        for record in self.list()? {
            if record.deleted_at_epoch > cutoff {
                continue;
            }
            if path_exists(&record.stored_path) {
                remove_path(&record.stored_path)?;
            }
            fs::remove_file(self.record_path(&record.id)).map_err(|error| {
                format!("Failed to remove trash record '{}': {error}", record.id)
            })?;
            removed += 1;
        }
        Ok(removed)
    }

    fn items_dir(&self) -> PathBuf {
        self.root.join("items")
    }

    fn records_dir(&self) -> PathBuf {
        self.root.join("records")
    }

    fn record_path(&self, id: &str) -> PathBuf {
        self.records_dir().join(format!("{id}.json"))
    }

    fn write_record(&self, record: &TrashRecord) -> Result<(), String> {
        let source = serde_json::to_string_pretty(record)
            .map_err(|error| format!("Failed to serialize trash record: {error}"))?;
        fs::write(self.record_path(&record.id), source)
            .map_err(|error| format!("Failed to save trash record: {error}"))
    }

    fn read_record(&self, id: &str) -> Result<TrashRecord, String> {
        let source = fs::read_to_string(self.record_path(id))
            .map_err(|error| format!("Trash record '{id}' was not found: {error}"))?;
        serde_json::from_str(&source)
            .map_err(|error| format!("Trash record '{id}' is invalid: {error}"))
    }
}

pub fn remove_path(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Failed to inspect '{}': {error}", path.display()))?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|error| format!("Failed to permanently remove '{}': {error}", path.display()))
}

fn move_path(source: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create '{}': {error}", parent.display()))?;
    }
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            if let Err(copy_error) = copy_path(source, destination) {
                if path_exists(destination) {
                    let _ = remove_path(destination);
                }
                return Err(format!(
                    "Failed to move '{}' to trash ({rename_error}); copy fallback failed: {copy_error}",
                    source.display()
                ));
            }
            if let Err(remove_error) = remove_path(source) {
                let _ = remove_path(destination);
                return Err(format!(
                    "Copied '{}' to trash but could not remove the original: {remove_error}",
                    source.display()
                ));
            }
            Ok(())
        }
    }
}

fn copy_path(source: &Path, destination: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        let target_is_dir = fs::metadata(source)
            .map(|target| target.is_dir())
            .unwrap_or(false);
        return copy_symlink(source, destination, target_is_dir);
    }
    if metadata.is_file() {
        fs::copy(source, destination)?;
        fs::set_permissions(destination, metadata.permissions())?;
        return Ok(());
    }
    fs::create_dir(destination)?;
    fs::set_permissions(destination, metadata.permissions())?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        copy_path(&entry.path(), &destination.join(entry.file_name()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path, _is_dir: bool) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, destination)
}

#[cfg(windows)]
fn copy_symlink(source: &Path, destination: &Path, is_dir: bool) -> io::Result<()> {
    let target = fs::read_link(source)?;
    if is_dir {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
}

fn path_size(path: &Path) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(metadata.len());
    }
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        total = total.saturating_add(path_size(&entry?.path())?);
    }
    Ok(total)
}

fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .map_err(|error| format!("Failed to resolve '{}': {error}", path.display()))
    }
}

fn unique_restore_path(original: &Path) -> PathBuf {
    if !path_exists(original) {
        return original.to_path_buf();
    }
    let parent = original.parent().unwrap_or_else(|| Path::new("."));
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("restored");
    let extension = original.extension().and_then(|value| value.to_str());
    for suffix in 1.. {
        let name = match extension {
            Some(extension) => format!("{stem} (restored {suffix}).{extension}"),
            None => format!("{stem} (restored {suffix})"),
        };
        let candidate = parent.join(name);
        if !path_exists(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn make_id() -> String {
    format!(
        "{}-{}-{}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '.'))
    {
        return Err("Invalid trash record id".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trashes_lists_and_restores_a_file_without_overwriting() {
        let root = tempfile::tempdir().unwrap();
        let store = TrashStore::new(root.path().join("store"));
        let source = root.path().join("note.txt");
        fs::write(&source, "original").unwrap();

        let record = store.put(&source).unwrap();
        assert!(!source.exists());
        assert_eq!(store.list().unwrap(), vec![record.clone()]);

        fs::write(&source, "replacement").unwrap();
        let restored = store.restore(&record.id).unwrap();
        assert_ne!(restored, source);
        assert_eq!(fs::read_to_string(restored).unwrap(), "original");
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn empty_is_age_gated() {
        let root = tempfile::tempdir().unwrap();
        let store = TrashStore::new(root.path().join("store"));
        let source = root.path().join("old.txt");
        fs::write(&source, "old").unwrap();
        let mut record = store.put(&source).unwrap();
        record.deleted_at_epoch = 0;
        store.write_record(&record).unwrap();

        assert_eq!(store.empty_older_than(1).unwrap(), 1);
        assert!(!record.stored_path.exists());
    }

    #[test]
    fn directory_trees_round_trip() {
        let root = tempfile::tempdir().unwrap();
        let store = TrashStore::new(root.path().join("store"));
        let source = root.path().join("project");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("README.md"), "restorable").unwrap();

        let record = store.put(&source).unwrap();
        assert!(record.is_dir);
        let restored = store.restore(&record.id).unwrap();
        assert_eq!(
            fs::read_to_string(restored.join("README.md")).unwrap(),
            "restorable"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_moved_without_touching_their_targets() {
        let root = tempfile::tempdir().unwrap();
        let store = TrashStore::new(root.path().join("store"));
        let target = root.path().join("target.txt");
        let link = root.path().join("link.txt");
        fs::write(&target, "target").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let record = store.put(&link).unwrap();
        assert!(target.is_file());
        assert!(fs::symlink_metadata(&record.stored_path)
            .unwrap()
            .file_type()
            .is_symlink());
        let restored = store.restore(&record.id).unwrap();
        assert!(fs::symlink_metadata(restored)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(target.is_file());
    }
}
