use crate::{
    config::{FolderCleanerConfig, ProfileConfig},
    model::{ScanOptions, ScanReport, ScannedEntry},
};
use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use walkdir::{DirEntry, WalkDir};

pub fn options_from_config(config: &FolderCleanerConfig, profile: &ProfileConfig) -> ScanOptions {
    ScanOptions {
        recursive: profile.recursive.unwrap_or(config.scan.recursive),
        max_depth: profile.max_depth.unwrap_or(config.scan.max_depth),
        include_hidden: profile.include_hidden.unwrap_or(config.scan.include_hidden),
        follow_symlinks: config.scan.follow_symlinks,
        same_filesystem: config.scan.same_filesystem,
    }
}

pub fn scan_directory(
    root: &Path,
    config: &FolderCleanerConfig,
    options: &ScanOptions,
) -> Result<ScanReport, String> {
    if !root.exists() {
        return Err(format!("Directory does not exist: {}", root.display()));
    }
    if !root.is_dir() {
        return Err(format!("Path is not a directory: {}", root.display()));
    }

    let root = fs::canonicalize(root)
        .map_err(|e| format!("Failed to resolve '{}': {}", root.display(), e))?;
    let root_device = device_id(&root);
    let max_depth = if options.recursive {
        options.max_depth
    } else {
        1
    };

    let ignored = Cell::new(0usize);
    let walker = WalkDir::new(&root)
        .follow_links(options.follow_symlinks)
        .max_depth(max_depth)
        .into_iter()
        .filter_entry(|entry| {
            let keep = should_descend(entry, &root, config, options, root_device);
            if !keep {
                ignored.set(ignored.get() + 1);
            }
            keep
        });

    let mut entries = Vec::new();
    for item in walker {
        let item = item.map_err(|e| format!("Failed to scan directory: {}", e))?;
        if item.path() == root {
            continue;
        }

        let metadata = match item.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                ignored.set(ignored.get() + 1);
                continue;
            }
        };

        entries.push(ScannedEntry {
            path: item.path().to_path_buf(),
            is_dir: metadata.is_dir(),
            is_file: metadata.is_file(),
            size: metadata.len(),
            modified_secs: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
            created_secs: metadata
                .created()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
        });
    }

    Ok(ScanReport {
        root,
        entries,
        ignored: ignored.get(),
    })
}

fn should_descend(
    entry: &DirEntry,
    root: &Path,
    config: &FolderCleanerConfig,
    options: &ScanOptions,
    root_device: Option<u64>,
) -> bool {
    if entry.path() == root {
        return true;
    }

    if !options.include_hidden && is_hidden(entry.path()) {
        return false;
    }

    if matches_ignore(entry.path(), root, &config.scan.ignore_patterns) {
        return false;
    }

    if options.same_filesystem && entry.file_type().is_dir() {
        if let (Some(root_device), Some(entry_device)) = (root_device, device_id(entry.path())) {
            if root_device != entry_device {
                return false;
            }
        }
    }

    true
}

pub fn matches_ignore(path: &Path, root: &Path, patterns: &[String]) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let relative_str = relative.to_string_lossy();
    patterns.iter().any(|pattern| {
        relative
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == pattern.as_str())
            || relative_str.contains(pattern)
    })
}

pub fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.') && name != "." && name != "..")
        .unwrap_or(false)
}

#[cfg(unix)]
fn device_id(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|metadata| metadata.dev())
}

#[cfg(not(unix))]
fn device_id(_path: &Path) -> Option<u64> {
    None
}

pub fn relative_to(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_files_are_detected() {
        assert!(is_hidden(Path::new(".env")));
        assert!(!is_hidden(Path::new("notes.txt")));
    }

    #[test]
    fn ignore_rules_match_components() {
        let root = Path::new("/tmp/work");
        let patterns = vec!["node_modules".to_string(), ".git".to_string()];
        assert!(matches_ignore(
            Path::new("/tmp/work/app/node_modules/react"),
            root,
            &patterns
        ));
        assert!(!matches_ignore(
            Path::new("/tmp/work/app/src/main.rs"),
            root,
            &patterns
        ));
    }
}
