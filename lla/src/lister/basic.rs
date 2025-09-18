use super::FileLister;
use crate::commands::args::Args;
use crate::error::Result;
use std::fs;
use std::path::PathBuf;

pub struct BasicLister;

impl FileLister for BasicLister {
    fn list_files(
        &self,
        _args: &Args,
        directory: &str,
        _recursive: bool,
        _depth: Option<usize>,
    ) -> Result<Vec<PathBuf>> {
        let mut files = Vec::with_capacity(16);

        let entries = fs::read_dir(directory)?;
        // No config available here; exclusion is applied later in list_and_decorate_files
        for entry in entries.flatten() {
            let p = entry.path();
            // Skip current and parent dir entries if the underlying FS yields them
            if p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == "." || n == "..")
                .unwrap_or(false)
            {
                continue;
            }
            files.push(p);
        }

        Ok(files)
    }
}
