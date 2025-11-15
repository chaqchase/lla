use crate::commands::args::{DiffCommand, DiffTarget};
use crate::error::{LlaError, Result};
use crate::theme;
use crate::utils::color::colorize_size;
use colored::Colorize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use unicode_width::UnicodeWidthStr;
use walkdir::WalkDir;

pub fn run(diff: DiffCommand) -> Result<()> {
    let DiffCommand { left, target } = diff;
    match target {
        DiffTarget::Directory(right) => diff_directories(&left, &right),
        DiffTarget::Git { reference } => diff_with_git(&left, &reference),
    }
}

fn diff_directories(left: &str, right: &str) -> Result<()> {
    let left_path = canonicalize_dir(left)?;
    let right_path = canonicalize_dir(right)?;
    let left_entries = collect_local_entries(&left_path)?;
    let right_entries = collect_local_entries(&right_path)?;

    render_diff(
        &left_path.display().to_string(),
        &right_path.display().to_string(),
        left_entries,
        right_entries,
    )
}

fn diff_with_git(left: &str, reference: &str) -> Result<()> {
    let left_path = canonicalize_dir(left)?;
    let left_entries = collect_local_entries(&left_path)?;
    let git_entries = collect_git_entries(&left_path, reference)?;

    render_diff(
        &left_path.display().to_string(),
        &format!("git:{}", reference),
        left_entries,
        git_entries,
    )
}

fn canonicalize_dir(path: &str) -> Result<PathBuf> {
    let pb = PathBuf::from(path);
    let canonical = pb.canonicalize().map_err(|_| {
        LlaError::Other(format!(
            "Directory '{}' does not exist or is not accessible",
            path
        ))
    })?;
    if !canonical.is_dir() {
        return Err(LlaError::Other(format!(
            "Path '{}' is not a directory",
            path
        )));
    }
    Ok(canonical)
}

fn collect_local_entries(root: &Path) -> Result<BTreeMap<String, u64>> {
    let mut entries = BTreeMap::new();

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|e| {
            LlaError::Other(format!(
                "Failed to read directory '{}': {}",
                root.display(),
                e
            ))
        })?;
        if entry.path().is_dir() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or_else(|_| entry.path());
        if rel.as_os_str().is_empty() {
            continue;
        }

        let metadata = entry
            .metadata()
            .or_else(|_| entry.path().symlink_metadata())
            .map_err(|e| {
                LlaError::Other(format!(
                    "Failed to read metadata for '{}': {}",
                    entry.path().display(),
                    e
                ))
            })?;

        let path_string = rel.to_string_lossy().replace('\\', "/");
        entries.insert(path_string, metadata.len());
    }

    Ok(entries)
}

