use lla_plugin_interface::proto::DecoratedEntry;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, TimeZone, Utc};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use users::{get_group_by_gid, get_user_by_uid};

static USER_CACHE: Lazy<Mutex<HashMap<u32, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static GROUP_CACHE: Lazy<Mutex<HashMap<u32, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Serialize)]
pub struct SerializableEntry {
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub file_type: String,
    pub size_bytes: u64,
    pub modified: String,
    pub created: Option<String>,
    pub accessed: Option<String>,
    pub mode_octal: String,
    pub owner_user: Option<String>,
    pub owner_group: Option<String>,
    pub inode: Option<u64>,
    pub hard_links: Option<u64>,
    pub symlink_target: Option<String>,
    pub is_hidden: bool,
    pub git_status: Option<String>,
    pub plugin: HashMap<String, serde_json::Value>,
}

fn fmt_ts_opt(secs: u64) -> Option<String> {
    if secs == 0 {
        return None;
    }
    let dt = Utc.timestamp_opt(secs as i64, 0).single();
    dt.map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn fmt_ts_required(secs: u64) -> String {
    let dt = Utc.timestamp_opt(secs as i64, 0).single();
    dt.map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

fn mode_to_octal(permissions: u32) -> String {
    format!("{:04o}", permissions & 0o7777)
}

fn uid_to_name(uid: u32) -> Option<String> {
    if uid == 0 && get_user_by_uid(uid).is_none() {
        return None;
    }
    let mut cache = USER_CACHE.lock().unwrap();
    if let Some(name) = cache.get(&uid) {
        return Some(name.clone());
    }
    let name = get_user_by_uid(uid).map(|u| u.name().to_string_lossy().into_owned());
    if let Some(ref n) = name {
        cache.insert(uid, n.clone());
    }
    name
}

fn gid_to_name(gid: u32) -> Option<String> {
    if gid == 0 && get_group_by_gid(gid).is_none() {
        return None;
    }
    let mut cache = GROUP_CACHE.lock().unwrap();
    if let Some(name) = cache.get(&gid) {
        return Some(name.clone());
    }
    let name = get_group_by_gid(gid).map(|g| g.name().to_string_lossy().into_owned());
    if let Some(ref n) = name {
        cache.insert(gid, n.clone());
    }
    name
}

pub fn to_serializable(entry: &DecoratedEntry, git_status: Option<String>) -> SerializableEntry {
    let path = Path::new(&entry.path);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_string());

    let md = entry.metadata.as_ref().cloned().unwrap_or_default();

    let file_type = if md.is_symlink {
        "symlink"
    } else if md.is_dir {
        "dir"
    } else if md.is_file {
        "file"
    } else {
        "other"
    }
    .to_string();

    // Extra FS data
    let (inode, hard_links) = {
        #[cfg(unix)]
        {
            match fs::symlink_metadata(&entry.path) {
                Ok(m) => {
                    use std::os::unix::fs::MetadataExt;
                    (Some(m.ino()), Some(m.nlink() as u64))
                }
                Err(_) => (None, None),
            }
        }
        #[cfg(not(unix))]
        {
            (None, None)
        }
    };

    let symlink_target = if md.is_symlink {
        if let Some(t) = entry.custom_fields.get("symlink_target") {
            Some(t.clone())
        } else if let Ok(target) = fs::read_link(&entry.path) {
            Some(target.to_string_lossy().into_owned())
        } else {
            None
        }
    } else {
        None
    };

    let is_hidden = name.starts_with('.');

    let owner_user = {
        #[cfg(unix)]
        {
            uid_to_name(md.uid)
        }
        #[cfg(windows)]
        {
            crate::windows_metadata::get_windows_owner(path)
        }
        #[cfg(not(any(unix, windows)))]
        {
            None
        }
    };

    let owner_group = {
        #[cfg(unix)]
        {
            gid_to_name(md.gid)
        }
        #[cfg(windows)]
        {
            crate::windows_metadata::get_windows_group(path)
        }
        #[cfg(not(any(unix, windows)))]
        {
            None
        }
    };

    let mut plugin: HashMap<String, serde_json::Value> = HashMap::new();
    for (k, v) in &entry.custom_fields {
        plugin.insert(k.clone(), serde_json::Value::String(v.clone()));
    }

    SerializableEntry {
        path: entry.path.clone(),
        name,
        extension,
        file_type,
        size_bytes: md.size,
        modified: fmt_ts_required(md.modified),
        created: fmt_ts_opt(md.created),
        accessed: fmt_ts_opt(md.accessed),
        mode_octal: mode_to_octal(md.permissions),
        owner_user,
        owner_group,
        inode,
        hard_links,
        symlink_target,
        is_hidden,
        git_status,
        plugin,
    }
}

pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return None,
        }
    }
}

pub fn get_git_status_map(workspace_root: &Path) -> HashMap<String, String> {
    use std::process::Command;
    let mut status_map = HashMap::new();

    if let Ok(output) = Command::new("git")
        .args(["status", "--porcelain=v2", "--untracked-files=all"])
        .current_dir(workspace_root)
        .output()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0] {
                "1" | "2" if parts.len() >= 9 => {
                    let xy = parts[1];
                    let path = parts[8];
                    status_map.insert(path.to_string(), xy.to_string());
                }
                "?" if parts.len() >= 2 => {
                    status_map.insert(parts[1].to_string(), "??".to_string());
                }
                "!" if parts.len() >= 2 => {
                    status_map.insert(parts[1].to_string(), "!!".to_string());
                }
                _ => {}
            }
        }
    }

    if let Ok(output) = std::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(workspace_root)
        .output()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            status_map
                .entry(line.to_string())
                .or_insert_with(|| ".".to_string());
        }
    }

    status_map
}
