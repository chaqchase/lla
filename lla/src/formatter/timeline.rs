use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::theme::{self, ColorValue};
use crate::utils::color::{self, colorize_file_name, colorize_file_name_with_icon};
use crate::utils::hyperlink;
use crate::utils::icons::format_with_icon;
use chrono::{DateTime, Duration, Local};
use colored::*;
use lla_plugin_interface::proto::DecoratedEntry;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

pub struct TimelineFormatter {
    pub show_icons: bool,
}

impl TimelineFormatter {
    pub fn new(show_icons: bool) -> Self {
        Self { show_icons }
    }

    fn format_relative_time(dt: DateTime<Local>) -> String {
        let now = Local::now();
        let duration = now.signed_duration_since(dt);

        if duration.num_seconds() < 60 {
            "just now".to_string()
        } else if duration.num_minutes() < 60 {
            format!("{} mins ago", duration.num_minutes())
        } else if duration.num_hours() < 24 {
            format!("{} hours ago", duration.num_hours())
        } else if duration.num_days() < 7 {
            format!("{} days ago", duration.num_days())
        } else if duration.num_days() < 30 {
            format!("{} weeks ago", duration.num_weeks())
        } else if duration.num_days() < 365 {
            dt.format("%b %d").to_string()
        } else {
            dt.format("%b %d, %Y").to_string()
        }
    }

    fn get_header_color() -> Color {
        let theme = color::get_theme();
        theme::color_value_to_color(&theme.colors.directory)
    }

    fn get_separator_color() -> Color {
        theme::color_value_to_color(&ColorValue::Named("bright black".to_string()))
    }

    fn get_time_color() -> Color {
        let theme = color::get_theme();
        theme::color_value_to_color(&theme.colors.date)
    }

    fn get_commit_color() -> Color {
        let theme = color::get_theme();
        theme::color_value_to_color(&theme.colors.symlink)
    }
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
enum TimeGroup {
    Today,
    Yesterday,
    LastWeek,
    LastMonth,
    Older,
}

impl TimeGroup {
    fn from_datetime(dt: DateTime<Local>) -> Self {
        let now = Local::now();
        let today = now.date_naive();
        let yesterday = today - Duration::days(1);
        let last_week = today - Duration::days(7);
        let last_month = today - Duration::days(30);

        let file_date = dt.date_naive();

        if file_date == today {
            TimeGroup::Today
        } else if file_date == yesterday {
            TimeGroup::Yesterday
        } else if file_date > last_week {
            TimeGroup::LastWeek
        } else if file_date > last_month {
            TimeGroup::LastMonth
        } else {
            TimeGroup::Older
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            TimeGroup::Today => "Today",
            TimeGroup::Yesterday => "Yesterday",
            TimeGroup::LastWeek => "Last Week",
            TimeGroup::LastMonth => "Last Month",
            TimeGroup::Older => "Older",
        }
    }
}

impl FileFormatter for TimelineFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        _depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() {
            return Ok(String::new());
        }
        plugin_manager.prepare_format_fields(files, "timeline");

        let mut groups: BTreeMap<TimeGroup, Vec<&DecoratedEntry>> = BTreeMap::new();

        for file in files {
            let modified = file.metadata.as_ref().map_or(0, |m| m.modified);
            let modified = UNIX_EPOCH + std::time::Duration::from_secs(modified);
            let dt = DateTime::<Local>::from(modified);
            let group = TimeGroup::from_datetime(dt);
            groups.entry(group).or_default().push(file);
        }

        let mut output = String::new();

        for (group, entries) in groups {
            output.push_str(&format!(
                "\n{}\n{}\n",
                group.display_name().color(Self::get_header_color()).bold(),
                "─".repeat(40).color(Self::get_separator_color())
            ));

            for entry in entries {
                let modified = entry.metadata.as_ref().map_or(0, |m| m.modified);
                let modified = UNIX_EPOCH + std::time::Duration::from_secs(modified);
                let dt = DateTime::<Local>::from(modified);

                let time_str = Self::format_relative_time(dt).color(Self::get_time_color());

                let path = Path::new(&entry.path);
                let colored_name = colorize_file_name(path).to_string();
                let name = colorize_file_name_with_icon(
                    path,
                    format_with_icon(path, colored_name, self.show_icons),
                )
                .to_string();
                let name = hyperlink::link_path(path, name);

                let plugin_fields = plugin_manager.format_fields(entry, "timeline").join(" ");
                let plugin_info = if plugin_fields.is_empty() {
                    String::new()
                } else {
                    format!(" {}", plugin_fields.color(Self::get_commit_color()))
                };

                output.push_str(&format!("{} • {}{}\n", name, time_str, plugin_info));
            }
            output.push('\n');
        }

        Ok(output)
    }
}