fn collect_git_entries(root: &Path, reference: &str) -> Result<BTreeMap<String, u64>> {
    let repo_root = git_repo_root(root)?;
    let relative = root
        .strip_prefix(&repo_root)
        .map_err(|_| {
            LlaError::Other(format!(
                "Directory '{}' is outside the git repository",
                root.display()
            ))
        })?
        .to_path_buf();

    let relative_git_path = to_git_path(&relative);
    let filter_arg = if relative_git_path.is_empty() {
        ".".to_string()
    } else {
        relative_git_path.clone()
    };

    let output = Command::new("git")
        .arg("ls-tree")
        .arg("-r")
        .arg("--long")
        .arg(reference)
        .arg("--")
        .arg(&filter_arg)
        .current_dir(&repo_root)
        .output()?;

    if !output.status.success() {
        return Err(LlaError::Other(format!(
            "git ls-tree failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let mut entries = BTreeMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Some(tab_index) = line.find('\t') else {
            continue;
        };
        let meta = &line[..tab_index];
        let path_part = &line[tab_index + 1..];
        let mut meta_parts = meta.split_whitespace();
        let _mode = meta_parts.next();
        let kind = meta_parts.next().unwrap_or("");
        let _object = meta_parts.next();
        let size_str = meta_parts.next().unwrap_or("0");

        if kind != "blob" {
            continue;
        }

        let size = size_str.parse::<u64>().unwrap_or(0);
        if let Some(rel_path) = relativize_git_path(path_part, &relative_git_path) {
            if rel_path.is_empty() {
                continue;
            }
            entries.insert(rel_path, size);
        }
    }

    Ok(entries)
}

fn git_repo_root(path: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(path)
        .output()?;
    if !output.status.success() {
        return Err(LlaError::Other(format!(
            "'{}' is not inside a git repository",
            path.display()
        )));
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(root))
}

fn to_git_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn relativize_git_path(path: &str, prefix: &str) -> Option<String> {
    if prefix.is_empty() || prefix == "." {
        return Some(path.trim_start_matches("./").to_string());
    }

    if path == prefix {
        return None;
    }

    let normalized_prefix = prefix.trim_end_matches('/');
    let prefix_with_sep = format!("{}/", normalized_prefix);
    if let Some(stripped) = path.strip_prefix(&prefix_with_sep) {
        Some(stripped.to_string())
    } else {
        None
    }
}

fn render_diff(
    left_label: &str,
    right_label: &str,
    left: BTreeMap<String, u64>,
    right: BTreeMap<String, u64>,
) -> Result<()> {
    println!(
        "{}",
        format!("Comparing {} → {}", left_label, right_label).bold()
    );

    let rows = build_rows(left, right);
    if rows.is_empty() {
        println!("No differences found.");
        return Ok(());
    }

    let added = rows
        .iter()
        .filter(|r| r.status == DiffStatus::Added)
        .count();
    let removed = rows
        .iter()
        .filter(|r| r.status == DiffStatus::Removed)
        .count();
    let changed = rows
        .iter()
        .filter(|r| r.status == DiffStatus::Modified)
        .count();
    let total_delta: i64 = rows.iter().map(|r| r.delta()).sum();

    let summary = format!(
        "{} added, {} removed, {} changed (net {})",
        colorize_count(added, DiffStatus::Added),
        colorize_count(removed, DiffStatus::Removed),
        colorize_count(changed, DiffStatus::Modified),
        format_delta(total_delta)
    );

    println!("{}", summary);
    println!();

    print_table(&rows);
    Ok(())
}

fn build_rows(left: BTreeMap<String, u64>, right: BTreeMap<String, u64>) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let mut keys = BTreeSet::new();
    keys.extend(left.keys().cloned());
    keys.extend(right.keys().cloned());

    for key in keys {
        match (left.get(&key), right.get(&key)) {
            (Some(l_size), Some(r_size)) => {
                if l_size != r_size {
                    rows.push(DiffRow {
                        status: DiffStatus::Modified,
                        path: key,
                        left_size: Some(*l_size),
                        right_size: Some(*r_size),
                    });
                }
            }
            (Some(l_size), None) => rows.push(DiffRow {
                status: DiffStatus::Removed,
                path: key,
                left_size: Some(*l_size),
                right_size: None,
            }),
            (None, Some(r_size)) => rows.push(DiffRow {
                status: DiffStatus::Added,
                path: key,
                left_size: None,
                right_size: Some(*r_size),
            }),
            (None, None) => {}
        }
    }

    rows.sort_by(|a, b| {
        a.status
            .sort_key()
            .cmp(&b.status.sort_key())
            .then_with(|| a.path.cmp(&b.path))
    });
    rows
}

fn print_table(rows: &[DiffRow]) {
    let headers = vec![
        "Status".to_string(),
        "Path".to_string(),
        "Left".to_string(),
        "Right".to_string(),
        "Δ".to_string(),
    ];
    let mut widths: Vec<usize> = headers.iter().map(|h| visible_width(h)).collect();
    let mut rendered_rows = Vec::with_capacity(rows.len());

    for row in rows {
        let status = format_status(&row.status);
        let path = row.path.clone();
        let left = row
            .left_size
            .map(|s| colorize_size(s).to_string())
            .unwrap_or_else(|| "-".to_string());
        let right = row
            .right_size
            .map(|s| colorize_size(s).to_string())
            .unwrap_or_else(|| "-".to_string());
        let delta = format_delta(row.delta());

        update_width(&mut widths, 0, &status);
        update_width(&mut widths, 1, &path);
        update_width(&mut widths, 2, &left);
        update_width(&mut widths, 3, &right);
        update_width(&mut widths, 4, &delta);

        rendered_rows.push(vec![status, path, left, right, delta]);
    }

    let alignments = [false, false, true, true, true];
    println!("{}", build_row_line(&headers, &widths, &alignments));
    println!("{}", build_separator_line(&widths));
    for row in rendered_rows {
        println!("{}", build_row_line(&row, &widths, &alignments));
    }
}

fn build_row_line(values: &[String], widths: &[usize], align_right: &[bool]) -> String {
    let mut cells = Vec::new();
    for ((value, width), align) in values.iter().zip(widths).zip(align_right.iter()) {
        let padded = if *align {
            pad_left(value, *width)
        } else {
            pad_right(value, *width)
        };
        cells.push(padded);
    }
    cells.join("  ")
}

fn build_separator_line(widths: &[usize]) -> String {
    widths
        .iter()
        .map(|w| "─".repeat(*w))
        .collect::<Vec<_>>()
        .join("  ")
}

fn update_width(widths: &mut [usize], index: usize, content: &str) {
    let width = visible_width(content);
    if width > widths[index] {
        widths[index] = width;
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

fn pad_left(value: &str, width: usize) -> String {
    let visible = visible_width(value);
    if visible >= width {
        value.to_string()
    } else {
        format!("{}{}", " ".repeat(width - visible), value)
    }
}

fn visible_width(value: &str) -> usize {
    let stripped = strip_ansi_escapes::strip(value).unwrap_or_default();
    let plain = String::from_utf8_lossy(&stripped);
    plain.width()
}

fn format_status(status: &DiffStatus) -> String {
    let symbol = status.symbol();
    let symbol_str = symbol.to_string();
    if theme::is_no_color() {
        symbol_str
    } else {
        match status {
            DiffStatus::Added => symbol_str.green().bold().to_string(),
            DiffStatus::Removed => symbol_str.red().bold().to_string(),
            DiffStatus::Modified => symbol_str.yellow().bold().to_string(),
        }
    }
}

fn colorize_count(count: usize, status: DiffStatus) -> String {
    if theme::is_no_color() {
        count.to_string()
    } else {
        match status {
            DiffStatus::Added => count.to_string().green().bold().to_string(),
            DiffStatus::Removed => count.to_string().red().bold().to_string(),
            DiffStatus::Modified => count.to_string().yellow().bold().to_string(),
        }
    }
}

fn format_delta(delta: i64) -> String {
    if delta == 0 {
        return "0B".to_string();
    }
    let magnitude = human_size(delta.abs() as u64);
    let text = if delta > 0 {
        format!("+{}", magnitude)
    } else {
        format!("-{}", magnitude)
    };

    if theme::is_no_color() {
        text
    } else if delta > 0 {
        text.green().bold().to_string()
    } else {
        text.red().bold().to_string()
    }
}

fn human_size(size: u64) -> String {
    if size < 1024 {
        format!("{}B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1}K", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1}M", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}G", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffStatus {
    Added,
    Removed,
    Modified,
}

impl DiffStatus {
    fn symbol(&self) -> char {
        match self {
            DiffStatus::Added => '+',
            DiffStatus::Removed => '-',
            DiffStatus::Modified => '~',
        }
    }

    fn sort_key(&self) -> u8 {
        match self {
            DiffStatus::Added => 0,
            DiffStatus::Removed => 1,
            DiffStatus::Modified => 2,
        }
    }
}

struct DiffRow {
    status: DiffStatus,
    path: String,
    left_size: Option<u64>,
    right_size: Option<u64>,
}

impl DiffRow {
    fn delta(&self) -> i64 {
        let left = self.left_size.unwrap_or(0) as i64;
        let right = self.right_size.unwrap_or(0) as i64;
        right - left
    }
}
