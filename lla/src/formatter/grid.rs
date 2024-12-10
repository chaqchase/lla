use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::utils::color::colorize_file_name;
use lla_plugin_interface::proto::DecoratedEntry;
use std::path::Path;
use terminal_size::{terminal_size, Width};
use unicode_width::UnicodeWidthStr;

pub struct GridFormatter;

impl FileFormatter for GridFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        _depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() {
            return Ok(String::new());
        }

        let term_width = terminal_size()
            .map(|(Width(w), _)| w as usize)
            .unwrap_or(80);

        let mut formatted_entries = Vec::with_capacity(files.len());
        let mut max_width = 0;

        for file in files {
            let path = Path::new(&file.path);
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let plugin_fields = plugin_manager.format_fields(file, "grid").join(" ");
            let total_str = if plugin_fields.is_empty() {
                name.to_string()
            } else {
                format!("{} {}", name, plugin_fields)
            };
            let width =
                String::from_utf8_lossy(&strip_ansi_escapes::strip(&total_str).unwrap_or_default())
                    .width();
            max_width = max_width.max(width);
            formatted_entries.push((colorize_file_name(path).to_string(), plugin_fields));
        }

        let column_width = max_width + 2;
        let num_columns = std::cmp::max(1, term_width / column_width);

        let mut output = String::new();
        let mut current_col = 0;

        for (colored_name, plugin_fields) in formatted_entries {
            let entry = if plugin_fields.is_empty() {
                colored_name
            } else {
                format!("{} {}", colored_name, plugin_fields)
            };

            if current_col >= num_columns {
                output.push('\n');
                current_col = 0;
            }

            output.push_str(&entry);
            if current_col < num_columns - 1 {
                let stripped_bytes = strip_ansi_escapes::strip(&entry).unwrap_or_default();
                let stripped = String::from_utf8_lossy(&stripped_bytes);
                let padding = column_width - stripped.width();
                output.push_str(&" ".repeat(padding));
            }

            current_col += 1;
        }

        output.push('\n');
        Ok(output)
    }
}
