use super::{FileSorter, SortOptions};
use crate::error::Result;
use lla_plugin_interface::proto::DecoratedEntry;
use rayon::prelude::*;
use std::path::PathBuf;

pub struct SizeSorter;

impl FileSorter for SizeSorter {
    fn sort_files_with_metadata(
        &self,
        entries: &mut [(PathBuf, &DecoratedEntry)],
        options: SortOptions,
    ) -> Result<()> {
        entries.par_sort_unstable_by(|(_path_a, entry_a), (_path_b, entry_b)| {
            if options.dirs_first {
                let a_is_dir = entry_a.metadata.as_ref().map_or(false, |m| m.is_dir);
                let b_is_dir = entry_b.metadata.as_ref().map_or(false, |m| m.is_dir);

                match (a_is_dir, b_is_dir) {
                    (true, false) => {
                        return if options.reverse {
                            std::cmp::Ordering::Greater
                        } else {
                            std::cmp::Ordering::Less
                        }
                    }
                    (false, true) => {
                        return if options.reverse {
                            std::cmp::Ordering::Less
                        } else {
                            std::cmp::Ordering::Greater
                        }
                    }
                    _ => {}
                }
            }

            let size_a = entry_a.metadata.as_ref().map_or(0, |m| m.size);
            let size_b = entry_b.metadata.as_ref().map_or(0, |m| m.size);
            let size_order = size_a.cmp(&size_b);

            if options.reverse {
                size_order
            } else {
                size_order.reverse()
            }
        });

        Ok(())
    }
}
