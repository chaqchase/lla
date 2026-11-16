use super::{render_entry_name, FileFormatter};
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::utils::color::*;
use colored::*;
use lla_plugin_interface::proto::DecoratedEntry;
use std::io::IsTerminal;
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
        let name_display = render_entry_name(entry, self.show_icons);

        let perms_display =
            colorize_permissions(metadata.permissions, Some(&self.permission_format));
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

        // Picked paths are usually consumed by another program
        // (`vim $(lla -F)`, `lla -F | xargs ...`): print them bare.
        if !std::io::stdout().is_terminal() {
            return Ok(plain_paths(files));
        }

        let mut output = String::new();
        for file in files {
            let line = self.format_entry(file, "", false, plugin_manager);
            output.push_str(&line);
            output.push('\n');
        }

        Ok(output)
    }
}

/// One path per line, without a trailing newline (the caller adds one).
fn plain_paths(files: &[DecoratedEntry]) -> String {
    files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_are_newline_separated() {
        let entry = |path: &str| DecoratedEntry {
            path: path.to_string(),
            ..Default::default()
        };
        assert_eq!(
            plain_paths(&[entry("a b.txt"), entry("src/main.rs")]),
            "a b.txt\nsrc/main.rs"
        );
    }
}
