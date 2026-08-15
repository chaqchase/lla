use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::theme::{self, ColorValue};
use crate::utils::color::{self, colorize_file_name, colorize_file_name_with_icon};
use crate::utils::icons::format_with_icon;
use colored::*;
use lla_plugin_interface::proto::DecoratedEntry;
use std::path::Path;
use terminal_size::{terminal_size, Width};
use unicode_width::UnicodeWidthStr;

pub struct SizeMapFormatter {
    pub show_icons: bool,
}

impl SizeMapFormatter {
    pub fn new(show_icons: bool) -> Self {
        Self { show_icons }
    }

    fn strip_ansi(s: &str) -> String {
        String::from_utf8(strip_ansi_escapes::strip(s).unwrap_or_default()).unwrap_or_default()
    }

    fn visible_width(s: &str) -> usize {
        Self::strip_ansi(s).width()
    }

    fn calculate_layout(
        files: &[DecoratedEntry],
        total_size: u64,
        term_width: usize,
    ) -> (usize, usize, usize) {
        let max_name_width = (term_width as f64 * 0.3) as usize;
        let name_width = files
            .iter()
            .map(|f| {
                let path = Path::new(&f.path);
                let colored_name = colorize_file_name(path).to_string();
                let name_with_icon =
                    colorize_file_name_with_icon(path, format_with_icon(path, colored_name, true));
                Self::visible_width(&name_with_icon)
            })
            .max()
            .unwrap_or(20)
            .clamp(20, max_name_width.min(60));

        let size_width = files
            .iter()
            .map(|f| {
                let size = f.metadata.as_ref().map_or(0, |m| m.size);
                Self::visible_width(&format_size(size))
            })
            .chain(std::iter::once(Self::visible_width(&format_size(
                total_size,
            ))))
            .max()
            .unwrap_or(8)
            .max(8);

        let spacing = 6;
        let bar_width = term_width
            .saturating_sub(name_width + size_width + spacing)
            .max(20);

        (name_width, size_width, bar_width)
    }

    fn create_bar(percentage: f64, width: usize, is_dir: bool) -> String {
        let theme = color::get_theme();
        let percent_width = 6;
        let bar_width = width.saturating_sub(percent_width);
        let filled_width = ((percentage / 100.0) * bar_width as f64) as usize;

        let (bar_char, partial_char) = if is_dir {
            let color = theme::color_value_to_color(&theme.colors.directory);
            ("█".color(color), "▓".color(color))
        } else if percentage > 75.0 {
            let color = theme::color_value_to_color(&theme.colors.permission_write);
            ("█".color(color), "▓".color(color))
        } else if percentage > 50.0 {
            let color = theme::color_value_to_color(&theme.colors.executable);
            ("█".color(color), "▓".color(color))
        } else if percentage > 25.0 {
            let color = theme::color_value_to_color(&theme.colors.symlink);
            ("█".color(color), "▓".color(color))
        } else {
            let color = theme::color_value_to_color(&theme.colors.file);
            ("█".color(color), "▓".color(color))
        };

        let empty = "⋅".color(theme::color_value_to_color(&ColorValue::Named(
            "bright black".to_string(),
        )));

        let filled = if filled_width > 0 {
            bar_char.to_string().repeat(filled_width - 1)
        } else {
            String::new()
        };
        let partial = if filled_width > 0 {
            partial_char.to_string()
        } else {
            String::new()
        };
        let empty = empty
            .to_string()
            .repeat(bar_width.saturating_sub(filled_width));

        format!("{}{}{} {:>4.1}%", filled, partial, empty, percentage)
    }

    fn truncate_name(name: &str, max_width: usize) -> String {
        if Self::visible_width(name) <= max_width {
            return name.to_string();
        }

        let mut result = String::new();
        let mut current_width = 0;
        let mut in_ansi = false;
        let mut ansi_code = String::new();

        for c in name.chars() {
            if c == '\x1B' {
                in_ansi = true;
                ansi_code.push(c);
                continue;
            }

            if in_ansi {
                ansi_code.push(c);
                if c == 'm' {
                    in_ansi = false;
                    result.push_str(&ansi_code);
                    ansi_code.clear();
                }
                continue;
            }

            let char_width = c.to_string().width();
            if current_width + char_width + 3 <= max_width {
                result.push(c);
                current_width += char_width;
            } else {
                break;
            }
        }

        result.push_str("...");
        if !ansi_code.is_empty() {
            result.push_str("\x1B[0m");
        }

        result
    }

