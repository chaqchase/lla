use crate::error::{LlaError, Result};
use lla_plugin_interface::proto::{DecoratedEntry, EntryMetadata};
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::Path;

fn normalize_internal_path<S: AsRef<str>>(s: S) -> String {
    let mut p = s.as_ref().replace('\\', "/");
    if p.starts_with("./") {
        p = p.trim_start_matches("./").to_string();
    }
    while p.contains("//") {
        p = p.replace("//", "/");
    }
    p.trim_matches('/').to_string()
}

fn synthesize_directory_entries(entries: &mut Vec<DecoratedEntry>, root_prefix: &str) {
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    let root = Path::new(root_prefix);

    for e in entries.iter() {
        let mut current = Path::new(&e.path);
        // Walk up ancestors, collect all parent directories under the same root
        while let Some(parent) = current.parent() {
            if parent == root || parent.as_os_str().is_empty() {
                break;
            }
            dirs.insert(parent.to_string_lossy().to_string());
            current = parent;
        }
    }

    let existing: BTreeSet<String> = entries.iter().map(|e| e.path.clone()).collect();
    for d in dirs.into_iter() {
        if existing.contains(&d) {
            continue;
        }
        entries.push(DecoratedEntry {
            path: d,
            metadata: Some(EntryMetadata {
                size: 0,
                modified: 0,
                accessed: 0,
                created: 0,
                is_dir: true,
                is_file: false,
                is_symlink: false,
                permissions: 0o755,
                uid: 0,
                gid: 0,
            }),
            custom_fields: HashMap::new(),
        });
    }
}

pub fn is_archive_path_str(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".zip")
        || lower.ends_with(".tar")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
}

pub fn read_zip(path: &Path) -> Result<Vec<DecoratedEntry>> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| LlaError::Other(format!("Failed to read zip: {}", e)))?;

    let root_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".to_string());

    let abs_src = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut entries: Vec<DecoratedEntry> = Vec::with_capacity(archive.len() + 8);

    // Synthetic root
    let mut root_fields = HashMap::new();
    root_fields.insert(
        "archive_source".to_string(),
        abs_src.to_string_lossy().into_owned(),
    );
    entries.push(DecoratedEntry {
        path: root_name.clone(),
        metadata: Some(EntryMetadata {
            size: 0,
            modified: 0,
            accessed: 0,
            created: 0,
            is_dir: true,
            is_file: false,
            is_symlink: false,
            permissions: 0o755,
            uid: 0,
            gid: 0,
        }),
        custom_fields: root_fields,
    });

    for i in 0..archive.len() {
        let file = match archive.by_index(i) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Warning: skipping zip entry {}: {}", i, e);
                continue;
            }
        };

        let raw_name = file.name().to_string();
        let name = normalize_internal_path(raw_name);
        if name.is_empty() {
            continue;
        }

        let is_dir = file.is_dir() || name.ends_with('/');
        let is_file = !is_dir;
        let size = if is_dir { 0 } else { file.size() };

        // zip stores msdos datetime; best-effort to convert
        let modified = 0;

        let mode = file
            .unix_mode()
            .unwrap_or_else(|| if is_dir { 0o755 } else { 0o644 });

        let mut custom_fields = HashMap::new();
        custom_fields.insert(
            "archive_source".to_string(),
            abs_src.to_string_lossy().into_owned(),
        );

        // Heuristic for symlink from mode bits (if present)
        let is_symlink = match file.unix_mode() {
            Some(m) => (m & (libc::S_IFMT as u32)) == (libc::S_IFLNK as u32),
            None => false,
        };

        entries.push(DecoratedEntry {
            path: format!("{}/{}", root_name, name),
            metadata: Some(EntryMetadata {
                size,
                modified,
                accessed: 0,
                created: 0,
                is_dir,
                is_file,
                is_symlink,
                permissions: mode,
                uid: 0,
                gid: 0,
            }),
            custom_fields,
        });
    }

    synthesize_directory_entries(&mut entries, &root_name);
    Ok(entries)
}

pub fn read_tar<R: Read>(mut reader: R, source_path: &Path) -> Result<Vec<DecoratedEntry>> {
    let mut archive = tar::Archive::new(&mut reader);

    let root_name = source_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".to_string());
    let abs_src = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());

    let mut entries: Vec<DecoratedEntry> = Vec::with_capacity(64);
    let mut root_fields = HashMap::new();
    root_fields.insert(
        "archive_source".to_string(),
        abs_src.to_string_lossy().into_owned(),
    );
    entries.push(DecoratedEntry {
        path: root_name.clone(),
        metadata: Some(EntryMetadata {
            size: 0,
            modified: 0,
            accessed: 0,
            created: 0,
            is_dir: true,
            is_file: false,
            is_symlink: false,
            permissions: 0o755,
            uid: 0,
            gid: 0,
        }),
        custom_fields: root_fields,
    });

    let entries_iter = match archive.entries() {
        Ok(e) => e,
        Err(e) => {
            return Err(LlaError::Other(format!(
                "Failed to read tar entries: {}",
                e
            )))
        }
    };

    for item in entries_iter {
        let entry = match item {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Warning: skipping tar entry: {}", e);
                continue;
            }
        };

        let path = match entry.path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Warning: tar path error: {}", e);
                continue;
            }
        };
        let name = normalize_internal_path(path.to_string_lossy());
        if name.is_empty() {
            continue;
        }

        let header = entry.header();
        let kind = header.entry_type();
        let is_dir = kind.is_dir();
        let is_symlink = kind.is_symlink();
        let is_file = !(is_dir || is_symlink);

        let size = if is_dir {
            0
        } else {
            header.size().unwrap_or(0)
        };
        let modified = header.mtime().unwrap_or(0);
        let mode = header
            .mode()
            .unwrap_or_else(|_| if is_dir { 0o755 } else { 0o644 });
        let uid = header.uid().unwrap_or(0) as u32;
        let gid = header.gid().unwrap_or(0) as u32;

        let mut custom_fields = HashMap::new();
        custom_fields.insert(
            "archive_source".to_string(),
            abs_src.to_string_lossy().into_owned(),
        );
        if is_symlink {
            if let Ok(Some(link)) = entry.link_name() {
                custom_fields.insert(
                    "symlink_target".to_string(),
                    link.to_string_lossy().into_owned(),
                );
            }
        }

        entries.push(DecoratedEntry {
            path: format!("{}/{}", root_name, name),
            metadata: Some(EntryMetadata {
                size,
                modified,
                accessed: 0,
                created: 0,
                is_dir,
                is_file,
                is_symlink,
                permissions: mode as u32,
                uid,
                gid,
            }),
            custom_fields,
        });
    }

    synthesize_directory_entries(&mut entries, &root_name);
    Ok(entries)
}

pub fn read_tar_gz(path: &Path) -> Result<Vec<DecoratedEntry>> {
    let file = File::open(path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    read_tar(decoder, path)
}

pub fn read_tar_file(path: &Path) -> Result<Vec<DecoratedEntry>> {
    let file = File::open(path)?;
    read_tar(file, path)
}
