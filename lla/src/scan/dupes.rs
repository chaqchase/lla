//! Large files with the same content in more than one place.

use super::clean::Target;
use super::model::Scan;
use super::rules::{Category, Rule, Rules};
use super::walk::{Node, MIN_FILE};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::hash::Hash;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// How much of each end of a file the first pass reads.
const EDGE: u64 = 64 * 1024;
const CHUNK: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Bytes each copy takes on disk.
    pub size: u64,
    /// Every copy, sorted by path.
    pub paths: Vec<PathBuf>,
}

impl Group {
    /// What removing all but one copy would free.
    pub fn wasted(&self) -> u64 {
        self.size * self.paths.len().saturating_sub(1) as u64
    }
}

#[derive(Default)]
pub struct Progress {
    /// Bytes read so far in the full comparison.
    pub hashed: AtomicU64,
    /// Bytes the full comparison has to read; 0 until the quick pass is done.
    pub total: AtomicU64,
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

#[derive(Clone, Debug, Default)]
pub struct Duplicates {
    /// Largest waste first.
    pub groups: Vec<Group>,
}

impl Duplicates {
    pub fn wasted(&self) -> u64 {
        self.groups.iter().map(Group::wasted).sum()
    }

    pub fn copies(&self) -> usize {
        self.groups.iter().map(|group| group.paths.len()).sum()
    }

    pub fn group_of(&self, path: &Path) -> Option<&Group> {
        self.groups
            .iter()
            .find(|group| group.paths.iter().any(|copy| copy == path))
    }

    /// Whether `path` can be marked and still leave a copy of its file unmarked.
    pub fn may_mark(&self, path: &Path, is_marked: impl Fn(&Path) -> bool) -> bool {
        self.group_of(path).is_none_or(|group| {
            group
                .paths
                .iter()
                .any(|copy| copy != path && !is_marked(copy))
        })
    }

    /// Takes out targets that would remove every copy of a file, keeping the
    /// first copy. Returns the paths that were kept.
    pub fn keep_one(&self, targets: &mut Vec<Target>) -> Vec<PathBuf> {
        let mut kept = Vec::new();
        for group in &self.groups {
            let all = group
                .paths
                .iter()
                .all(|copy| targets.iter().any(|target| &target.path == copy));
            if all {
                let keep = &group.paths[0];
                targets.retain(|target| &target.path != keep);
                kept.push(keep.clone());
            }
        }
        kept
    }

    /// Drops copies at or under `removed`, and groups left with one copy.
    pub fn forget(&mut self, removed: &[PathBuf]) {
        for group in &mut self.groups {
            group
                .paths
                .retain(|copy| !removed.iter().any(|gone| copy.starts_with(gone)));
        }
        self.groups.retain(|group| group.paths.len() > 1);
    }

    /// Keeps only copies that are still on disk.
    pub fn prune_missing(&mut self) {
        for group in &mut self.groups {
            group.paths.retain(|copy| copy.exists());
        }
        self.groups.retain(|group| group.paths.len() > 1);
    }
}

/// Large files worth comparing: only your own files. Apps, caches, build
/// folders, backups and the trash are skipped; removing one file from inside
/// them breaks the whole, and most are offered for cleaning as a whole anyway.
pub fn candidates(scan: &Scan, rules: &Rules) -> Vec<(PathBuf, u64)> {
    // Apps manage what they keep here (Messages attachments, for one).
    let app_data: Vec<PathBuf> = rules
        .home
        .iter()
        .flat_map(|home| [home.join("Library"), home.join("AppData")])
        .collect();
    let walk = Walk {
        rules,
        app_data: &app_data,
    };
    let mut out = Vec::new();
    for (root, node) in &scan.roots {
        if rules.enclosing(root).is_some_and(off_limits) {
            continue;
        }
        let mut path = root.clone();
        walk.collect(node, &mut path, &mut out);
    }
    out
}

struct Walk<'a> {
    rules: &'a Rules,
    app_data: &'a [PathBuf],
}

impl Walk<'_> {
    fn collect(&self, node: &Node, path: &mut PathBuf, out: &mut Vec<(PathBuf, u64)>) {
        if !node.dir {
            if node.size >= MIN_FILE {
                out.push((path.clone(), node.size));
            }
            return;
        }
        if node.tag != 0 || is_repository(node) {
            return;
        }
        for child in &node.children {
            if child.dir && skipped_folder(&child.name) {
                continue;
            }
            path.push(&*child.name);
            let skip = self.rules.exact(path).is_some_and(off_limits)
                || (child.dir && self.app_data.iter().any(|data| data == path));
            if !skip {
                self.collect(child, path, out);
            }
            path.pop();
        }
    }
}

fn off_limits(rule: &Rule) -> bool {
    !matches!(
        rule.category,
        Category::Downloads
            | Category::Documents
            | Category::Photos
            | Category::Videos
            | Category::Music
            | Category::Other
    )
}

