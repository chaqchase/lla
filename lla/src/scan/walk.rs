//! Parallel disk usage walk that keeps a compact tree of folders and big files.

use super::rules::artifact_tag;
use dashmap::DashSet;
use parking_lot::Mutex;
use rayon::prelude::*;
use std::ffi::{OsStr, OsString};
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

/// Files smaller than this are summed into their folder instead of kept as nodes.
/// It keeps memory flat on homes with millions of files.
pub const MIN_FILE: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Node {
    pub name: Box<OsStr>,
    /// Bytes on disk, including everything below.
    pub size: u64,
    pub files: u64,
    /// Newest modification time below this node, in seconds since the epoch.
    pub newest: i64,
    pub dir: bool,
    /// 1-based index into the build-artifact table, 0 when none.
    pub tag: u8,
    /// Folders below this one that could not be read.
    pub unreadable: u64,
    /// Bytes and count of the small files not kept as children.
    pub small: u64,
    pub small_count: u64,
    /// Sorted by size, largest first.
    pub children: Vec<Node>,
}

impl Node {
    fn file(name: OsString, size: u64, modified: i64) -> Self {
        Self {
            name: name.into_boxed_os_str(),
            size,
            files: 1,
            newest: modified,
            dir: false,
            tag: 0,
            unreadable: 0,
            small: 0,
            small_count: 0,
            children: Vec::new(),
        }
    }

    pub fn empty_dir(name: &OsStr) -> Self {
        Self {
            name: name.into(),
            size: 0,
            files: 0,
            newest: 0,
            dir: true,
            tag: 0,
            unreadable: 0,
            small: 0,
            small_count: 0,
            children: Vec::new(),
        }
    }

    pub fn child(&self, name: &OsStr) -> Option<&Node> {
        self.children.iter().find(|child| &*child.name == name)
    }

    pub fn child_mut(&mut self, name: &OsStr) -> Option<&mut Node> {
        self.children.iter_mut().find(|child| &*child.name == name)
    }

    /// Follows `rel` down from this node.
    pub fn find(&self, rel: &Path) -> Option<&Node> {
        rel.iter().try_fold(self, |node, part| node.child(part))
    }

    /// Detaches the node at `rel` and returns it, fixing up the totals above it.
    pub fn remove(&mut self, rel: &Path) -> Option<Node> {
        let mut parts = rel.iter();
        let first = parts.next()?;
        let rest: PathBuf = parts.collect();
        let removed = if rest.as_os_str().is_empty() {
            let index = self
                .children
                .iter()
                .position(|child| &*child.name == first)?;
            self.children.remove(index)
        } else {
            self.child_mut(first)?.remove(&rest)?
        };
        self.size = self.size.saturating_sub(removed.size);
        self.files = self.files.saturating_sub(removed.files);
        self.unreadable = self.unreadable.saturating_sub(removed.unreadable);
        Some(removed)
    }

    /// Adds `node` as a child of the folder at `rel`, fixing up totals.
    pub fn insert(&mut self, rel: &Path, node: Node) -> bool {
        let mut parts = rel.iter();
        let target = match parts.next() {
            None => {
                self.add_child(node);
                return true;
            }
            Some(first) => first,
        };
        let rest: PathBuf = parts.collect();
        let (size, files, newest) = (node.size, node.files, node.newest);
        let Some(child) = self.child_mut(target) else {
            return false;
        };
        if !child.insert(&rest, node) {
            return false;
        }
        self.size += size;
        self.files += files;
        self.newest = self.newest.max(newest);
        true
    }

    fn add_child(&mut self, node: Node) {
        self.size += node.size;
        self.files += node.files;
        self.newest = self.newest.max(node.newest);
        let at = self
            .children
            .iter()
            .position(|child| child.size < node.size)
            .unwrap_or(self.children.len());
        self.children.insert(at, node);
    }
}

#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub dirs: AtomicU64,
    pub unreadable: AtomicU64,
    pub current: Mutex<PathBuf>,
    cancel: AtomicBool,
}

impl Progress {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub fn allocated(meta: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.blocks() * 512
    }
    #[cfg(not(unix))]
    {
        meta.len()
    }
}

fn modified(meta: &Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs() as i64)
}

#[cfg(unix)]
fn device(meta: &Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.dev())
}

#[cfg(not(unix))]
fn device(_meta: &Metadata) -> Option<u64> {
    None
}

struct Walker<'a> {
    progress: &'a Progress,
    device: Option<u64>,
    /// (device, inode) of hard-linked files already counted.
    #[cfg_attr(not(unix), allow(dead_code))]
    seen: DashSet<(u64, u64)>,
}

