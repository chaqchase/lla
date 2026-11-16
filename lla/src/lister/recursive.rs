use super::FileLister;
use crate::config::Config;
use crate::error::Result;
use crate::lister::BasicLister;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct RecursiveLister {
    config: Config,
    skip_hidden: bool,
}

impl RecursiveLister {
    /// `skip_hidden` prunes dotfiles and whole dot-directories during the walk
    /// instead of filtering them out after their contents were read.
    pub fn new(config: Config, skip_hidden: bool) -> Self {
        Self {
            config,
            skip_hidden,
        }
    }
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

/// Resolves configured exclusions once so each entry only needs a prefix
/// comparison instead of a `canonicalize` call.
pub struct ExcludedPaths {
    prefixes: Vec<PathBuf>,
    root: PathBuf,
    canonical_root: Option<PathBuf>,
}

impl ExcludedPaths {
    pub fn new(config: &Config, root: &Path) -> Self {
        Self {
            prefixes: config
                .exclude_paths
                .iter()
                .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
                .collect(),
            root: root.to_path_buf(),
            canonical_root: if config.exclude_paths.is_empty() {
                None
            } else {
                root.canonicalize().ok()
            },
        }
    }

    pub fn contains(&self, path: &Path) -> bool {
        if self.prefixes.is_empty() {
            return false;
        }
        let absolute = match (&self.canonical_root, path.strip_prefix(&self.root)) {
            (Some(canonical_root), Ok(relative)) => canonical_root.join(relative),
            _ => path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
        };
        self.prefixes
            .iter()
            .any(|prefix| absolute.starts_with(prefix))
    }
}

impl FileLister for RecursiveLister {
    fn list_files(
        &self,
        directory: &str,
        recursive: bool,
        depth: Option<usize>,
    ) -> Result<Vec<PathBuf>> {
        if !recursive {
            return BasicLister.list_files(directory, false, None);
        }

        let max_files = self
            .config
            .listers
            .recursive
            .max_entries
            .unwrap_or(usize::MAX);
        let excluded = ExcludedPaths::new(&self.config, Path::new(directory));

        let mut entries = Vec::with_capacity(1024);
        let mut file_count = 0usize;
        let mut walker = WalkDir::new(directory)
            .max_depth(depth.unwrap_or(usize::MAX))
            .follow_links(false)
            .same_file_system(true)
            .into_iter();

        while let Some(entry) = walker.next() {
            // Unreadable subdirectories are skipped rather than aborting the
            // whole listing.
            let Ok(entry) = entry else { continue };
            let is_dir = entry.file_type().is_dir();

            if entry.depth() > 0 {
                if self.skip_hidden && is_hidden(entry.file_name()) {
                    if is_dir {
                        walker.skip_current_dir();
                    }
                    continue;
                }
                if excluded.contains(entry.path()) {
                    if is_dir {
                        walker.skip_current_dir();
                    }
                    continue;
                }
            }

            if !is_dir {
                if file_count >= max_files {
                    break;
                }
                file_count += 1;
            }

            // Show a repository's .git directory but not its object store.
            let is_git_dir = is_dir && entry.depth() > 0 && entry.file_name() == ".git";
            entries.push(entry.into_path());
            if is_git_dir {
                walker.skip_current_dir();
            }
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn names(root: &Path, paths: &[PathBuf]) -> Vec<String> {
        let mut names: Vec<_> = paths
            .iter()
            .filter_map(|path| path.strip_prefix(root).ok())
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .filter(|name| !name.is_empty())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn lists_dotfiles_unless_hidden_entries_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".config/nested")).unwrap();
        fs::write(root.path().join(".env"), "").unwrap();
        fs::write(root.path().join("visible"), "").unwrap();
        fs::write(root.path().join(".config/nested/file"), "").unwrap();
        let dir = root.path().to_str().unwrap();

        let all = RecursiveLister::new(Config::default(), false)
            .list_files(dir, true, None)
            .unwrap();
        assert_eq!(
            names(root.path(), &all),
            [
                ".config",
                ".config/nested",
                ".config/nested/file",
                ".env",
                "visible"
            ]
        );

        let visible = RecursiveLister::new(Config::default(), true)
            .list_files(dir, true, None)
            .unwrap();
        assert_eq!(names(root.path(), &visible), ["visible"]);
    }

    #[test]
    fn shows_git_directory_without_descending_into_it() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".git/objects")).unwrap();
        fs::write(root.path().join(".git/HEAD"), "").unwrap();
        let dir = root.path().to_str().unwrap();

        let listed = RecursiveLister::new(Config::default(), false)
            .list_files(dir, true, None)
            .unwrap();
        assert_eq!(names(root.path(), &listed), [".git"]);
    }

    #[test]
    fn excluded_directories_are_pruned() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("skip/deep")).unwrap();
        fs::create_dir_all(root.path().join("keep")).unwrap();
        let config = Config {
            exclude_paths: vec![root.path().join("skip")],
            ..Config::default()
        };

        let listed = RecursiveLister::new(config, false)
            .list_files(root.path().to_str().unwrap(), true, None)
            .unwrap();
        assert_eq!(names(root.path(), &listed), ["keep"]);
    }
}