/// Hidden folders hold app data, and macOS packages (apps, Photos and Music
/// libraries, projects) look like folders but are one document.
fn skipped_folder(name: &OsStr) -> bool {
    const PACKAGES: [&str; 19] = [
        "app",
        "framework",
        "bundle",
        "plugin",
        "photoslibrary",
        "musiclibrary",
        "tvlibrary",
        "imovielibrary",
        "fcpbundle",
        "logicx",
        "band",
        "xcarchive",
        "xcresult",
        "xcframework",
        "dsym",
        "lrlibrary",
        "sparsebundle",
        "vmwarevm",
        "utm",
    ];
    let name = name.to_string_lossy();
    name.starts_with('.')
        || Path::new(&*name)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                PACKAGES
                    .iter()
                    .any(|package| ext.eq_ignore_ascii_case(package))
            })
}

/// Files in a project are part of it: two checkouts that both vendor the same
/// binary are not waste. Covers working trees and bare clones, like the ones
/// Xcode and SwiftPM keep in build folders.
fn is_repository(node: &Node) -> bool {
    let has_dir = |name: &str| node.child(OsStr::new(name)).is_some_and(|child| child.dir);
    has_dir(".git") || (has_dir("objects") && has_dir("refs"))
}

/// Groups candidates by size, then by a hash of their first and last 64 KiB,
/// and confirms the rest with a hash of the whole file.
pub fn find(candidates: Vec<(PathBuf, u64)>, progress: &Progress) -> Vec<Group> {
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for (path, size) in candidates {
        by_size.entry(size).or_default().push(path);
    }
    let same_size: Vec<(u64, Vec<PathBuf>)> = by_size
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect();

    // Quick pass: exact length plus both ends of the file.
    let narrowed: Vec<(u64, u64, Vec<PathBuf>)> = same_size
        .into_par_iter()
        .flat_map_iter(|(size, paths)| {
            let keyed: Vec<((u64, [u8; 32]), PathBuf)> = without_links(paths)
                .into_iter()
                .filter_map(|path| {
                    if progress.cancelled() {
                        return None;
                    }
                    let len = fs::metadata(&path).ok()?.len();
                    let hash = edge_hash(&path, len).ok()?;
                    Some(((len, hash), path))
                })
                .collect();
            split(keyed)
                .into_iter()
                .map(move |(key, paths)| (size, key.0, paths))
        })
        .collect();
    if progress.cancelled() {
        return Vec::new();
    }

    let total: u64 = narrowed
        .iter()
        .map(|(_, len, paths)| len * paths.len() as u64)
        .sum();
    progress.total.store(total.max(1), Ordering::Relaxed);

    // Full pass over what is left, one file at a time per thread.
    let jobs: Vec<(usize, PathBuf)> = narrowed
        .iter()
        .enumerate()
        .flat_map(|(index, (_, _, paths))| paths.iter().map(move |path| (index, path.clone())))
        .collect();
    let hashed: Vec<((usize, [u8; 32]), PathBuf)> = jobs
        .into_par_iter()
        .filter_map(|(index, path)| {
            let hash = full_hash(&path, progress).ok()?;
            Some(((index, hash), path))
        })
        .collect();
    if progress.cancelled() {
        return Vec::new();
    }

    let mut groups: Vec<Group> = split(hashed)
        .into_iter()
        .map(|((index, _), mut paths)| {
            paths.sort();
            Group {
                size: narrowed[index].0,
                paths,
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        b.wasted()
            .cmp(&a.wasted())
            .then_with(|| a.paths.cmp(&b.paths))
    });
    groups
}

/// Paths that share a key, for keys with more than one path.
fn split<K: Hash + Eq>(keyed: Vec<(K, PathBuf)>) -> Vec<(K, Vec<PathBuf>)> {
    let mut map: HashMap<K, Vec<PathBuf>> = HashMap::new();
    for (key, path) in keyed {
        map.entry(key).or_default().push(path);
    }
    map.into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect()
}

/// Hard links are one file with several names; removing one frees nothing.
fn without_links(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mut seen = std::collections::HashSet::new();
        paths
            .into_iter()
            .filter(|path| {
                fs::metadata(path)
                    .map(|meta| seen.insert((meta.dev(), meta.ino())))
                    .unwrap_or(false)
            })
            .collect()
    }
    #[cfg(not(unix))]
    {
        paths
    }
}

fn edge_hash(path: &Path, len: u64) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; EDGE as usize];
    let head = read_up_to(&mut file, &mut buffer)?;
    hasher.update(&buffer[..head]);
    if len > 2 * EDGE {
        file.seek(SeekFrom::End(-(EDGE as i64)))?;
        let tail = read_up_to(&mut file, &mut buffer)?;
        hasher.update(&buffer[..tail]);
    }
    Ok(hasher.finalize().into())
}

fn full_hash(path: &Path, progress: &Progress) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        if progress.cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        progress.hashed.fetch_add(read as u64, Ordering::Relaxed);
    }
    Ok(hasher.finalize().into())
}

