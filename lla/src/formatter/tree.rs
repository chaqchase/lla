use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::utils::color::*;
use crate::utils::hyperlink;
use crate::utils::icons::format_with_icon;
use colored::Colorize;
use lla_plugin_interface::proto::DecoratedEntry;
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const BUFFER_SIZE: usize = 16384;

pub struct TreeFormatter {
    pub show_icons: bool,
}

impl TreeFormatter {
    pub fn new(show_icons: bool) -> Self {
        Self { show_icons }
    }

    fn format_entry(&self, path: &Path) -> String {
        let colored_name = colorize_file_name(path).to_string();
        let name = if self.show_icons {
            format_with_icon(path, colored_name, true)
        } else {
            colored_name
        };
        hyperlink::link_path(path, name)
    }

    fn build_tree(
        &self,
        entries: &[DecoratedEntry],
    ) -> (Vec<PathBuf>, HashMap<PathBuf, Vec<PathBuf>>) {
        let mut tree: HashMap<PathBuf, Vec<PathBuf>> = HashMap::with_capacity(entries.len());
        let mut path_set: HashSet<PathBuf> = HashSet::with_capacity(entries.len());
        let mut child_paths = HashSet::new();

        for entry in entries {
            path_set.insert(PathBuf::from(&entry.path));
        }

        for path in path_set.iter() {
            if let Some(parent) = path.parent() {
                if path_set.contains(parent) {
                    tree.entry(parent.to_path_buf())
                        .or_default()
                        .push(path.clone());
                    child_paths.insert(path.clone());
                }
            }
        }
        for children in tree.values_mut() {
            children.sort_unstable();
        }
        let mut root_paths: Vec<_> = path_set
            .into_iter()
            .filter(|path| !child_paths.contains(path))
            .collect();
        root_paths.sort_unstable();

        (root_paths, tree)
    }

    #[allow(clippy::too_many_arguments)]
    fn write_tree_recursive(
        &self,
        path: &Path,
        prefix: &str,
        is_last: bool,
        tree: &HashMap<PathBuf, Vec<PathBuf>>,
        writer: &mut impl Write,
        current_depth: usize,
        max_depth: Option<usize>,
    ) -> io::Result<()> {
        if let Some(max) = max_depth {
            if current_depth > max {
                return Ok(());
            }
        }

        let node_prefix = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };

        let formatted_name = self.format_entry(path);
        writeln!(
            writer,
            "{}{}{}",
            prefix.bright_black(),
            node_prefix.bright_black(),
            formatted_name
        )?;

        if let Some(children) = tree.get(path) {
            let new_prefix = format!("{}{}", prefix, child_prefix);
            let last_idx = children.len().saturating_sub(1);
            for (i, child) in children.iter().enumerate() {
                let is_last_child = i == last_idx;
                self.write_tree_recursive(
                    child,
                    &new_prefix,
                    is_last_child,
                    tree,
                    writer,
                    current_depth + 1,
                    max_depth,
                )?;
            }
        }
        Ok(())
    }
}

impl FileFormatter for TreeFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        _plugin_manager: &mut PluginManager,
        depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() {
            return Ok(String::new());
        }

        if depth == Some(0) {
            return Ok(String::new());
        }

        let (root_paths, tree) = self.build_tree(files);
        let mut buffer = Vec::with_capacity(BUFFER_SIZE);

        let last_idx = root_paths.len().saturating_sub(1);
        for (i, path) in root_paths.iter().enumerate() {
            let is_last = i == last_idx;
            self.write_tree_recursive(path, "", is_last, &tree, &mut buffer, 0, depth)?;
        }

        Ok(String::from_utf8_lossy(&buffer).into_owned())
    }
}
