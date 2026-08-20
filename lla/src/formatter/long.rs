use super::column_config::ColumnKey;
use super::FileFormatter;
use crate::config::DEFAULT_LONG_DATE_FORMAT;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::utils::color::*;
use crate::utils::icons::format_with_icon;
use crate::utils::{fs_metadata, hyperlink};
use chrono::format::{Item, StrftimeItems};
use chrono::{DateTime, Local};
use lla_plugin_interface::proto::{DecoratedEntry, EntryMetadata};
#[cfg(unix)]
use once_cell::sync::Lazy;
use unicode_width::UnicodeWidthStr;

#[cfg(unix)]
use std::collections::HashMap;
use std::path::Path;
#[cfg(unix)]
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
#[cfg(unix)]
use users::{get_group_by_gid, get_user_by_uid};

#[cfg(unix)]
static USER_CACHE: Lazy<Mutex<HashMap<u32, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));
#[cfg(unix)]
static GROUP_CACHE: Lazy<Mutex<HashMap<u32, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub struct LongFormatter {
    pub show_icons: bool,
    pub permission_format: String,
    pub hide_group: bool,
    pub relative_dates: bool,
    date_format_items: Vec<Item<'static>>,
    columns: Vec<ColumnKey>,
    has_plugins_column: bool,
}

impl LongFormatter {
    pub fn new(
        show_icons: bool,
        permission_format: String,
        hide_group: bool,
        relative_dates: bool,
        date_format: String,
        columns: Vec<ColumnKey>,
    ) -> Self {
        let date_format_items = compile_date_format(&date_format);
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
            date_format_items,
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
        plugin_manager.prepare_format_fields(files, "long");

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
                let mut rendered =
                    colorize_permissions(metadata.permissions, Some(&self.permission_format));
                if metadata.has_acl {
                    rendered.push('+');
                }
                rendered
            }
            ColumnKey::Inode => fs_metadata::format_inode(metadata),
            ColumnKey::HardLinks => fs_metadata::format_hard_links(metadata),
            ColumnKey::Size => colorize_size(metadata.size).to_string(),
            ColumnKey::AllocatedSize => fs_metadata::allocated_size(metadata)
                .map(|size| colorize_size(size).to_string())
                .unwrap_or_else(|| "-".to_string()),
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
            ColumnKey::Xattrs => fs_metadata::format_xattrs(metadata),
            ColumnKey::Context => fs_metadata::format_context(metadata),
            ColumnKey::Mount => fs_metadata::format_mount(metadata),
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
        let base_name = hyperlink::link_path(path, base_name);

        let with_target =
            if metadata.is_symlink && !entry.custom_fields.contains_key("hide_symlink_target") {
                if let Some(target) = entry.custom_fields.get("symlink_target") {
                    if entry.custom_fields.contains_key("invalid_symlink") {
                        let broken_target = console::style(target).red().bold();
                        format!("{} -> {} (broken)", base_name, broken_target)
                    } else {
                        let target_path = Path::new(target);
                        let resolved_target = if target_path.is_absolute() {
                            target_path.to_path_buf()
                        } else {
                            path.parent()
                                .unwrap_or_else(|| Path::new("."))
                                .join(target_path)
                        };
                        let target_label = colorize_symlink_target(target_path).to_string();
                        let target_label = hyperlink::link_path(&resolved_target, target_label);
                        format!("{} -> {}", base_name, target_label)
                    }
                } else if entry.custom_fields.contains_key("invalid_symlink") {
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
            let datetime: DateTime<Local> = time.into();
            let formatted = datetime
                .format_with_items(self.date_format_items.iter())
                .to_string();
            colorize_date_text(formatted).to_string()
        }
    }

    #[cfg(test)]
    fn set_date_format(&mut self, format: &str) {
        self.date_format_items = compile_date_format(format);
    }
}

fn compile_date_format(format: &str) -> Vec<Item<'static>> {
    StrftimeItems::new(format)
        .parse_to_owned()
        .unwrap_or_else(|_| {
            StrftimeItems::new(DEFAULT_LONG_DATE_FORMAT)
                .parse_to_owned()
                .expect("default date format must be valid")
        })
}

impl Default for LongFormatter {
    fn default() -> Self {
        Self::new(
            false,
            "symbolic".to_string(),
            false,
            false,
            DEFAULT_LONG_DATE_FORMAT.to_string(),
            vec![ColumnKey::Name],
        )
    }
}

#[cfg(unix)]
fn lookup_user(uid: u32) -> String {
    let resolved = || {
        get_user_by_uid(uid)
            .map(|u| u.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| uid.to_string())
    };

    let Ok(mut cache) = USER_CACHE.lock() else {
        return resolved();
    };
    if let Some(cached) = cache.get(&uid) {
        return cached.clone();
    }
    let resolved = resolved();
    cache.insert(uid, resolved.clone());
    resolved
}

#[cfg(windows)]
fn lookup_user(_uid: u32) -> String {
    "-".to_string()
}

#[cfg(unix)]
fn lookup_group(gid: u32) -> String {
    let resolved = || {
        get_group_by_gid(gid)
            .map(|g| g.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| gid.to_string())
    };

    let Ok(mut cache) = GROUP_CACHE.lock() else {
        return resolved();
    };
    if let Some(cached) = cache.get(&gid) {
        return cached.clone();
    }
    let resolved = resolved();
    cache.insert(gid, resolved.clone());
    resolved
}

#[cfg(windows)]
fn lookup_group(_gid: u32) -> String {
    "-".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Local};

    fn plain(value: &str) -> String {
        let stripped = strip_ansi_escapes::strip(value).unwrap_or_default();
        String::from_utf8_lossy(&stripped).into_owned()
    }

    fn expected(seconds: u64, format: &str) -> String {
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds);
        let datetime: DateTime<Local> = time.into();
        datetime.format(format).to_string()
    }

    #[test]
    fn default_timestamp_format_is_unchanged() {
        let formatter = LongFormatter::default();
        let seconds = 1_700_000_000;

        assert_eq!(
            plain(&formatter.format_timestamp(seconds)),
            expected(seconds, DEFAULT_LONG_DATE_FORMAT)
        );
    }

    #[test]
    fn custom_timestamp_format_can_include_year() {
        let mut formatter = LongFormatter::default();
        formatter.set_date_format("%Y-%m-%d %H:%M");
        let seconds = 1_700_000_000;

        assert_eq!(
            plain(&formatter.format_timestamp(seconds)),
            expected(seconds, "%Y-%m-%d %H:%M")
        );
    }

    #[test]
    fn relative_dates_take_precedence_over_custom_format() {
        let mut formatter = LongFormatter {
            relative_dates: true,
            ..Default::default()
        };
        formatter.set_date_format("%Y-%m-%d %H:%M");
        let seconds = 1_700_000_000;

        let rendered = plain(&formatter.format_timestamp(seconds));

        assert_ne!(rendered, expected(seconds, "%Y-%m-%d %H:%M"));
        assert!(
            rendered.contains("ago") || rendered.contains("from now") || rendered == "now",
            "unexpected relative date output: {}",
            rendered
        );
    }

    #[test]
    fn zero_timestamp_renders_placeholder() {
        let mut formatter = LongFormatter::default();
        formatter.set_date_format("%Y-%m-%d %H:%M");

        assert_eq!(formatter.format_timestamp(0), "-");
    }
}
