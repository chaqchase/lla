use crate::commands::args::{DiffCommand, DiffTarget};
use crate::error::{LlaError, Result};
use crate::theme;
use crate::utils::color::colorize_size;
use colored::Colorize;
use ignore::WalkBuilder;
use similar::TextDiff;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;
use unicode_width::UnicodeWidthStr;

pub fn run(diff: DiffCommand) -> Result<()> {
    let DiffCommand { left, target } = diff;
    let left_entry = resolve_path(&left)?;

    match target {
        DiffTarget::Directory(right) => {
            let right_entry = resolve_path(&right)?;
            match (left_entry.kind, right_entry.kind) {
                (PathKind::Directory, PathKind::Directory) => {
                    diff_directories(&left_entry.path, &right_entry.path)
                }
                (PathKind::File, PathKind::File) => diff_files(&left_entry.path, &right_entry.path),
                (PathKind::Directory, PathKind::File) => Err(LlaError::Other(format!(
                    "Cannot diff directory '{}' against file '{}'",
                    left_entry.path.display(),
                    right_entry.path.display()
                ))),
                (PathKind::File, PathKind::Directory) => Err(LlaError::Other(format!(
                    "Cannot diff file '{}' against directory '{}'",
                    left_entry.path.display(),
                    right_entry.path.display()
                ))),
            }
        }
        DiffTarget::Git { reference } => match left_entry.kind {
            PathKind::Directory => diff_directory_with_git(&left_entry.path, &reference),
            PathKind::File => diff_file_with_git(&left_entry.path, &reference),
        },
    }
}

