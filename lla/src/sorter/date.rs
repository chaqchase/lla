use super::FileSorter;
use lla_plugin_interface::proto::DecoratedEntry;
use std::cmp::Ordering;

pub struct DateSorter;

impl FileSorter for DateSorter {
    fn compare(&self, a: &DecoratedEntry, b: &DecoratedEntry) -> Ordering {
        let modified = |entry: &DecoratedEntry| entry.metadata.as_ref().map_or(0, |m| m.modified);
        modified(a).cmp(&modified(b))
    }
}
