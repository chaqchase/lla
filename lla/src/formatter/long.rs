use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::utils::color::*;
use crate::utils::icons::format_with_icon;
use console;
use lla_plugin_interface::proto::DecoratedEntry;
use once_cell::sync::Lazy;
use unicode_width::UnicodeWidthStr;

use std::collections::HashMap;
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
}

impl LongFormatter {
    pub fn new(
        show_icons: bool,
        permission_format: String,
        hide_group: bool,
        relative_dates: bool,
    ) -> Self {
        Self {
            show_icons,
            permission_format,
            hide_group,
            relative_dates,
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
        let min_size_len = 8;

        // Precompute max visible width for the date column so it aligns when using relative dates
        let max_date_len = files
            .iter()
            .map(|entry| {
                let metadata = entry.metadata.as_ref().cloned().unwrap_or_default();
                let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(metadata.modified);
                let date_colored = if self.relative_dates {
                    colorize_date_relative(&modified)
                } else {
                    colorize_date(&modified)
                };
                let date_str = date_colored.to_string();
                let stripped_bytes = strip_ansi_escapes::strip(&date_str).unwrap_or_default();
                let stripped = String::from_utf8_lossy(&stripped_bytes);
                stripped.width()
            })
            .max()
            .unwrap_or(0);

        let max_user_len = files
            .iter()
            .map(|entry| {
                let uid = entry.metadata.as_ref().map_or(0, |m| m.uid);
                let user = get_user_by_uid(uid)
                    .map(|u| u.name().to_string_lossy().into_owned())
                    .unwrap_or_else(|| uid.to_string());
                user.len()
            })
            .max()
            .unwrap_or(0);

        let max_group_len = if self.hide_group {
            0
        } else {
            files
                .iter()
                .map(|entry| {
                    let gid = entry.metadata.as_ref().map_or(0, |m| m.gid);
                    let group = get_group_by_gid(gid)
                        .map(|g| g.name().to_string_lossy().into_owned())
                        .unwrap_or_else(|| gid.to_string());
                    group.len()
                })
                .max()
                .unwrap_or(0)
        };

        let mut output = String::new();
        for entry in files {
            let metadata = entry.metadata.as_ref().cloned().unwrap_or_default();
            let size = colorize_size(metadata.size);
            let perms = permissions_from_mode(metadata.permissions);
            let permissions = colorize_permissions(&perms, Some(&self.permission_format));
            let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(metadata.modified);
            let modified_colored = if self.relative_dates {
                colorize_date_relative(&modified)
            } else {
                colorize_date(&modified)
            };
            // Left-align the date to the max visible width to match the existing layout
            let modified_uncolored = String::from_utf8_lossy(
                &strip_ansi_escapes::strip(&modified_colored.to_string()).unwrap_or_default(),
            )
            .to_string();
            let date_padding = max_date_len.saturating_sub(modified_uncolored.width());
            let modified_str = format!("{}{}", modified_colored, " ".repeat(date_padding));
            let path = Path::new(&entry.path);
            let colored_name = colorize_file_name(path).to_string();
            let name = colorize_file_name_with_icon(
                path,
                format_with_icon(path, colored_name, self.show_icons),
            )
            .to_string();

            let user = {
                #[cfg(unix)]
                {
                    let uid = metadata.uid;
                    let mut cache = USER_CACHE.lock().unwrap();
                    if let Some(cached_user) = cache.get(&uid) {
                        cached_user.clone()
                    } else {
                        let user = get_user_by_uid(uid)
                            .map(|u| u.name().to_string_lossy().into_owned())
                            .unwrap_or_else(|| uid.to_string());
                        cache.insert(uid, user.clone());
                        user
                    }
                }
                #[cfg(windows)]
                {
                    crate::windows_metadata::get_windows_owner(path).unwrap_or_else(|| "UNKNOWN".to_string())
                }
                #[cfg(not(any(unix, windows)))]
                {
                    "UNKNOWN".to_string()
                }
            };

            let group = if self.hide_group {
                String::new()
            } else {
                #[cfg(unix)]
                {
                    let gid = metadata.gid;
                    let mut cache = GROUP_CACHE.lock().unwrap();
                    if let Some(cached_group) = cache.get(&gid) {
                        cached_group.clone()
                    } else {
                        let group = get_group_by_gid(gid)
                            .map(|g| g.name().to_string_lossy().into_owned())
                            .unwrap_or_else(|| gid.to_string());
                        cache.insert(gid, group.clone());
                        group
                    }
                }
                #[cfg(windows)]
                {
                    crate::windows_metadata::get_windows_group(path).unwrap_or_else(|| "UNKNOWN".to_string())
                }
                #[cfg(not(any(unix, windows)))]
                {
                    "UNKNOWN".to_string()
                }
            };

            let plugin_fields = plugin_manager.format_fields(entry, "long").join(" ");
            let plugin_suffix = if plugin_fields.is_empty() {
                String::new()
            } else {
                format!(" {}", plugin_fields)
            };

            let name_with_target = if metadata.is_symlink {
                if let Some(target) = entry.custom_fields.get("symlink_target") {
                    if entry.custom_fields.get("invalid_symlink").is_some() {
                        let broken_target = console::style(target).red().bold();
                        format!("{} -> {} (broken)", name, broken_target)
                    } else {
                        format!("{} -> {}", name, colorize_symlink_target(Path::new(target)))
                    }
                } else if entry.custom_fields.get("invalid_symlink").is_some() {
                    let broken_indicator = console::style("(broken link)").red().bold();
                    format!("{} -> {}", name, broken_indicator)
                } else {
                    name
                }
            } else {
                name
            };

            if self.hide_group {
                output.push_str(&format!(
                    "{} {:>width_size$} {} {:<width_user$} {}{}\n",
                    permissions,
                    size,
                    modified_str,
                    colorize_user(&user),
                    name_with_target,
                    plugin_suffix,
                    width_size = min_size_len,
                    width_user = max_user_len
                ));
            } else {
                output.push_str(&format!(
                    "{} {:>width_size$} {} {:<width_user$} {:<width_group$} {}{}\n",
                    permissions,
                    size,
                    modified_str,
                    colorize_user(&user),
                    colorize_group(&group),
                    name_with_target,
                    plugin_suffix,
                    width_size = min_size_len,
                    width_user = max_user_len,
                    width_group = max_group_len
                ));
            }
        }
        if output.ends_with('\n') {
            output.pop();
        }
        Ok(output)
    }
}
