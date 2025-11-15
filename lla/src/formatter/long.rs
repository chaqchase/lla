use super::column_config::ColumnKey;
use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::utils::color::*;
use crate::utils::icons::format_with_icon;
use console;
use lla_plugin_interface::proto::{DecoratedEntry, EntryMetadata};
use once_cell::sync::Lazy;
use unicode_width::UnicodeWidthStr;

use std::collections::HashMap;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use users::{get_group_by_gid, get_user_by_uid};

static USER_CACHE: Lazy<Mutex<HashMap<u32, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static GROUP_CACHE: Lazy<Mutex<HashMap<u32, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub struct LongFormatter {
    pub show_icons: bool,
    pub permission_format: String,
    pub hide_group: bool,
    pub relative_dates: bool,
    columns: Vec<ColumnKey>,
    has_plugins_column: bool,
}

impl LongFormatter {
    pub fn new(
        show_icons: bool,
        permission_format: String,
        hide_group: bool,
        relative_dates: bool,
        columns: Vec<ColumnKey>,
    ) -> Self {
        let filtered_columns: Vec<ColumnKey> = columns
            .into_iter()
            .filter(|column| !(hide_group && column.is_group()))
            .collect();
        let final_columns = if filtered_columns.is_empty() {
            vec![ColumnKey::Name]
        } else {
            filtered_columns
        };

        let has_plugins_column = final_columns.iter().any(|c| c.is_plugins());

        Self {
            show_icons,
            permission_format,
            hide_group,
            relative_dates,
            columns: final_columns,
            has_plugins_column,
        }
    }
}

impl FileFormatter for LongFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        _depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() {
            return Ok(String::new());
        }

        let mut widths = vec![0usize; self.columns.len()];
        let mut rendered_rows: Vec<Vec<String>> = Vec::with_capacity(files.len());

        for entry in files {
            let metadata = entry.metadata.as_ref().cloned().unwrap_or_default();
            let plugin_text = plugin_manager.format_fields(entry, "long").join(" ");
            let mut row = Vec::with_capacity(self.columns.len());
            for (idx, column) in self.columns.iter().enumerate() {
                let value = self.render_column(entry, &metadata, column, &plugin_text);
                let width = visible_width(&value);
                if width > widths[idx] {
                    widths[idx] = width;
                }
                row.push(value);
            }
            rendered_rows.push(row);
        }

        let mut output = String::new();
        for row in rendered_rows {
            let mut segments = Vec::with_capacity(row.len());
            for (idx, value) in row.into_iter().enumerate() {
                let segment = if self.columns[idx].align_right() {
                    pad_left(&value, widths[idx])
                } else {
                    pad_right(&value, widths[idx])
                };
                segments.push(segment);
            }
            if !segments.is_empty() {
                output.push_str(segments.join(" ").trim_end());
            }
            output.push('\n');
        }

        if output.ends_with('\n') {
            output.pop();
        }
        Ok(output)
    }
}

impl LongFormatter {
    fn render_column(
        &self,
        entry: &DecoratedEntry,
        metadata: &EntryMetadata,
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
            ColumnKey::Group => {
                if self.hide_group {
                    String::new()
                } else {
                    colorize_group(&lookup_group(metadata.gid)).to_string()
                }
            }
            ColumnKey::Name => self.render_name(entry, metadata, plugin_text),
            ColumnKey::Path => entry.path.clone(),
            ColumnKey::Plugins => plugin_text.to_string(),
            ColumnKey::CustomField(field) => entry
                .custom_fields
                .get(field)
                .cloned()
                .unwrap_or_else(|| "-".to_string()),
        }
    }

    fn render_name(
        &self,
        entry: &DecoratedEntry,
        metadata: &EntryMetadata,
        plugin_text: &str,
    ) -> String {
        let path = Path::new(&entry.path);
        let colored_name = colorize_file_name(path).to_string();
        let base_name = colorize_file_name_with_icon(
            path,
            format_with_icon(path, colored_name, self.show_icons),
        )
        .to_string();

        let with_target = if metadata.is_symlink {
            if let Some(target) = entry.custom_fields.get("symlink_target") {
                if entry.custom_fields.get("invalid_symlink").is_some() {
                    let broken_target = console::style(target).red().bold();
                    format!("{} -> {} (broken)", base_name, broken_target)
                } else {
                    format!(
                        "{} -> {}",
                        base_name,
                        colorize_symlink_target(Path::new(target))
                    )
                }
            } else if entry.custom_fields.get("invalid_symlink").is_some() {
                let broken_indicator = console::style("(broken link)").red().bold();
                format!("{} -> {}", base_name, broken_indicator)
            } else {
                base_name
            }
        } else {
            base_name
        };

        if self.has_plugins_column || plugin_text.is_empty() {
            with_target
        } else {
            format!("{} {}", with_target, plugin_text)
        }
    }

    fn format_timestamp(&self, seconds: u64) -> String {
        if seconds == 0 {
            return "-".to_string();
        }
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds);
        if self.relative_dates {
            colorize_date_relative(&time).to_string()
        } else {
            colorize_date(&time).to_string()
        }
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

fn visible_width(value: &str) -> usize {
    let stripped = strip_ansi_escapes::strip(value).unwrap_or_default();
    let plain = String::from_utf8_lossy(&stripped);
    plain.width()
}

fn pad_left(value: &str, width: usize) -> String {
    let visible = visible_width(value);
    if visible >= width {
        value.to_string()
    } else {
        format!("{}{}", " ".repeat(width - visible), value)
    }
}

fn pad_right(value: &str, width: usize) -> String {
    let visible = visible_width(value);
    if visible >= width {
        value.to_string()
    } else {
        format!("{}{}", value, " ".repeat(width - visible))
    }
}