impl Walker<'_> {
    fn first_sighting(&self, meta: &Metadata) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.nlink() > 1 {
                return self.seen.insert((meta.dev(), meta.ino()));
            }
        }
        let _ = meta;
        true
    }

    fn dir(&self, path: &Path, name: Box<OsStr>, meta: &Metadata, in_artifact: bool) -> Node {
        let mut node = Node::empty_dir(&name);
        node.size = allocated(meta);
        node.newest = modified(meta);
        if self.progress.cancelled() {
            return node;
        }

        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) => {
                node.unreadable = 1;
                self.progress.unreadable.fetch_add(1, Ordering::Relaxed);
                return node;
            }
        };

        let mut names = Vec::new();
        let mut subdirs = Vec::new();
        let mut file_bytes = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let kind = meta.file_type();
            if kind.is_dir() {
                if self.device.is_some() && device(&meta) != self.device {
                    // Another mounted filesystem; it has its own free space.
                    continue;
                }
                subdirs.push((name.clone(), meta));
            } else if self.first_sighting(&meta) {
                let size = if kind.is_symlink() {
                    0
                } else {
                    allocated(&meta)
                };
                let time = modified(&meta);
                node.files += 1;
                node.newest = node.newest.max(time);
                file_bytes += size;
                if size >= MIN_FILE {
                    node.children.push(Node::file(name.clone(), size, time));
                } else {
                    node.small += size;
                    node.small_count += 1;
                }
            }
            names.push(name);
        }
        node.size += file_bytes;
        self.progress.files.fetch_add(node.files, Ordering::Relaxed);
        self.progress.bytes.fetch_add(file_bytes, Ordering::Relaxed);
        self.progress.dirs.fetch_add(1, Ordering::Relaxed);
        if let Some(mut current) = self.progress.current.try_lock() {
            current.clear();
            current.push(path);
        }

        let tagged: Vec<(OsString, Metadata, u8)> = subdirs
            .into_iter()
            .map(|(name, meta)| {
                let tag = if in_artifact {
                    0
                } else {
                    artifact_tag(&name, &names)
                };
                (name, meta, tag)
            })
            .collect();
        drop(names);

        let children: Vec<Node> = tagged
            .into_par_iter()
            .map(|(name, meta, tag)| {
                let child_path = path.join(&name);
                let mut child = self.dir(
                    &child_path,
                    name.into_boxed_os_str(),
                    &meta,
                    in_artifact || tag != 0,
                );
                child.tag = tag;
                child
            })
            .collect();

        for child in &children {
            node.size += child.size;
            node.files += child.files;
            node.newest = node.newest.max(child.newest);
            node.unreadable += child.unreadable;
        }
        node.children.extend(children);
        node.children
            .sort_unstable_by_key(|child| std::cmp::Reverse(child.size));
        node
    }
}

fn pool() -> &'static rayon::ThreadPool {
    static POOL: once_cell::sync::OnceCell<rayon::ThreadPool> = once_cell::sync::OnceCell::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            // Deep trees (nested node_modules) recurse a long way.
            .stack_size(16 * 1024 * 1024)
            .thread_name(|index| format!("lla-scan-{index}"))
            .build()
            .expect("start scan threads")
    })
}

/// Walks `path` without following symlinks or leaving its filesystem.
pub fn scan(path: &Path, progress: &Progress) -> Node {
    let name: Box<OsStr> = path.file_name().unwrap_or(path.as_os_str()).into();
    let Ok(meta) = fs::symlink_metadata(path) else {
        let mut node = Node::empty_dir(&name);
        node.unreadable = 1;
        progress.unreadable.fetch_add(1, Ordering::Relaxed);
        return node;
    };
    if !meta.is_dir() {
        return Node::file(name.into(), allocated(&meta), modified(&meta));
    }
    let in_artifact = path
        .parent()
        .and_then(|parent| {
            let names: Vec<OsString> = fs::read_dir(parent)
                .ok()?
                .flatten()
                .map(|entry| entry.file_name())
                .collect();
            Some(artifact_tag(&name, &names))
        })
        .unwrap_or(0);
    let walker = Walker {
        progress,
        device: device(&meta),
        seen: DashSet::new(),
    };
    let mut node = pool().install(|| walker.dir(path, name, &meta, in_artifact != 0));
    node.tag = in_artifact;
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_sizes_and_tags_artifacts() {
        let root = tempfile::TempDir::new().unwrap();
        let project = root.path().join("app");
        fs::create_dir_all(project.join("target/debug")).unwrap();
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("Cargo.toml"), "[package]").unwrap();
        fs::write(project.join("target/debug/big"), vec![1u8; 2 * 1024 * 1024]).unwrap();
        fs::write(project.join("src/main.rs"), "fn main() {}").unwrap();

        let progress = Progress::default();
        let tree = scan(root.path(), &progress);
        let app = tree.child(OsStr::new("app")).unwrap();
        let target = app.child(OsStr::new("target")).unwrap();
        assert_ne!(target.tag, 0);
        assert_eq!(app.child(OsStr::new("src")).unwrap().tag, 0);
        assert!(target.size >= 2 * 1024 * 1024);
        assert_eq!(app.children[0].name.as_ref(), OsStr::new("target"));
        assert_eq!(tree.files, 3);

        let mut tree = tree;
        let before = tree.size;
        let removed = tree.remove(Path::new("app/target")).unwrap();
        assert_eq!(tree.size, before - removed.size);
        assert!(tree.find(Path::new("app/target")).is_none());
        assert!(tree.insert(Path::new("app"), removed));
        assert_eq!(tree.size, before);
    }
}
