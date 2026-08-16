use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::theme::{self, ColorValue};
use crate::utils::color::{self, colorize_file_name, colorize_file_name_with_icon};
use crate::utils::hyperlink;
use crate::utils::icons::format_with_icon;
use chrono::{DateTime, Local};
use colored::*;
use lla_plugin_interface::proto::DecoratedEntry;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

pub struct RecursiveFormatter {
    pub show_icons: bool,
}

impl RecursiveFormatter {
    pub fn new(show_icons: bool) -> Self {
        Self { show_icons }
    }

    fn format_date(timestamp: u64) -> ColoredString {
        let datetime = UNIX_EPOCH + Duration::from_secs(timestamp);
        let datetime: DateTime<Local> = DateTime::from(datetime);
        let date_str = datetime.format("%Y-%m-%d %H:%M").to_string();
        let color = theme::color_value_to_color(&ColorValue::Named("bright black".to_string()));
        date_str.color(color)
    }

    fn get_header_color() -> Color {
        let theme = color::get_theme();
        theme::color_value_to_color(&theme.colors.directory)
    }

    fn get_separator_color() -> Color {
        theme::color_value_to_color(&ColorValue::Named("bright black".to_string()))
    }

    fn print_entry(&self, entry: &DecoratedEntry, plugin_manager: &mut PluginManager) {
        if let Some(metadata) = &entry.metadata {
            let date = Self::format_date(metadata.modified);
            let path = Path::new(&entry.path);
            let colored_name = colorize_file_name(path).to_string();
            let name = colorize_file_name_with_icon(
                path,
                format_with_icon(path, colored_name, self.show_icons),
            )
            .to_string();
            let name = hyperlink::link_path(path, name);

            print!("{:>16} │ {}", date, name);

            let plugin_fields = plugin_manager.format_fields(entry, "recursive").join(" ");
            if !plugin_fields.is_empty() {
                print!(" {}", plugin_fields);
            }
            println!();
        }
    }

    fn get_parent_path(path: &str) -> String {
        Path::new(path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or(".")
            .to_string()
    }
}

impl FileFormatter for RecursiveFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        _depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() {
            return Ok(String::new());
        }
        plugin_manager.prepare_format_fields(files, "recursive");

        let mut groups: BTreeMap<String, Vec<&DecoratedEntry>> = BTreeMap::new();

        for file in files {
            let parent = Self::get_parent_path(&file.path);
            groups.entry(parent).or_default().push(file);
        }

        let single_group = groups.len() == 1;

        for (parent, entries) in &groups {
            if !(single_group && parent == ".") {
                println!("\n{}", parent.color(Self::get_header_color()).bold());
                println!("{}", "─".repeat(40).color(Self::get_separator_color()));
            }

            for entry in entries {
                self.print_entry(entry, plugin_manager);
            }
        }
        Ok(String::new())
    }
}
