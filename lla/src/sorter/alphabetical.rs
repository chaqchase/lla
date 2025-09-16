use super::{natural_cmp, FileSorter, SortOptions};
use crate::error::Result;
use lla_plugin_interface::proto::DecoratedEntry;
use rayon::prelude::*;
use std::path::PathBuf;

pub struct AlphabeticalSorter;

impl FileSorter for AlphabeticalSorter {
    fn sort_files_with_metadata(
        &self,
        entries: &mut [(PathBuf, &DecoratedEntry)],
        options: SortOptions,
    ) -> Result<()> {
        entries.par_sort_unstable_by(|(path_a, entry_a), (path_b, entry_b)| {
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

            let a_name = path_a.file_name().unwrap_or_default().to_string_lossy();
            let b_name = path_b.file_name().unwrap_or_default().to_string_lossy();

            let name_order = if options.natural {
                natural_cmp(&a_name, &b_name)
            } else if options.case_sensitive {
                a_name.cmp(&b_name)
            } else {
                a_name.to_lowercase().cmp(&b_name.to_lowercase())
            };

            if options.reverse {
                name_order.reverse()
            } else {
                name_order
            }
        });

        Ok(())
    }
}
