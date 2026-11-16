use super::{render_entry_name, FileFormatter};
use crate::error::Result;
use crate::plugin::PluginManager;
use colored::Colorize;
use lla_plugin_interface::proto::DecoratedEntry;
use std::collections::HashMap;
use std::path::Path;

pub struct TreeFormatter {
    pub show_icons: bool,
}

struct Connectors {
    branch: String,
    last_branch: String,
    pipe: String,
    space: &'static str,
}

impl Connectors {
    fn new() -> Self {
        Self {
            branch: "├── ".bright_black().to_string(),
            last_branch: "└── ".bright_black().to_string(),
            pipe: "│   ".bright_black().to_string(),
            space: "    ",
        }
    }
}

impl TreeFormatter {
    pub fn new(show_icons: bool) -> Self {
        Self { show_icons }
    }

    /// Links every entry to its parent by index. Children keep the order of
    /// `entries`, which the caller has already sorted.
    fn build_tree(entries: &[DecoratedEntry]) -> (Vec<usize>, Vec<Vec<usize>>) {
        let index_of: HashMap<&Path, usize> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (Path::new(&entry.path), index))
            .collect();

        let mut roots = Vec::new();
        let mut children = vec![Vec::new(); entries.len()];
        for (index, entry) in entries.iter().enumerate() {
            let parent = Path::new(&entry.path)
                .parent()
                .and_then(|parent| index_of.get(parent))
                .filter(|&&parent| parent != index);
            match parent {
                Some(&parent) => children[parent].push(index),
                None => roots.push(index),
            }
        }
        (roots, children)
    }
}

struct TreeWriter<'a> {
    children: &'a [Vec<usize>],
    names: &'a [String],
    connectors: Connectors,
    max_depth: Option<usize>,
    prefix: String,
    output: String,
}

impl TreeWriter<'_> {
    fn write(&mut self, index: usize, is_last: bool, depth: usize) {
        if self.max_depth.is_some_and(|max| depth > max) {
            return;
        }

        self.output.push_str(&self.prefix);
        self.output.push_str(if is_last {
            &self.connectors.last_branch
        } else {
            &self.connectors.branch
        });
        self.output.push_str(&self.names[index]);
        self.output.push('\n');

        let children = &self.children[index];
        if children.is_empty() {
            return;
        }

        let prefix_len = self.prefix.len();
        if is_last {
            self.prefix.push_str(self.connectors.space);
        } else {
            self.prefix.push_str(&self.connectors.pipe);
        }
        let last = children.len() - 1;
        for (position, &child) in children.iter().enumerate() {
            self.write(child, position == last, depth + 1);
        }
        self.prefix.truncate(prefix_len);
    }
}

impl FileFormatter for TreeFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() || depth == Some(0) {
            return Ok(String::new());
        }

        plugin_manager.prepare_format_fields(files, "tree");
        let names: Vec<String> = files
            .iter()
            .map(|entry| {
                let name = render_entry_name(entry, self.show_icons);
                let fields = plugin_manager.format_fields(entry, "tree").join(" ");
                if fields.is_empty() {
                    name
                } else {
                    format!("{name} {fields}")
                }
            })
            .collect();

        let (roots, children) = Self::build_tree(files);
        let mut writer = TreeWriter {
            children: &children,
            names: &names,
            connectors: Connectors::new(),
            max_depth: depth,
            prefix: String::new(),
            output: String::with_capacity(files.len() * 48),
        };
        let last = roots.len().saturating_sub(1);
        for (position, &root) in roots.iter().enumerate() {
            writer.write(root, position == last, 0);
        }

        let mut output = writer.output;
        if output.ends_with('\n') {
            output.pop();
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lla_plugin_interface::proto::EntryMetadata;

    fn entry(path: &str, is_dir: bool) -> DecoratedEntry {
        DecoratedEntry {
            path: path.to_string(),
            metadata: Some(EntryMetadata {
                is_dir,
                is_file: !is_dir,
                ..EntryMetadata::default()
            }),
            ..DecoratedEntry::default()
        }
    }

    #[test]
    fn children_keep_the_sorted_input_order() {
        let entries = vec![
            entry("root", true),
            entry("root/zeta", false),
            entry("root/sub", true),
            entry("root/sub/inner", false),
            entry("root/alpha", false),
        ];
        let (roots, children) = TreeFormatter::build_tree(&entries);
        assert_eq!(roots, vec![0]);
        assert_eq!(children[0], vec![1, 2, 4]);
        assert_eq!(children[2], vec![3]);
    }
}
