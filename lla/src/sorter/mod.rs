use lla_plugin_interface::proto::DecoratedEntry;
use rayon::prelude::*;
use std::cmp::Ordering;
use std::path::Path;

#[derive(Clone, Copy, Default)]
pub struct SortOptions {
    pub reverse: bool,
    pub dirs_first: bool,
    pub case_sensitive: bool,
    pub natural: bool,
}

pub trait FileSorter: Send + Sync {
    /// Primary ordering for this sort key. Entries that compare equal are
    /// ordered by name, so the output is deterministic.
    fn compare(&self, a: &DecoratedEntry, b: &DecoratedEntry) -> Ordering;
}

mod alphabetical;
mod date;
mod size;

pub use alphabetical::AlphabeticalSorter;
pub use date::DateSorter;
pub use size::SizeSorter;

/// Sorts entries in place. Name keys are computed once per entry instead of
/// once per comparison, and the sort is stable.
pub fn sort_entries(
    entries: &mut Vec<DecoratedEntry>,
    sorter: &dyn FileSorter,
    options: SortOptions,
) {
    if entries.len() < 2 {
        return;
    }

    let keys: Vec<String> = entries
        .par_iter()
        .map(|entry| {
            let name = Path::new(&entry.path)
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default();
            if options.case_sensitive {
                name.into_owned()
            } else {
                name.to_lowercase()
            }
        })
        .collect();

    let is_dir = |entry: &DecoratedEntry| entry.metadata.as_ref().is_some_and(|m| m.is_dir);
    let mut order: Vec<usize> = (0..entries.len()).collect();
    {
        let entries = &*entries;
        order.par_sort_by(|&i, &j| {
            let (a, b) = (&entries[i], &entries[j]);
            // Like `ls --group-directories-first`, directories stay first
            // even when the order is reversed.
            let group = if options.dirs_first {
                is_dir(b).cmp(&is_dir(a))
            } else {
                Ordering::Equal
            };
            let ordering = sorter
                .compare(a, b)
                .then_with(|| {
                    if options.natural {
                        natural_cmp(&keys[i], &keys[j])
                    } else {
                        keys[i].cmp(&keys[j])
                    }
                })
                .then_with(|| a.path.cmp(&b.path));
            group.then(if options.reverse {
                ordering.reverse()
            } else {
                ordering
            })
        });
    }

    let mut slots: Vec<Option<DecoratedEntry>> = entries.drain(..).map(Some).collect();
    entries.extend(order.into_iter().filter_map(|i| slots[i].take()));
}

/// Compares strings treating runs of ASCII digits as numbers of any length.
pub(crate) fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);

    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let a_start = i;
            let b_start = j;
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let a_digits = trim_leading_zeros(&a[a_start..i]);
            let b_digits = trim_leading_zeros(&b[b_start..j]);
            let ordering = a_digits
                .len()
                .cmp(&b_digits.len())
                .then_with(|| a_digits.cmp(b_digits))
                // "01" sorts after "1" so equal values still have a stable order.
                .then_with(|| (i - a_start).cmp(&(j - b_start)));
            if ordering != Ordering::Equal {
                return ordering;
            }
        } else {
            // Compare whole UTF-8 sequences byte-wise; ASCII digits never
            // appear inside a multi-byte sequence.
            match a[i].cmp(&b[j]) {
                Ordering::Equal => {
                    i += 1;
                    j += 1;
                }
                other => return other,
            }
        }
    }

    (a.len() - i).cmp(&(b.len() - j))
}

fn trim_leading_zeros(digits: &[u8]) -> &[u8] {
    let first_non_zero = digits
        .iter()
        .position(|&digit| digit != b'0')
        .unwrap_or(digits.len());
    &digits[first_non_zero..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use lla_plugin_interface::proto::EntryMetadata;

    fn entry(path: &str, size: u64, modified: u64, is_dir: bool) -> DecoratedEntry {
        DecoratedEntry {
            path: path.to_string(),
            metadata: Some(EntryMetadata {
                size,
                modified,
                is_dir,
                is_file: !is_dir,
                ..EntryMetadata::default()
            }),
            ..DecoratedEntry::default()
        }
    }

    fn names(entries: &[DecoratedEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.path.as_str()).collect()
    }

    #[test]
    fn natural_order_handles_long_digit_runs_and_leading_zeros() {
        assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
        assert_eq!(
            natural_cmp("v123456789012345678901234", "v99999999999999999999999"),
            Ordering::Greater
        );
        assert_eq!(natural_cmp("a1", "a01"), Ordering::Less);
        assert_eq!(natural_cmp("a", "a1"), Ordering::Less);
        assert_eq!(natural_cmp("x9y", "x9y"), Ordering::Equal);
    }

    #[test]
    fn equal_sizes_fall_back_to_name_order() {
        let mut entries = vec![
            entry("c", 5, 0, false),
            entry("a", 5, 0, false),
            entry("big", 9, 0, false),
            entry("b", 5, 0, false),
        ];
        sort_entries(&mut entries, &SizeSorter, SortOptions::default());
        assert_eq!(names(&entries), ["big", "a", "b", "c"]);
    }

    #[test]
    fn natural_sort_respects_case_insensitivity() {
        let mut entries = vec![
            entry("File10", 0, 0, false),
            entry("file2", 0, 0, false),
            entry("FILE1", 0, 0, false),
        ];
        let options = SortOptions {
            natural: true,
            ..SortOptions::default()
        };
        sort_entries(&mut entries, &AlphabeticalSorter, options);
        assert_eq!(names(&entries), ["FILE1", "file2", "File10"]);
    }

    #[test]
    fn dirs_first_and_reverse_compose() {
        let mut entries = vec![
            entry("b.txt", 0, 0, false),
            entry("dir", 0, 0, true),
            entry("a.txt", 0, 0, false),
        ];
        let options = SortOptions {
            dirs_first: true,
            ..SortOptions::default()
        };
        sort_entries(&mut entries, &AlphabeticalSorter, options);
        assert_eq!(names(&entries), ["dir", "a.txt", "b.txt"]);

        let options = SortOptions {
            reverse: true,
            dirs_first: true,
            ..SortOptions::default()
        };
        sort_entries(&mut entries, &AlphabeticalSorter, options);
        assert_eq!(names(&entries), ["dir", "b.txt", "a.txt"]);

        let options = SortOptions {
            reverse: true,
            ..SortOptions::default()
        };
        sort_entries(&mut entries, &AlphabeticalSorter, options);
        assert_eq!(names(&entries), ["dir", "b.txt", "a.txt"]);
    }
}
