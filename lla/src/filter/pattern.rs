use super::FileFilter;
use crate::error::Result;
use std::path::{Path, PathBuf};

pub struct PatternFilter {
    patterns: Vec<String>,
    match_all: bool,
}

impl PatternFilter {
    pub fn new(pattern: String) -> Self {
        let patterns: Vec<String> = if pattern.contains(',') {
            pattern
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            vec![pattern]
        };

        let match_all = patterns.iter().any(|p| p.starts_with('+'));
        let patterns = patterns
            .into_iter()
            .map(|p| {
                if let Some(pattern) = p.strip_prefix('+') {
                    pattern.to_string()
                } else {
                    p
                }
            })
            .collect();

        PatternFilter {
            patterns,
            match_all,
        }
    }

    fn matches_pattern(&self, path: &Path) -> bool {
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            if self.match_all {
                return self.patterns.iter().all(|pattern| name.contains(pattern));
            } else if self.patterns.iter().any(|pattern| name.contains(pattern)) {
                return true;
            }
        }

        if let Some(path_str) = path.to_str() {
            if self.match_all {
                self.patterns
                    .iter()
                    .all(|pattern| path_str.contains(pattern))
            } else {
                self.patterns
                    .iter()
                    .any(|pattern| path_str.contains(pattern))
            }
        } else {
            false
        }
    }
}

impl FileFilter for PatternFilter {
    fn filter_files(&self, files: &[PathBuf]) -> Result<Vec<PathBuf>> {
        Ok(files
            .iter()
            .filter(|file| self.matches_pattern(file))
            .cloned()
            .collect())
    }
}
