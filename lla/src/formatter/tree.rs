use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::utils::color::colorize_file_name;
use colored::*;
use lla_plugin_interface::proto::DecoratedEntry;
use std::path::Path;

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
enum TreePart {
    Edge,
    Line,
    Corner,
}

impl TreePart {
    #[inline]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Edge => "├── ",
            Self::Line => "│   ",
            Self::Corner => "└── ",
        }
    }

    fn colored(self) -> ColoredString {
        self.as_str().bright_black()
    }
}

pub struct TreeFormatter;

impl TreeFormatter {
    fn format_entry(
        entry: &DecoratedEntry,
        prefix: &str,
        plugin_manager: &mut PluginManager,
        buf: &mut String,
    ) {
        buf.clear();
        let path = Path::new(&entry.path);
        buf.reserve(prefix.len() + path.as_os_str().len() + 1);
        buf.push_str(prefix);
        buf.push_str(&colorize_file_name(path).to_string());

        let plugin_fields = plugin_manager.format_fields(entry, "tree").join(" ");
        if !plugin_fields.is_empty() {
            buf.push(' ');
            buf.push_str(&plugin_fields);
        }

        buf.push('\n');
    }
}

#[derive(Debug)]
struct TreeTrunk {
    stack: Vec<TreePart>,
    last_depth: Option<(usize, bool)>,
}

impl Default for TreeTrunk {
    fn default() -> Self {
        Self {
            stack: Vec::with_capacity(32),
            last_depth: None,
        }
    }
}

impl TreeTrunk {
    #[inline]
    fn get_prefix(&mut self, depth: usize, is_absolute_last: bool, buf: &mut String) {
        if let Some((last_depth, _)) = self.last_depth {
            if last_depth < self.stack.len() {
                self.stack[last_depth] = TreePart::Line;
            }
        }

        if depth + 1 > self.stack.len() {
            self.stack.resize(depth + 1, TreePart::Line);
        }

        if depth < self.stack.len() {
            self.stack[depth] = if is_absolute_last {
                TreePart::Corner
            } else {
                TreePart::Edge
            };
        }

        self.last_depth = Some((depth, is_absolute_last));

        buf.clear();
        buf.reserve(depth * 4);
        for part in self.stack[1..=depth].iter() {
            buf.push_str(&part.colored());
        }
    }
}

impl FileFormatter for TreeFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        max_depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() {
            return Ok(String::new());
        }

        let mut trunk = TreeTrunk::default();
        let mut prefix_buf = String::with_capacity(128);
        let mut entry_buf = String::with_capacity(256);
        let mut result = String::new();

        let mut entries: Vec<_> = files
            .iter()
            .map(|entry| {
                let path = Path::new(&entry.path);
                let depth = path.components().count();
                (entry, depth, path.to_path_buf())
            })
            .collect();

        entries.sort_by(|a, b| a.2.cmp(&b.2));

        if let Some(max_depth) = max_depth {
            entries.retain(|(_, depth, _)| *depth <= max_depth);
        }

        let avg_line_len = entries
            .first()
            .map(|(e, d, _)| {
                let path = Path::new(&e.path);
                let name_len = path.file_name().map_or(0, |n| n.len());
                let prefix_len = *d * 4;
                name_len + prefix_len + 1
            })
            .unwrap_or(64);

        result.reserve(entries.len() * avg_line_len);

        const CHUNK_SIZE: usize = 8192;
        for chunk in entries.chunks(CHUNK_SIZE) {
            let chunk_len = chunk.len();
            for (i, (entry, depth, path)) in chunk.iter().enumerate() {
                let is_last = if i + 1 < chunk_len {
                    let (next_entry, next_depth, _) = &chunk[i + 1];
                    *depth > *next_depth
                        || !Path::new(&next_entry.path).starts_with(path.parent().unwrap_or(path))
                } else {
                    true
                };

                trunk.get_prefix(*depth, is_last, &mut prefix_buf);
                Self::format_entry(entry, &prefix_buf, plugin_manager, &mut entry_buf);
                result.push_str(&entry_buf);
            }
        }

        Ok(result)
    }
}
