use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::theme::{self, ColorValue};
use crate::utils::color::{self, *};
use crate::utils::icons::format_with_icon;
use colored::*;
use lla_plugin_interface::proto::DecoratedEntry;
use std::cmp;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};
use unicode_width::UnicodeWidthStr;

pub struct TableFormatter {
    pub show_icons: bool,
    pub permission_format: String,
}

impl TableFormatter {
    pub fn new(show_icons: bool, permission_format: String) -> Self {
        Self {
            show_icons,
            permission_format,
        }
    }
}
impl TableFormatter {
    const PADDING: usize = 1;

    fn strip_ansi(s: &str) -> String {
        String::from_utf8(strip_ansi_escapes::strip(s).unwrap_or_default()).unwrap_or_default()
    }

    fn visible_width(s: &str) -> usize {
        Self::strip_ansi(s).width()
    }

    fn calculate_column_widths(files: &[DecoratedEntry], permission_format: &str) -> [usize; 4] {
        let mut widths = [
            "Permissions".len(),
            "Size".len(),
            "Modified".len(),
            "Name".len(),
        ];

        for entry in files {
            let metadata = entry.metadata.as_ref().cloned().unwrap_or_default();
            let perms = permissions_from_mode(metadata.permissions);
            let perms = colorize_permissions(&perms, Some(permission_format));
            widths[0] = cmp::max(widths[0], Self::visible_width(&perms));

            let size: ColoredString = colorize_size(metadata.size);
            widths[1] = cmp::max(widths[1], Self::visible_width(&size));

            let modified = UNIX_EPOCH + Duration::from_secs(metadata.modified);
            let date = colorize_date(&modified);
            widths[2] = cmp::max(widths[2], Self::visible_width(&date));

            let path = Path::new(&entry.path);
            let colored_name = colorize_file_name(path).to_string();
            let name_with_icon =
                colorize_file_name_with_icon(path, format_with_icon(path, colored_name, true));
            widths[3] = cmp::max(widths[3], Self::visible_width(&name_with_icon));
        }

        widths
    }

    fn get_border_color() -> Color {
        theme::color_value_to_color(&ColorValue::Named("bright black".to_string()))
    }

    fn get_header_color() -> Color {
        let theme = color::get_theme();
        theme::color_value_to_color(&theme.colors.directory)
    }

    fn create_separator(widths: &[usize]) -> String {
        let border_color = Self::get_border_color();
        let mut separator = String::new();
        separator.push('├');
        for (i, &width) in widths.iter().enumerate() {
            separator.push_str(&"─".repeat(width + Self::PADDING * 2));
            if i < widths.len() - 1 {
                separator.push('┼');
            }
        }
        separator.push('┤');
        separator.color(border_color).to_string()
    }

    fn create_header(widths: &[usize]) -> String {
        let border_color = Self::get_border_color();
        let header_color = Self::get_header_color();
        let headers = ["Permissions", "Size", "Modified", "Name"];
        let mut header = String::new();
        header.push('│');

        for (&width, &title) in widths.iter().zip(headers.iter()) {
            header.push(' ');
            header.push_str(
                &format!("{:width$}", title, width = width)
                    .color(header_color)
                    .bold()
                    .to_string(),
            );
            header.push(' ');
            header.push('│');
        }
        header.color(border_color).to_string()
    }

    fn create_top_border(widths: &[usize]) -> String {
        let border_color = Self::get_border_color();
        let mut border = String::new();
        border.push('┌');
        for (i, &width) in widths.iter().enumerate() {
            border.push_str(&"─".repeat(width + Self::PADDING * 2));
            if i < widths.len() - 1 {
                border.push('┬');
            }
        }
        border.push('┐');
        border.color(border_color).to_string()
    }

    fn create_bottom_border(widths: &[usize]) -> String {
        let border_color = Self::get_border_color();
        let mut border = String::new();
        border.push('└');
        for (i, &width) in widths.iter().enumerate() {
            border.push_str(&"─".repeat(width + Self::PADDING * 2));
            if i < widths.len() - 1 {
                border.push('┴');
            }
        }
        border.push('┘');
        border.color(border_color).to_string()
    }

    fn format_cell(content: &str, width: usize, align_right: bool) -> String {
        let visible_width = Self::visible_width(content);
        let padding = width.saturating_sub(visible_width);

        if align_right {
            format!("{}{}", " ".repeat(padding), content)
        } else {
            format!("{}{}", content, " ".repeat(padding))
        }
    }
}

impl FileFormatter for TableFormatter {
    fn format_files(
        &self,
        files: &[lla_plugin_interface::proto::DecoratedEntry],
        plugin_manager: &mut PluginManager,
        _depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() {
            return Ok(String::new());
        }

        let widths = Self::calculate_column_widths(files, &self.permission_format);

        let mut output = String::new();
        output.push_str(&Self::create_top_border(&widths));
        output.push('\n');
        output.push_str(&Self::create_header(&widths));
        output.push('\n');
        output.push_str(&Self::create_separator(&widths));
        output.push('\n');

        for entry in files {
            let metadata = entry.metadata.as_ref().cloned().unwrap_or_default();
            let perms = permissions_from_mode(metadata.permissions);
            let perms = colorize_permissions(&perms, Some(&self.permission_format));
            let size = colorize_size(metadata.size);
            let modified = UNIX_EPOCH + Duration::from_secs(metadata.modified);
            let date = colorize_date(&modified);
            let path = Path::new(&entry.path);
            let colored_name = colorize_file_name(path).to_string();
            let name = format_with_icon(path, colored_name, self.show_icons);

            let plugin_fields = plugin_manager.format_fields(entry, "table").join(" ");
            let plugin_suffix = if plugin_fields.is_empty() {
                String::new()
            } else {
                format!(" {}", plugin_fields)
            };

            let formatted_perms = Self::format_cell(&perms, widths[0], false);
            let formatted_size = Self::format_cell(&size, widths[1], true);
            let formatted_date = Self::format_cell(&date, widths[2], false);
            let formatted_name = Self::format_cell(&name, widths[3], false);

            output.push_str(&format!(
                "│{pad}{}{pad}│{pad}{}{pad}│{pad}{}{pad}│{pad}{}{pad}│{}\n",
                formatted_perms,
                formatted_size,
                formatted_date,
                formatted_name,
                plugin_suffix,
                pad = " ",
            ));
        }

        output.push_str(&Self::create_bottom_border(&widths));
        output.push('\n');

        Ok(output)
    }
}
