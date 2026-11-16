use crate::error::Result;
use std::path::PathBuf;

pub trait FileLister {
    fn list_files(
        &self,
        directory: &str,
        recursive: bool,
        depth: Option<usize>,
    ) -> Result<Vec<PathBuf>>;
}

pub mod archive;
mod basic;
mod fuzzy;
mod graphics;
mod grep;
mod preview;
mod recursive;

pub use basic::BasicLister;
pub use fuzzy::{FuzzyLister, SearchMode};
pub use recursive::{ExcludedPaths, RecursiveLister};