fn read_up_to(file: &mut File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn pattern(seed: u8, len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    #[test]
    fn finds_copies_and_ignores_lookalikes() {
        let root = tempfile::TempDir::new().unwrap();
        let a = root.path().join("a/movie.mp4");
        let b = root.path().join("b/movie copy.mp4");
        let same_size = root.path().join("c/other.mp4");
        // Same length and the same first and last 64 KiB, different middle.
        let mut middle = pattern(1, 3 << 20);
        middle[1 << 20] ^= 0xff;
        write(&a, &pattern(1, 3 << 20));
        write(&b, &pattern(1, 3 << 20));
        write(&same_size, &middle);

        let size = 3 << 20;
        let progress = Progress::default();
        let groups = find(
            vec![(a.clone(), size), (b.clone(), size), (same_size, size)],
            &progress,
        );
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[0].paths, vec![a, b]);
        assert_eq!(groups[0].wasted(), size);
        assert!(progress.hashed.load(Ordering::Relaxed) >= 2 * size);
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_are_not_duplicates() {
        let root = tempfile::TempDir::new().unwrap();
        let a = root.path().join("a.bin");
        let link = root.path().join("link.bin");
        write(&a, &pattern(7, 2 << 20));
        fs::hard_link(&a, &link).unwrap();
        let groups = find(vec![(a, 2 << 20), (link, 2 << 20)], &Progress::default());
        assert!(groups.is_empty(), "{groups:?}");
    }

    #[test]
    fn one_copy_always_stays() {
        let copies = vec![
            PathBuf::from("/x/a"),
            PathBuf::from("/y/a"),
            PathBuf::from("/z/a"),
        ];
        let mut dupes = Duplicates {
            groups: vec![Group {
                size: 10,
                paths: copies.clone(),
            }],
        };
        let marked = |path: &Path| path != Path::new("/z/a");
        assert!(dupes.may_mark(Path::new("/x/a"), |_| false));
        assert!(!dupes.may_mark(Path::new("/z/a"), marked));
        assert!(dupes.may_mark(Path::new("/elsewhere"), |_| true));

        let mut targets: Vec<Target> = copies
            .iter()
            .map(|path| Target {
                path: path.clone(),
                size: 10,
                permanent: false,
            })
            .collect();
        let kept = dupes.keep_one(&mut targets);
        assert_eq!(kept, vec![PathBuf::from("/x/a")]);
        assert_eq!(targets.len(), 2);

        dupes.forget(&[PathBuf::from("/y")]);
        assert_eq!(dupes.groups[0].paths.len(), 2);
        dupes.forget(&[PathBuf::from("/z/a")]);
        assert!(dupes.groups.is_empty());
    }

    #[test]
    fn only_compares_your_own_files() {
        use super::super::rules::{Mode, Safety};
        use super::super::walk::{scan, Progress as WalkProgress};
        let root = tempfile::TempDir::new().unwrap();
        let home = root.path().canonicalize().unwrap();
        let big = pattern(1, 2 << 20);
        for rel in [
            "docs/big.bin",
            "Downloads/big.bin",
            "cache/big.bin",
            "apps/Tool/big.bin",
            "site/node_modules/x/big.bin",
            "Applications/Some.app/Contents/big.bin",
            "Pictures/Photos Library.photoslibrary/originals/big.bin",
            ".ollama/models/big.bin",
            "code/app/vendor/big.bin",
            "Library/Messages/Attachments/a/big.mov",
            "build/Sparkle.framework.dSYM/Contents/big.bin",
            "code/app/.git/objects/pack/big.pack",
            "build/SourcePackages/repositories/Lib/objects/pack/big.pack",
        ] {
            write(&home.join(rel), &big);
        }
        write(&home.join("site/package.json"), b"{}");
        write(
            &home.join("build/SourcePackages/repositories/Lib/refs/heads/main"),
            b"x",
        );
        write(&home.join("small.txt"), b"tiny");
        let rule = |rel: &str, category, safety| Rule {
            path: home.join(rel),
            category,
            mode: Mode::Container,
            safety,
            reason: "test",
            permanent: false,
        };
        let rules = Rules::new(
            Some(home.clone()),
            vec![
                rule("cache", Category::Caches, Safety::Safe),
                rule("apps", Category::Apps, Safety::Review),
                rule("Downloads", Category::Downloads, Safety::Review),
            ],
        );
        let tree = scan(&home, &WalkProgress::default());
        let scanned = Scan {
            roots: vec![(home.clone(), tree)],
            volume: None,
            elapsed: std::time::Duration::ZERO,
            custom: true,
        };
        let mut found: Vec<PathBuf> = candidates(&scanned, &rules)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        found.sort();
        assert_eq!(
            found,
            vec![home.join("Downloads/big.bin"), home.join("docs/big.bin")]
        );
    }
}
