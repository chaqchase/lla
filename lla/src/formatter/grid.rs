use super::{render_entry_name, FileFormatter};
use crate::error::Result;
use crate::plugin::PluginManager;
use lla_plugin_interface::proto::DecoratedEntry;
use terminal_size::{terminal_size, Width};
use unicode_width::UnicodeWidthStr;

pub struct GridFormatter {
    pub show_icons: bool,
    pub grid_ignore: bool,
    pub max_width: usize,
}

impl GridFormatter {
    pub fn new(show_icons: bool, grid_ignore: bool, max_width: usize) -> Self {
        Self {
            show_icons,
            grid_ignore,
            max_width,
        }
    }
}
impl FileFormatter for GridFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        _depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() {
            return Ok(String::new());
        }
        plugin_manager.prepare_format_fields(files, "grid");

        let term_width = if self.grid_ignore {
            self.max_width
        } else {
            terminal_size()
                .map(|(Width(w), _)| w as usize)
                .unwrap_or(80)
        };

        let mut entries = Vec::with_capacity(files.len());
        let mut widths = Vec::with_capacity(files.len());
        for file in files {
            let name_with_icon = render_entry_name(file, self.show_icons);
            let plugin_fields = plugin_manager.format_fields(file, "grid").join(" ");
            let entry = if plugin_fields.is_empty() {
                name_with_icon
            } else {
                format!("{} {}", name_with_icon, plugin_fields)
            };
            widths.push(visible_width(&entry));
            entries.push(entry);
        }

        let (rows, column_widths) = fit_columns(&widths, term_width);
        let mut output = String::new();
        for row in 0..rows {
            if row > 0 {
                output.push('\n');
            }
            for (column, &column_width) in column_widths.iter().enumerate() {
                let index = column * rows + row;
                let Some(entry) = entries.get(index) else {
                    break;
                };
                output.push_str(entry);
                // Pad unless this is the last entry printed on the row.
                if column + 1 < column_widths.len() && index + rows < entries.len() {
                    output.push_str(&" ".repeat(column_width - widths[index] + COLUMN_GAP));
                }
            }
        }

        Ok(output)
    }
}

const COLUMN_GAP: usize = 2;

fn visible_width(text: &str) -> usize {
    String::from_utf8_lossy(&strip_ansi_escapes::strip(text).unwrap_or_default()).width()
}

/// Lays entries out column-major like `ls`, giving each column its own width,
/// and returns the row count and column widths of the narrowest layout that
/// fits `term_width`.
fn fit_columns(widths: &[usize], term_width: usize) -> (usize, Vec<usize>) {
    let count = widths.len();
    if count == 0 {
        return (0, Vec::new());
    }
    let narrowest = widths.iter().copied().min().unwrap_or(1).max(1);
    let max_columns = (term_width / (narrowest + COLUMN_GAP)).clamp(1, count);
    let min_rows = count.div_ceil(max_columns);

    for rows in min_rows..=count {
        let column_widths: Vec<usize> = widths
            .chunks(rows)
            .map(|column| column.iter().copied().max().unwrap_or(0))
            .collect();
        let total = column_widths.iter().sum::<usize>()
            + COLUMN_GAP * column_widths.len().saturating_sub(1);
        if total <= term_width || column_widths.len() == 1 {
            return (rows, column_widths);
        }
    }
    (count, vec![widths.iter().copied().max().unwrap_or(0)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_long_name_does_not_force_a_single_column() {
        let mut widths = vec![4; 9];
        widths.push(30);
        let (rows, columns) = fit_columns(&widths, 50);
        assert!(columns.len() > 1, "got {rows} rows");
        let total: usize = columns.iter().sum::<usize>() + COLUMN_GAP * (columns.len() - 1);
        assert!(total <= 50);
    }

    #[test]
    fn falls_back_to_one_column_when_nothing_fits() {
        assert_eq!(fit_columns(&[90, 90], 50), (2, vec![90]));
    }
}
