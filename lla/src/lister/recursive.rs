use super::FileLister;
use crate::commands::args::{Args, DotfilesMode};
use crate::config::Config;
use crate::error::Result;
use crate::lister::BasicLister;
use crate::utils::walk::build_walk_builder;
use ignore::{DirEntry, WalkState};
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const PARALLEL_THRESHOLD: usize = 1000;
const BUFFER_CAPACITY: usize = 1024;

pub struct RecursiveLister {
    config: Config,
}

impl RecursiveLister {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    fn matches_dotfiles_mode(entry: &DirEntry, mode: DotfilesMode) -> bool {
        if entry.depth() == 0 {
            return true;
        }

        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_dot = name.starts_with('.');
        let is_current_or_parent = name == "." || name == "..";

        match mode {
            DotfilesMode::Hide => !is_dot,
            DotfilesMode::ShowAll => true,
            DotfilesMode::ShowAlmostAll => !is_current_or_parent,
            DotfilesMode::Only => is_dot && !is_current_or_parent,
        }
    }

    fn max_entries_limit(&self) -> Option<usize> {
        self.config
            .listers
            .recursive
            .max_entries
            .and_then(|limit| match limit {
                0 | usize::MAX => None,
                value => Some(value),
            })
    }

    fn is_excluded(path: &Path, exclude_prefixes: &[PathBuf]) -> bool {
        if exclude_prefixes.is_empty() {
            return false;
        }

        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        exclude_prefixes
            .iter()
            .any(|prefix| canonical.starts_with(prefix))
    }
}

impl FileLister for RecursiveLister {
    fn list_files(
        &self,
        args: &Args,
        directory: &str,
        recursive: bool,
        depth: Option<usize>,
    ) -> Result<Vec<PathBuf>> {
        if !recursive {
            return BasicLister.list_files(args, directory, false, None);
        }

        let limit = self.max_entries_limit();
        let root = Path::new(directory);
        let mut builder = build_walk_builder(root, args, &self.config);
        builder
            .max_depth(depth)
            .follow_links(false)
            .same_file_system(true);

        let exclude_prefixes: Vec<PathBuf> = self
            .config
            .exclude_paths
            .iter()
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
            .collect();
        let exclude_prefixes_arc = Arc::new(exclude_prefixes);
        let dotfiles_mode = args.dotfiles_mode;

        if !exclude_prefixes_arc.is_empty() {
            let exclude_for_filter = Arc::clone(&exclude_prefixes_arc);
            builder.filter_entry(move |entry| {
                !RecursiveLister::is_excluded(entry.path(), &exclude_for_filter)
            });
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let use_parallel = match limit {
            Some(limit) => limit > PARALLEL_THRESHOLD,
            None => true,
        };

        let entries = if use_parallel {
            let results = Arc::new(Mutex::new(Vec::with_capacity(BUFFER_CAPACITY)));
            let exclude_for_walk = Arc::clone(&exclude_prefixes_arc);
            let counter_for_walk = Arc::clone(&counter);

            builder.build_parallel().run(|| {
                let results = Arc::clone(&results);
                let exclude = Arc::clone(&exclude_for_walk);
                let counter = Arc::clone(&counter_for_walk);
                let limit = limit;
                let mode = dotfiles_mode;

                Box::new(move |event| {
                    if let Some(max_limit) = limit {
                        if counter.load(Ordering::Relaxed) >= max_limit {
                            return WalkState::Quit;
                        }
                    }

                    let entry = match event {
                        Ok(entry) => entry,
                        Err(_) => return WalkState::Continue,
                    };

                    if !RecursiveLister::matches_dotfiles_mode(&entry, mode) {
                        return WalkState::Continue;
                    }

                    if RecursiveLister::is_excluded(entry.path(), &exclude) {
                        return WalkState::Continue;
                    }

                    if entry.file_type().map_or(false, |ft| ft.is_file()) {
                        if let Some(max_limit) = limit {
                            let prev = counter.fetch_add(1, Ordering::Relaxed);
                            if prev >= max_limit {
                                return WalkState::Quit;
                            }
                        }
                    }

                    results.lock().unwrap().push(entry.into_path());
                    WalkState::Continue
                })
            });

            match Arc::try_unwrap(results) {
                Ok(mutex) => mutex.into_inner().unwrap(),
                Err(arc) => {
                    let mut guard = arc.lock().unwrap();
                    mem::take(&mut *guard)
                }
            }
        } else {
            let mut collected = Vec::with_capacity(BUFFER_CAPACITY);
            let exclude_for_walk = exclude_prefixes_arc;
            for event in builder.build() {
                if let Some(max_limit) = limit {
                    if counter.load(Ordering::Relaxed) >= max_limit {
                        break;
                    }
                }

                let entry = match event {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };

                if !RecursiveLister::matches_dotfiles_mode(&entry, dotfiles_mode) {
                    continue;
                }

                if RecursiveLister::is_excluded(entry.path(), &exclude_for_walk) {
                    continue;
                }

                if entry.file_type().map_or(false, |ft| ft.is_file()) {
                    if let Some(max_limit) = limit {
                        let prev = counter.fetch_add(1, Ordering::Relaxed);
                        if prev >= max_limit {
                            break;
                        }
                    }
                }

                collected.push(entry.into_path());
            }
            collected
        };
        Ok(entries)
    }
}
