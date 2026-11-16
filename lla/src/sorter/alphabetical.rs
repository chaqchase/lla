use super::FileSorter;
use lla_plugin_interface::proto::DecoratedEntry;
use std::cmp::Ordering;

pub struct AlphabeticalSorter;

impl FileSorter for AlphabeticalSorter {
    fn compare(&self, _a: &DecoratedEntry, _b: &DecoratedEntry) -> Ordering {
        // Name order is the shared tie-breaker applied by `sort_entries`.
        Ordering::Equal
    }
}
