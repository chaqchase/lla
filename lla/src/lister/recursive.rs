use super::FileLister;
use crate::config::Config;
use crate::error::Result;
use crate::lister::BasicLister;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use walkdir::{DirEntry, WalkDir};

const PARALLEL_THRESHOLD: usize = 1000;
const BUFFER_CAPACITY: usize = 1024;

pub struct RecursiveLister {
    config: Config,
}

impl RecursiveLister {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    fn is_hidden(entry: &DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    }

    fn should_process_entry(
        entry: &DirEntry,
        counter: &Arc<AtomicUsize>,
        max_entries: usize,
    ) -> bool {
        // Skip excluded directories early
        // If the entry path starts with any excluded prefix, skip it entirely
        // Note: We cannot access self.config here; exclusion is handled in filter and at walker creation
        if counter.load(Ordering::Relaxed) >= max_entries {
            return false;
        }

        if !entry.file_type().is_file() {
            return true;
        }

        if !Self::is_hidden(entry) {
            counter.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
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

        let max_depth = depth.unwrap_or(usize::MAX);
        let max_entries = self
            .config
            .listers
            .recursive
            .max_entries
            .unwrap_or(usize::MAX);

        let counter = Arc::new(AtomicUsize::new(0));
        let mut entries = Vec::with_capacity(BUFFER_CAPACITY);

        // Pre-compute excluded prefixes as absolute PathBufs (canonicalize best-effort)
        let exclude_prefixes: Vec<PathBuf> = self
            .config
            .exclude_paths
            .iter()
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
            .collect();

        let walker = WalkDir::new(directory)
            .min_depth(0)
            .max_depth(max_depth)
            .follow_links(false)
            .same_file_system(true)
            .into_iter()
            .filter_entry(|e| {
                if exclude_prefixes.is_empty() {
                    return true;
                }
                // Compare against canonicalized path best-effort to avoid mismatches
                let p = match e.path().canonicalize() {
                    Ok(abs) => abs,
                    Err(_) => e.path().to_path_buf(),
                };
                // If this entry is exactly or under an excluded prefix, skip descending
                !exclude_prefixes.iter().any(|ex| p.starts_with(ex))
            })
            .filter_map(|e| e.ok())
            .filter(|e| Self::should_process_entry(e, &counter, max_entries))
            .collect::<Vec<_>>();

        if walker.len() > PARALLEL_THRESHOLD {
            entries.par_extend(walker.into_par_iter().map(|e| e.into_path()).filter(|p| {
                let p_abs = p.canonicalize().unwrap_or_else(|_| p.clone());
                !exclude_prefixes.iter().any(|ex| p_abs.starts_with(ex))
            }));
        } else {
            entries.extend(walker.into_iter().map(|e| e.into_path()).filter(|p| {
                let p_abs = p.canonicalize().unwrap_or_else(|_| p.clone());
                !exclude_prefixes.iter().any(|ex| p_abs.starts_with(ex))
            }));
        }

        Ok(entries)
    }
}
