use crate::error::Result;
use std::path::PathBuf;

pub trait FileFilter: Send + Sync {
    fn filter_files(&self, files: &[PathBuf]) -> Result<Vec<PathBuf>>;
}

mod case_insensitive;
mod composite;
mod extension;
mod glob_filter;
mod pattern;
mod range;
mod regex_filter;

pub use case_insensitive::CaseInsensitiveFilter;
pub use composite::{CompositeFilter, FilterOperation};
pub use extension::ExtensionFilter;
pub use glob_filter::GlobFilter;
pub use pattern::PatternFilter;
pub use range::{parse_size_range, parse_time_range, NumericRange, TimeRange};
pub use regex_filter::RegexFilter;
