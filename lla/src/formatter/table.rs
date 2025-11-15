use super::column_config::ColumnKey;
use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::theme::{self, ColorValue};
use crate::utils::color::{self, *};
use crate::utils::icons::format_with_icon;
use colored::*;
use lla_plugin_interface::proto::DecoratedEntry;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use unicode_width::UnicodeWidthStr;
use users::{get_group_by_gid, get_user_by_uid};

static USER_CACHE: Lazy<Mutex<HashMap<u32, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static GROUP_CACHE: Lazy<Mutex<HashMap<u32, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub struct TableFormatter {
    pub show_icons: bool,
    pub permission_format: String,
    columns: Vec<ColumnKey>,
    has_plugins_column: bool,
}

impl TableFormatter {
    pub fn new(show_icons: bool, permission_format: String, columns: Vec<ColumnKey>) -> Self {
        let mut final_columns = if columns.is_empty() {
            vec![
                ColumnKey::Permissions,
                ColumnKey::Size,
                ColumnKey::Modified,
                ColumnKey::Name,
            ]
        } else {
            columns
        };

        if !final_columns.iter().any(|c| matches!(c, ColumnKey::Name)) {
            final_columns.push(ColumnKey::Name);
        }

        let has_plugins_column = final_columns.iter().any(|c| c.is_plugins());

        Self {
            show_icons,
            permission_format,
            columns: final_columns,
            has_plugins_column,
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

    fn create_header(headers: &[String], widths: &[usize]) -> String {
        let border_color = Self::get_border_color();
        let header_color = Self::get_header_color();
        let mut header = String::new();
        header.push('│');

        for (width, title) in widths.iter().zip(headers.iter()) {
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

    fn create_row(values: &[String], widths: &[usize], align_right: &[bool]) -> String {
        let border_color = Self::get_border_color();
        let mut row = String::new();
        row.push('│');

        for ((value, width), align) in values.iter().zip(widths.iter()).zip(align_right.iter()) {
            row.push(' ');
            row.push_str(&Self::format_cell(value, *width, *align));
            row.push(' ');
            row.push('│');
        }

        row.color(border_color).to_string()
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

impl TableFormatter {
    fn render_column(
        &self,
        entry: &DecoratedEntry,
        metadata: &lla_plugin_interface::proto::EntryMetadata,
        column: &ColumnKey,
        plugin_text: &str,
    ) -> String {
        match column {
            ColumnKey::Permissions => {
                let perms = Permissions::from_mode(metadata.permissions);
                colorize_permissions(&perms, Some(&self.permission_format))
            }
            ColumnKey::Size => colorize_size(metadata.size).to_string(),
            ColumnKey::Modified => self.format_timestamp(metadata.modified),
            ColumnKey::Created => self.format_timestamp(metadata.created),
            ColumnKey::Accessed => self.format_timestamp(metadata.accessed),
            ColumnKey::User => colorize_user(&lookup_user(metadata.uid)).to_string(),
            ColumnKey::Group => colorize_group(&lookup_group(metadata.gid)).to_string(),
            ColumnKey::Name => self.render_name(entry),
            ColumnKey::Path => entry.path.clone(),
            ColumnKey::Plugins => plugin_text.to_string(),
            ColumnKey::CustomField(field) => entry
                .custom_fields
                .get(field)
                .cloned()
                .unwrap_or_else(|| "-".to_string()),
        }
    }

    fn render_name(&self, entry: &DecoratedEntry) -> String {
        let path = Path::new(&entry.path);
        let colored_name = colorize_file_name(path).to_string();
        colorize_file_name_with_icon(path, format_with_icon(path, colored_name, self.show_icons))
            .to_string()
    }

    fn format_timestamp(&self, seconds: u64) -> String {
        if seconds == 0 {
            return "-".to_string();
        }
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds);
        colorize_date(&time).to_string()
    }
}

fn lookup_user(uid: u32) -> String {
    let mut cache = USER_CACHE.lock().unwrap();
    if let Some(cached) = cache.get(&uid) {
        return cached.clone();
    }
    let resolved = get_user_by_uid(uid)
        .map(|u| u.name().to_string_lossy().into_owned())
        .unwrap_or_else(|| uid.to_string());
    cache.insert(uid, resolved.clone());
    resolved
}

fn lookup_group(gid: u32) -> String {
    let mut cache = GROUP_CACHE.lock().unwrap();
    if let Some(cached) = cache.get(&gid) {
        return cached.clone();
    }
    let resolved = get_group_by_gid(gid)
        .map(|g| g.name().to_string_lossy().into_owned())
        .unwrap_or_else(|| gid.to_string());
    cache.insert(gid, resolved.clone());
    resolved
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

        let headers: Vec<String> = self
            .columns
            .iter()
            .map(|column| column.header_label())
            .collect();
        let mut widths: Vec<usize> = headers
            .iter()
            .map(|header| Self::visible_width(header))
            .collect();
        let mut rows = Vec::with_capacity(files.len());

        for entry in files {
            let metadata = entry.metadata.as_ref().cloned().unwrap_or_default();
            let plugin_text = plugin_manager.format_fields(entry, "table").join(" ");
            let plugin_suffix = if self.has_plugins_column || plugin_text.is_empty() {
                String::new()
            } else {
                format!(" {}", plugin_text)
            };

            let mut values = Vec::with_capacity(self.columns.len());
            for (idx, column) in self.columns.iter().enumerate() {
                let value = self.render_column(entry, &metadata, column, &plugin_text);
                let width = Self::visible_width(&value);
                if width > widths[idx] {
                    widths[idx] = width;
                }
                values.push(value);
            }
            rows.push((values, plugin_suffix));
        }

        let alignments: Vec<bool> = self
            .columns
            .iter()
            .map(|column| matches!(column, ColumnKey::Size))
            .collect();

        let mut output = String::new();
        output.push_str(&Self::create_top_border(&widths));
        output.push('\n');
        output.push_str(&Self::create_header(&headers, &widths));
        output.push('\n');
        output.push_str(&Self::create_separator(&widths));
        output.push('\n');

        for (values, plugin_suffix) in rows {
            output.push_str(&Self::create_row(&values, &widths, &alignments));
            if !plugin_suffix.is_empty() {
                output.push_str(&plugin_suffix);
            }
            output.push('\n');
        }

        output.push_str(&Self::create_bottom_border(&widths));
        output.push('\n');

        Ok(output)
    }
}
