use crate::error::Result;
use std::path::PathBuf;

#[derive(Clone, Copy, Default)]
pub struct SortOptions {
    pub reverse: bool,
    pub dirs_first: bool,
    pub case_sensitive: bool,
    pub natural: bool,
}

pub trait FileSorter: Send + Sync {
    fn sort_files_with_metadata(
        &self,
        entries: &mut [(PathBuf, &DecoratedEntry)],
        options: SortOptions,
    ) -> Result<()>;
}

mod alphabetical;
mod date;
mod size;

pub use alphabetical::AlphabeticalSorter;
pub use date::DateSorter;
use lla_plugin_interface::proto::DecoratedEntry;
pub use size::SizeSorter;

pub(crate) fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();

    while a_chars.peek().is_some() && b_chars.peek().is_some() {
        if a_chars.peek().unwrap().is_ascii_digit() && b_chars.peek().unwrap().is_ascii_digit() {
            let mut a_num = 0u64;
            let mut b_num = 0u64;

            while a_chars.peek().map_or(false, |c| c.is_ascii_digit()) {
                if let Some(digit) = a_chars.next() {
                    a_num = a_num * 10 + digit.to_digit(10).unwrap() as u64;
                }
            }

            while b_chars.peek().map_or(false, |c| c.is_ascii_digit()) {
                if let Some(digit) = b_chars.next() {
                    b_num = b_num * 10 + digit.to_digit(10).unwrap() as u64;
                }
            }

            match a_num.cmp(&b_num) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        } else {
            match a_chars.next().unwrap().cmp(&b_chars.next().unwrap()) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
    }

    match (a_chars.peek(), b_chars.peek()) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        _ => unreachable!(),
    }
}
