use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::utils::color::*;
use crate::utils::icons::format_with_icon;
use colored::*;
use lla_plugin_interface::proto::DecoratedEntry;
use std::path::Path;
use std::time::{Duration, SystemTime};

pub struct FuzzyFormatter {
    pub show_icons: bool,
    pub permission_format: String,
}

impl FuzzyFormatter {
    pub fn new(show_icons: bool, permission_format: String) -> Self {
        Self {
            show_icons,
            permission_format,
        }
    }

    fn format_entry(
        &self,
        entry: &DecoratedEntry,
        _query: &str,
        selected: bool,
        _plugin_manager: &mut PluginManager,
    ) -> String {
        let metadata = entry.metadata.as_ref().cloned().unwrap_or_default();
        let path = Path::new(&entry.path);

        let colored_name = colorize_file_name(path).to_string();
        let name_display = if self.show_icons {
            colorize_file_name_with_icon(
                path,
                format_with_icon(path, colored_name, self.show_icons),
            )
            .to_string()
        } else {
            colored_name
        };

        let perms = permissions_from_mode(metadata.permissions);
        let perms_display = colorize_permissions(&perms, Some(&self.permission_format));
        let size = colorize_size(metadata.size);
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(metadata.modified);
        let date = colorize_date(&modified);

        let prefix = if selected { "→" } else { " " };

        format!(
            "  {} {}  {}  {}  {}  {}",
            prefix,
            name_display,
            entry.path.bright_black(),
            perms_display,
            size,
            date
        )
    }
}

impl FileFormatter for FuzzyFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        _depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() {
            return Ok(String::new());
        }

        let mut output = String::new();
        for (idx, file) in files.iter().enumerate() {
            let line = self.format_entry(file, "", idx == 0, plugin_manager);
            output.push_str(&line);
            output.push_str("\n");
        }

        Ok(output)
    }
}
