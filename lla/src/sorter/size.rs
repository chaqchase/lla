use super::FileSorter;
use lla_plugin_interface::proto::DecoratedEntry;
use std::cmp::Ordering;

pub struct SizeSorter;

impl FileSorter for SizeSorter {
    fn compare(&self, a: &DecoratedEntry, b: &DecoratedEntry) -> Ordering {
        let size = |entry: &DecoratedEntry| entry.metadata.as_ref().map_or(0, |m| m.size);
        // Largest first.
        size(b).cmp(&size(a))
    }
}