    fn format_entry(
        name: &str,
        size: &str,
        bar: &str,
        name_width: usize,
        size_width: usize,
    ) -> String {
        let truncated_name = Self::truncate_name(name, name_width);
        let visible_name_width = Self::visible_width(&truncated_name);
        let padding = " ".repeat(name_width.saturating_sub(visible_name_width));

        format!(
            "  {}{} {:>size_width$}  {}",
            truncated_name,
            padding,
            size,
            bar,
            size_width = size_width
        )
    }

    fn format_total_entry(total_size: u64, name_width: usize, size_width: usize) -> String {
        let theme = color::get_theme();
        let label = "Total"
            .color(theme::color_value_to_color(&theme.colors.size))
            .bold()
            .to_string();

        Self::format_entry(&label, &format_size(total_size), "", name_width, size_width)
    }
}

impl FileFormatter for SizeMapFormatter {
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
            .unwrap_or(100);

        let total_size: u64 = files
            .iter()
            .map(|f| f.metadata.as_ref().map_or(0, |m| m.size))
            .sum();

        let (name_width, size_width, bar_width) =
            Self::calculate_layout(files, total_size, term_width);

        let mut output = String::new();
        output.push('\n');

        let mut files = files.to_vec();
        files.sort_by_key(|f| std::cmp::Reverse(f.metadata.as_ref().map_or(0, |m| m.size)));

        for file in &files {
            let path = Path::new(&file.path);
            let colored_name = colorize_file_name(path).to_string();
            let name = format_with_icon(path, colored_name, self.show_icons);
            let metadata = file.metadata.as_ref().cloned().unwrap_or_default();
            let size = metadata.size;
            let size_str = format_size(size);
            let percentage = if total_size > 0 {
                (size as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };

            let bar = Self::create_bar(percentage, bar_width, metadata.is_dir);

            let plugin_fields = plugin_manager.format_fields(file, "sizemap").join(" ");
            let plugin_suffix = if plugin_fields.is_empty() {
                String::new()
            } else {
                format!(" {}", plugin_fields)
            };

            output.push_str(&Self::format_entry(
                &name,
                &size_str,
                &format!("{}{}", bar, plugin_suffix),
                name_width,
                size_width,
            ));
            output.push('\n');
        }

        output.push_str(&Self::format_total_entry(
            total_size, name_width, size_width,
        ));
        output.push('\n');
        output.push('\n');
        Ok(output)
    }
}

fn format_size(size: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if size == 0 {
        return "0 B".to_string();
    }

    let size = size as f64;
    let base = 1024_f64;

    let exp = if size > 0.0 {
        (size.ln() / base.ln()).floor() as i32
    } else {
        0
    };

    let exp = if exp < 0 {
        0
    } else if exp >= UNITS.len() as i32 {
        (UNITS.len() - 1) as i32
    } else {
        exp
    };

    let size = size / base.powi(exp);
    if exp == 0 {
        format!("{:.0} {}", size, UNITS[exp as usize])
    } else {
        format!("{:.1} {}", size, UNITS[exp as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_entry_includes_summed_size() {
        let line = SizeMapFormatter::format_total_entry(1536, 20, 8);
        let plain = String::from_utf8_lossy(&strip_ansi_escapes::strip(&line).unwrap_or_default())
            .into_owned();

        assert!(plain.contains("Total"));
        assert!(plain.contains("1.5 KB"));
    }

    #[test]
    fn layout_size_width_accounts_for_total() {
        let files = vec![
            DecoratedEntry {
                path: "a".to_string(),
                metadata: Some(lla_plugin_interface::proto::EntryMetadata {
                    size: 900,
                    modified: 0,
                    accessed: 0,
                    created: 0,
                    is_dir: false,
                    is_file: true,
                    is_symlink: false,
                    permissions: 0,
                    uid: 0,
                    gid: 0,
                }),
                custom_fields: Default::default(),
                typed_fields: Default::default(),
            },
            DecoratedEntry {
                path: "b".to_string(),
                metadata: Some(lla_plugin_interface::proto::EntryMetadata {
                    size: 900,
                    modified: 0,
                    accessed: 0,
                    created: 0,
                    is_dir: false,
                    is_file: true,
                    is_symlink: false,
                    permissions: 0,
                    uid: 0,
                    gid: 0,
                }),
                custom_fields: Default::default(),
                typed_fields: Default::default(),
            },
        ];

        let (_, size_width, _) = SizeMapFormatter::calculate_layout(&files, 1800, 80);

        assert!(size_width >= SizeMapFormatter::visible_width("1.8 KB"));
    }
}