struct ResolvedPath {
    path: PathBuf,
    kind: PathKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathKind {
    File,
    Directory,
}

fn resolve_path(path: &str) -> Result<ResolvedPath> {
    let pb = PathBuf::from(path);
    let canonical = pb.canonicalize().map_err(|_| {
        LlaError::Other(format!(
            "Path '{}' does not exist or is not accessible",
            path
        ))
    })?;
    let metadata = canonical.metadata().map_err(|e| {
        LlaError::Other(format!(
            "Failed to inspect '{}': {}",
            canonical.display(),
            e
        ))
    })?;

    let kind = if metadata.is_dir() {
        PathKind::Directory
    } else if metadata.is_file() {
        PathKind::File
    } else {
        return Err(LlaError::Other(format!(
            "Path '{}' is not a regular file or directory",
            path
        )));
    };

    Ok(ResolvedPath {
        path: canonical,
        kind,
    })
}

fn diff_directories(left: &Path, right: &Path) -> Result<()> {
    let left_entries = collect_local_entries(left)?;
    let right_entries = collect_local_entries(right)?;

    render_diff(
        &left.display().to_string(),
        &right.display().to_string(),
        left_entries,
        right_entries,
    )
}

fn diff_directory_with_git(left: &Path, reference: &str) -> Result<()> {
    let left_entries = collect_local_entries(left)?;
    let git_entries = collect_git_entries(left, reference)?;

    // When comparing against git we treat the git reference as the "left"/baseline
    // side so that additions/removals are reported from the perspective of the
    // working tree (i.e. files that exist locally but not in git show up as added).
    render_diff(
        &format!("git:{}", reference),
        &left.display().to_string(),
        git_entries,
        left_entries,
    )
}

fn diff_files(left: &Path, right: &Path) -> Result<()> {
    let left_bytes = read_file_bytes(left)?;
    let right_bytes = read_file_bytes(right)?;

    render_file_diff(
        &left.display().to_string(),
        &right.display().to_string(),
        &left_bytes,
        &right_bytes,
        None,
    )
}

fn diff_file_with_git(path: &Path, reference: &str) -> Result<()> {
    let repo_root = git_repo_root(path)?;
    verify_git_reference(&repo_root, reference)?;
    let relative = path.strip_prefix(&repo_root).map_err(|_| {
        LlaError::Other(format!(
            "File '{}' is outside the git repository",
            path.display()
        ))
    })?;
    let git_path = to_git_path(relative);
    let (baseline_bytes, missing_baseline) = match read_git_blob(&repo_root, reference, &git_path)?
    {
        Some(bytes) => (bytes, false),
        None => (Vec::new(), true),
    };
    let working_bytes = read_file_bytes(path)?;
    let note = missing_baseline.then_some(
        "Note: file does not exist in the selected git reference; treating it as newly added.",
    );

    render_file_diff(
        &format!("git:{}:{}", reference, git_path),
        &path.display().to_string(),
        &baseline_bytes,
        &working_bytes,
        note,
    )
}

fn read_file_bytes(path: &Path) -> Result<Vec<u8>> {
    fs::read(path)
        .map_err(|e| LlaError::Other(format!("Failed to read file '{}': {}", path.display(), e)))
}

fn read_git_blob(repo_root: &Path, reference: &str, git_path: &str) -> Result<Option<Vec<u8>>> {
    let spec = format!("{}:{}", reference, git_path);
    let ls_output = Command::new("git")
        .arg("ls-tree")
        .arg(reference)
        .arg("--")
        .arg(git_path)
        .current_dir(repo_root)
        .output()?;
    if !ls_output.status.success() {
        return Err(LlaError::Other(format!(
            "git ls-tree failed: {}",
            String::from_utf8_lossy(&ls_output.stderr).trim()
        )));
    }
    if ls_output.stdout.is_empty() {
        return Ok(None);
    }

    let output = Command::new("git")
        .arg("show")
        .arg(&spec)
        .current_dir(repo_root)
        .output()?;
    if !output.status.success() {
        return Err(LlaError::Other(format!(
            "git show failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(Some(output.stdout))
}

fn verify_git_reference(repo_root: &Path, reference: &str) -> Result<()> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--verify")
        .arg(reference)
        .current_dir(repo_root)
        .output()?;
    if !output.status.success() {
        return Err(LlaError::Other(format!(
            "Git reference '{}' is invalid: {}",
            reference,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn render_file_diff(
    left_label: &str,
    right_label: &str,
    left_bytes: &[u8],
    right_bytes: &[u8],
    note: Option<&str>,
) -> Result<()> {
    println!(
        "{}",
        format!("Comparing {} → {}", left_label, right_label).bold()
    );
    if let Some(note) = note {
        println!("{}", note.italic());
    }

    if left_bytes == right_bytes {
        println!("No differences found.");
        return Ok(());
    }

    let left_size = left_bytes.len() as u64;
    let right_size = right_bytes.len() as u64;
    let size_delta = right_size as i64 - left_size as i64;

    println!("{}", "Summary:".bold());
    println!(
        "  Size     {} → {}   {}",
        colorize_size(left_size),
        colorize_size(right_size),
        format_delta_with_percent(size_delta, Some(left_size), Some(right_size))
    );

    match (str::from_utf8(left_bytes), str::from_utf8(right_bytes)) {
        (Ok(left_text), Ok(right_text)) => {
            let left_lines = count_lines(left_text);
            let right_lines = count_lines(right_text);
            let line_delta = right_lines as i64 - left_lines as i64;
            println!(
                "  Lines    {} → {}   {}",
                left_lines,
                right_lines,
                format_line_delta(line_delta)
            );
            println!();
            print_text_diff(left_label, right_label, left_text, right_text);
        }
        _ => {
            println!("  Content  Binary data (diff not shown)");
            println!();
            println!("Binary files differ.");
        }
    }

    Ok(())
}

fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

fn format_line_delta(delta: i64) -> String {
    if delta == 0 {
        "0".to_string()
    } else if theme::is_no_color() {
        format!("{:+}", delta)
    } else if delta > 0 {
        format!("+{}", delta).green().bold().to_string()
    } else {
        delta.to_string().red().bold().to_string()
    }
}

fn print_text_diff(left_label: &str, right_label: &str, left_text: &str, right_text: &str) {
    let diff = TextDiff::from_lines(left_text, right_text);
    let diff_text = diff
        .unified_diff()
        .context_radius(3)
        .header(left_label, right_label)
        .to_string();

    if theme::is_no_color() {
        print!("{}", diff_text);
        if !diff_text.ends_with('\n') {
            println!();
        }
        return;
    }

    for line in diff_text.lines() {
        let styled = if line.starts_with('+') && !line.starts_with("+++") {
            line.green().to_string()
        } else if line.starts_with('-') && !line.starts_with("---") {
            line.red().to_string()
        } else if line.starts_with("@@") {
            line.cyan().bold().to_string()
        } else if line.starts_with("+++") || line.starts_with("---") {
            line.bold().to_string()
        } else {
            line.to_string()
        };
        println!("{}", styled);
    }
    if !diff_text.ends_with('\n') {
        println!();
    }
}

fn collect_local_entries(root: &Path) -> Result<BTreeMap<String, u64>> {
    let mut entries = BTreeMap::new();

    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .parents(true);

    for dent in builder.build() {
        let entry = dent.map_err(|e| {
            LlaError::Other(format!(
                "Failed to read directory '{}': {}",
                root.display(),
                e
            ))
        })?;

        let path = entry.path();
        if path == root {
            continue;
        }

        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }

        if is_git_internal(path, root) {
            continue;
        }

        let rel = path.strip_prefix(root).unwrap_or(path);
        if rel.as_os_str().is_empty() {
            continue;
        }

        let metadata = entry
            .metadata()
            .or_else(|_| path.symlink_metadata())
            .map_err(|e| {
                LlaError::Other(format!(
                    "Failed to read metadata for '{}': {}",
                    path.display(),
                    e
                ))
            })?;

        let path_string = rel.to_string_lossy().replace('\\', "/");
        entries.insert(path_string, metadata.len());
    }

    Ok(entries)
}

fn is_git_internal(path: &Path, root: &Path) -> bool {
    match path.strip_prefix(root) {
        Ok(relative) => relative
            .components()
            .any(|component| component.as_os_str() == ".git"),
        Err(_) => false,
    }
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
    let cd_target = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or_else(|| Path::new("/"))
    };

    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(cd_target)
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

    let stats = calculate_stats(&rows);
    print_summary(&stats);
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
        let delta = format_delta_with_percent(row.delta(), row.left_size, row.right_size);

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

fn format_delta_with_percent(
    delta: i64,
    left_size: Option<u64>,
    right_size: Option<u64>,
) -> String {
    if delta == 0 {
        return "0B".to_string();
    }
    let magnitude = human_size(delta.abs() as u64);
    let percent = match (left_size, right_size) {
        (Some(left), Some(right)) if left > 0 => {
            let pct = ((right as f64 - left as f64) / left as f64) * 100.0;
            Some(pct)
        }
        _ => None,
    };
    let percent_str = percent
        .map(|pct| format!(" ({:+.1}%)", pct))
        .unwrap_or_default();

    let text = if delta > 0 {
        format!("+{}{}", magnitude, percent_str)
    } else {
        format!("-{}{}", magnitude, percent_str)
    };

    if theme::is_no_color() {
        text
    } else if delta > 0 {
        text.green().bold().to_string()
    } else {
        text.red().bold().to_string()
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

struct DiffStats {
    added_files: usize,
    removed_files: usize,
    modified_files: usize,
    added_bytes: u64,
    removed_bytes: u64,
    net_bytes: i64,
    largest_growth: Option<(String, i64)>,
    largest_shrink: Option<(String, i64)>,
}

fn calculate_stats(rows: &[DiffRow]) -> DiffStats {
    let mut stats = DiffStats {
        added_files: 0,
        removed_files: 0,
        modified_files: 0,
        added_bytes: 0,
        removed_bytes: 0,
        net_bytes: 0,
        largest_growth: None,
        largest_shrink: None,
    };

    for row in rows {
        let delta = row.delta();
        stats.net_bytes += delta;

        match row.status {
            DiffStatus::Added => {
                stats.added_files += 1;
                stats.added_bytes += row.right_size.unwrap_or(0);
                if stats
                    .largest_growth
                    .as_ref()
                    .map_or(true, |(_, d)| delta > *d)
                {
                    stats.largest_growth = Some((row.path.clone(), delta));
                }
            }
            DiffStatus::Removed => {
                stats.removed_files += 1;
                stats.removed_bytes += row.left_size.unwrap_or(0);
                if stats
                    .largest_shrink
                    .as_ref()
                    .map_or(true, |(_, d)| delta < *d)
                {
                    stats.largest_shrink = Some((row.path.clone(), delta));
                }
            }
            DiffStatus::Modified => {
                stats.modified_files += 1;
                if delta > 0
                    && stats
                        .largest_growth
                        .as_ref()
                        .map_or(true, |(_, d)| delta > *d)
                {
                    stats.largest_growth = Some((row.path.clone(), delta));
                }
                if delta < 0
                    && stats
                        .largest_shrink
                        .as_ref()
                        .map_or(true, |(_, d)| delta < *d)
                {
                    stats.largest_shrink = Some((row.path.clone(), delta));
                }
            }
        }
    }

    stats
}

fn print_summary(stats: &DiffStats) {
    println!("{}", "Summary:".bold());
    println!(
        "  Files    {} added, {} removed, {} changed",
        colorize_count(stats.added_files, DiffStatus::Added),
        colorize_count(stats.removed_files, DiffStatus::Removed),
        colorize_count(stats.modified_files, DiffStatus::Modified)
    );
    println!(
        "  Sizes    {} added, {} removed",
        colorize_size(stats.added_bytes).to_string().green(),
        colorize_size(stats.removed_bytes).to_string().red()
    );
    println!("  Net      {}", format_delta(stats.net_bytes));

    if let Some((path, delta)) = &stats.largest_growth {
        println!("  Largest+ {} {}", format_delta(*delta), path.cyan());
    }
    if let Some((path, delta)) = &stats.largest_shrink {
        println!("  Largest- {} {}", format_delta(*delta), path.cyan());
    }
    println!();
}
