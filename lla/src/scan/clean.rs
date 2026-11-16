//! Moving things to the trash, deleting them, and showing them in the file manager.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Trash,
    Delete,
}

pub struct Target {
    pub path: PathBuf,
    pub size: u64,
    /// Already in a trash; moving it there again makes no sense.
    pub permanent: bool,
}

#[derive(Default)]
pub struct Outcome {
    pub trashed: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
    pub bytes: u64,
}

pub fn clean(targets: &[Target], method: Method) -> Outcome {
    let mut outcome = Outcome::default();
    for target in targets {
        let delete = method == Method::Delete || target.permanent;
        let result = if delete {
            remove(&target.path)
        } else {
            trash(&target.path)
        };
        match result {
            Ok(()) => {
                outcome.bytes += target.size;
                if delete {
                    forget_trash_info(&target.path);
                    outcome.deleted.push(target.path.clone());
                } else {
                    outcome.trashed.push(target.path.clone());
                }
            }
            Err(error) => outcome.failed.push((target.path.clone(), error)),
        }
    }
    outcome
}

fn trash(path: &Path) -> Result<(), String> {
    #[allow(unused_mut)]
    let mut context = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        // The Finder method asks for Automation access and plays a sound per file.
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context.delete(path).map_err(|error| match error {
        trash::Error::CouldNotAccess { .. } => "could not access it".to_string(),
        trash::Error::TargetedRoot => "refusing to trash a root folder".to_string(),
        other => other.to_string(),
    })
}

fn remove(path: &Path) -> Result<(), String> {
    let run = || -> io::Result<()> {
        let meta = fs::symlink_metadata(path)?;
        if meta.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        }
    };
    match run() {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            // Go's module cache and similar trees are read-only on purpose.
            make_writable(path);
            run().map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn make_writable(path: &Path) {
    for entry in walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.is_dir() {
                let mode = meta.permissions().mode();
                if mode & 0o200 == 0 {
                    let _ =
                        fs::set_permissions(entry.path(), fs::Permissions::from_mode(mode | 0o700));
                }
            }
        }
        #[cfg(windows)]
        #[allow(clippy::permissions_set_readonly_false)]
        {
            let mut permissions = meta.permissions();
            if permissions.readonly() {
                permissions.set_readonly(false);
                let _ = fs::set_permissions(entry.path(), permissions);
            }
        }
        #[cfg(not(any(unix, windows)))]
        let _ = meta;
    }
}

/// Freedesktop trashes keep a `.trashinfo` file next to each trashed item.
fn forget_trash_info(path: &Path) {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    if parent.file_name().and_then(|n| n.to_str()) != Some("files") {
        return;
    }
    let Some(trash_root) = parent.parent() else {
        return;
    };
    let mut info = name.to_os_string();
    info.push(".trashinfo");
    let _ = fs::remove_file(trash_root.join("info").join(info));
}

/// Shows `path` selected in Finder, Explorer or the default file manager.
pub fn reveal(path: &Path) -> io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg("-R").arg(path);
        command
    } else if cfg!(windows) {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        let mut command = Command::new("explorer");
        command.arg(arg);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        });
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// Where trashed items end up, so the tree can show them there without a rescan.
pub fn trash_folder(home: &Path) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(home.join(".Trash"))
    } else if cfg!(all(unix, not(target_os = "macos"))) {
        Some(home.join(".local/share/Trash/files"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletes_read_only_trees() {
        let root = tempfile::TempDir::new().unwrap();
        let dir = root.path().join("mod/cache");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file"), "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        }
        let outcome = clean(
            &[Target {
                path: root.path().join("mod"),
                size: 1,
                permanent: false,
            }],
            Method::Delete,
        );
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        assert!(!root.path().join("mod").exists());
        assert_eq!(outcome.deleted.len(), 1);
    }
}
