use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::theme::{self, ColorValue};
use crate::utils::color::{self, colorize_file_name, colorize_file_name_with_icon};
use crate::utils::icons::format_with_icon;
use colored::*;
use lla_plugin_interface::proto::DecoratedEntry;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use terminal_size::{terminal_size, Width};
use unicode_width::UnicodeWidthStr;

const SUBJECT_MAX_LENGTH: usize = 48;
const SUBJECT_MIN_LENGTH: usize = 16;

pub struct GitFormatter {
    pub show_icons: bool,
}

impl GitFormatter {
    pub fn new(show_icons: bool) -> Self {
        Self { show_icons }
    }

    fn strip_ansi(s: &str) -> String {
        String::from_utf8(strip_ansi_escapes::strip(s).unwrap_or_default()).unwrap_or_default()
    }

    fn get_theme_color(value: &ColorValue) -> Color {
        theme::color_value_to_color(value)
    }

    fn truncate_text(text: &str, max_length: usize) -> String {
        if text.chars().count() <= max_length {
            return text.to_string();
        }

        if max_length <= 3 {
            return ".".repeat(max_length);
        }

        let truncated: String = text.chars().take(max_length - 3).collect();
        format!("{}...", truncated)
    }

    fn append_aligned(buffer: &mut String, content: &str, width: usize) {
        buffer.push_str(content);
        let visible_width = Self::strip_ansi(content).width();
        if visible_width < width {
            buffer.push_str(&" ".repeat(width - visible_width));
        }
    }
}
#[derive(Debug, Default)]
struct GitInfo {
    branch: String,
    head: String,
    upstream: Option<String>,
    ahead: usize,
    behind: usize,
    detached: bool,
}

#[derive(Debug, Clone)]
struct CommitInfo {
    hash: String,
    subject: String,
    time: String,
    author: String,
}

impl Default for CommitInfo {
    fn default() -> Self {
        Self {
            hash: "-".to_string(),
            subject: "(uncommitted)".to_string(),
            time: "pending".to_string(),
            author: "-".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct FileGitStatus {
    raw: String,
    staged: Option<char>,
    worktree: Option<char>,
}

#[derive(Debug, Default)]
struct RepoStatus {
    entries: HashMap<String, FileGitStatus>,
    staged: usize,
    unstaged: usize,
    untracked: usize,
    conflicts: usize,
    ignored: usize,
}

struct PreparedTable {
    rows: Vec<RowData>,
    max_status_width: usize,
    max_name_width: usize,
    max_commit_width: usize,
    max_subject_width: usize,
    max_time_width: usize,
    max_author_width: usize,
    max_plugin_width: usize,
}

struct RowData {
    status: String,
    name: String,
    commit: String,
    subject: String,
    time: String,
    author: String,
    plugins: String,
}

impl FileGitStatus {
    fn from_raw(raw: &str) -> Self {
        let (staged, worktree) = match raw {
            "??" => (None, Some('?')),
            "!!" => (None, Some('!')),
            _ if raw.len() >= 2 => {
                let mut chars = raw.chars();
                (chars.next(), chars.next())
            }
            _ => (None, None),
        };

        Self {
            raw: raw.to_string(),
            staged,
            worktree,
        }
    }

    fn clean() -> Self {
        Self::from_raw("..")
    }

    fn is_untracked(&self) -> bool {
        self.raw == "??"
    }

    fn is_ignored(&self) -> bool {
        self.raw == "!!"
    }

    fn is_conflict(&self) -> bool {
        matches!(
            self.raw.as_str(),
            "DD" | "AA" | "UU" | "DU" | "UD" | "AU" | "UA"
        ) || self.raw.contains('U')
    }

    fn has_staged_change(&self) -> bool {
        matches!(self.staged, Some(code) if Self::is_change_code(code))
    }

    fn has_worktree_change(&self) -> bool {
        matches!(self.worktree, Some(code) if Self::is_change_code(code))
    }

    fn is_change_code(code: char) -> bool {
        matches!(code, 'M' | 'A' | 'D' | 'R' | 'C' | 'T')
    }
}

impl RepoStatus {
    fn status_for(&self, path: &str) -> FileGitStatus {
        self.entries
            .get(path)
            .cloned()
            .unwrap_or_else(FileGitStatus::clean)
    }

    fn record(&mut self, path: &str, status: FileGitStatus) {
        self.update_counts(&status);
        self.entries.insert(path.to_string(), status);
    }

    fn update_counts(&mut self, status: &FileGitStatus) {
        if status.is_untracked() {
            self.untracked += 1;
            return;
        }

        if status.is_ignored() {
            self.ignored += 1;
            return;
        }

        if status.has_staged_change() {
            self.staged += 1;
        }

        if status.has_worktree_change() {
            self.unstaged += 1;
        }

        if status.is_conflict() {
            self.conflicts += 1;
        }
    }
}

impl GitFormatter {
    fn get_git_info(path: &Path) -> Option<GitInfo> {
        let status = Command::new("git")
            .arg("rev-parse")
            .arg("--is-inside-work-tree")
            .current_dir(path)
            .output()
            .ok()?;

        if !status.status.success() {
            return None;
        }

        let mut info = GitInfo::default();

        if let Ok(output) = Command::new("git")
            .args(["status", "-b", "--porcelain=v2"])
            .current_dir(path)
            .output()
        {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if line.starts_with("# branch.head ") {
                        if let Some(value) = line.split_whitespace().nth(2) {
                            info.detached = value == "(detached)";
                            info.branch = if info.detached {
                                "DETACHED".to_string()
                            } else {
                                value.to_string()
                            };
                        }
                    } else if line.starts_with("# branch.oid ") {
                        if let Some(value) = line.split_whitespace().nth(2) {
                            info.head = value.to_string();
                        }
                    } else if line.starts_with("# branch.upstream ") {
                        if let Some(value) = line.split_whitespace().nth(2) {
                            info.upstream = Some(value.to_string());
                        }
                    } else if line.starts_with("# branch.ab ") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 4 {
                            info.ahead = parts[2].trim_start_matches('+').parse().unwrap_or(0);
                            info.behind = parts[3].trim_start_matches('-').parse().unwrap_or(0);
                        }
                    }
                }
            }
        }

        if info.branch.is_empty() {
            if let Ok(output) = Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(path)
                .output()
            {
                if output.status.success() {
                    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if branch == "HEAD" {
                        info.detached = true;
                        info.branch = "DETACHED".to_string();
                    } else {
                        info.branch = branch;
                    }
                }
            }
        }

        if info.head.is_empty() {
            if let Ok(output) = Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .current_dir(path)
                .output()
            {
                if output.status.success() {
                    info.head = String::from_utf8_lossy(&output.stdout).trim().to_string();
                }
            }
        } else if info.head.len() > 8 {
            info.head.truncate(8);
        }

        Some(info)
    }

    fn format_file_status(status: &FileGitStatus, theme: &theme::Theme) -> String {
        let staged_color = Self::get_theme_color(&theme.colors.executable);
        let worktree_color = Self::get_theme_color(&theme.colors.date);
        let untracked_color = Self::get_theme_color(&theme.colors.permission_none);
        let conflict_color = Self::get_theme_color(&theme.colors.permission_exec);

        if status.is_untracked() {
            return "[new]".to_string().color(untracked_color).to_string();
        }

        if status.is_ignored() {
            return "[ignored]".to_string().color(untracked_color).to_string();
        }

        let mut parts = Vec::new();

        if status.has_staged_change() {
            if let Some(code) = status.staged {
                parts.push(
                    format!("[S:{}]", Self::describe_status_code(code))
                        .color(staged_color)
                        .to_string(),
                );
            }
        }

        if status.has_worktree_change() {
            if let Some(code) = status.worktree {
                parts.push(
                    format!("[W:{}]", Self::describe_status_code(code))
                        .color(worktree_color)
                        .to_string(),
                );
            }
        }

        if status.is_conflict() {
            parts.push("[conflict]".to_string().color(conflict_color).to_string());
        }

        if parts.is_empty() {
            "[clean]".to_string().color(untracked_color).to_string()
        } else {
            parts.join(" ")
        }
    }

    fn describe_status_code(code: char) -> &'static str {
        match code {
            'M' => "mod",
            'A' => "add",
            'D' => "del",
            'R' => "ren",
            'C' => "copy",
            'T' => "type",
            'U' => "conf",
            _ => "chg",
        }
    }

    fn format_summary(status: &RepoStatus, theme: &theme::Theme) -> String {
        let staged_color = Self::get_theme_color(&theme.colors.executable);
        let worktree_color = Self::get_theme_color(&theme.colors.date);
        let untracked_color = Self::get_theme_color(&theme.colors.permission_none);
        let conflict_color = Self::get_theme_color(&theme.colors.permission_exec);

        let mut parts = Vec::new();

        if status.staged > 0 {
            parts.push(
                format!("stage {}", status.staged)
                    .color(staged_color)
                    .to_string(),
            );
        }

        if status.unstaged > 0 {
            parts.push(
                format!("worktree {}", status.unstaged)
                    .color(worktree_color)
                    .to_string(),
            );
        }

        if status.untracked > 0 {
            parts.push(
                format!("untracked {}", status.untracked)
                    .color(untracked_color)
                    .to_string(),
            );
        }

        if status.conflicts > 0 {
            parts.push(
                format!("conflict {}", status.conflicts)
                    .color(conflict_color)
                    .to_string(),
            );
        }

        if status.ignored > 0 {
            parts.push(
                format!("ignored {}", status.ignored)
                    .color(untracked_color)
                    .to_string(),
            );
        }

        if parts.is_empty() {
            "clean working tree"
                .to_string()
                .color(untracked_color)
                .to_string()
        } else {
            parts.join("  ")
        }
    }

    fn format_branch_header(info: &GitInfo, theme: &theme::Theme) -> String {
        let branch_color = Self::get_theme_color(&theme.colors.executable);
        let upstream_color = Self::get_theme_color(&theme.colors.symlink);
        let hash_color = Self::get_theme_color(&theme.colors.symlink);
        let ahead_color = Self::get_theme_color(&ColorValue::Named("yellow".to_string()));
        let behind_color = Self::get_theme_color(&ColorValue::Named("red".to_string()));

        let branch_label = if info.branch.is_empty() {
            "HEAD".to_string()
        } else {
            info.branch.clone()
        };

        let mut header = format!(
            "{} {}",
            "⎇".color(branch_color),
            branch_label.color(branch_color).bold()
        );

        if let Some(upstream) = info.upstream.as_ref() {
            header.push(' ');
            header.push_str(&format!("↥{}", upstream).color(upstream_color).to_string());
        }

        if !info.head.is_empty() {
            header.push(' ');
            header.push_str(&format!("@{}", info.head).color(hash_color).to_string());
        }

        if info.ahead > 0 {
            header.push(' ');
            header.push_str(&format!("↑{}", info.ahead).color(ahead_color).to_string());
        }

        if info.behind > 0 {
            header.push(' ');
            header.push_str(&format!("↓{}", info.behind).color(behind_color).to_string());
        }

        header
    }

    fn render_row(columns: &[(String, usize)]) -> String {
        let mut row = String::new();
        for (index, (content, width)) in columns.iter().enumerate() {
            Self::append_aligned(&mut row, content, *width);
            if index < columns.len() - 1 {
                row.push_str("  ");
            }
        }
        row
    }

    fn prepare_table(
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        workspace_root: &Path,
        repo_status: &RepoStatus,
        theme: &theme::Theme,
        hash_color: Color,
        time_color: Color,
        author_color: Color,
        subject_color: Color,
        subject_limit: usize,
        include_plugins: bool,
        show_icons: bool,
    ) -> PreparedTable {
        let mut rows: Vec<RowData> = Vec::with_capacity(files.len());
        let mut max_status_width: usize = 0;
        let mut max_name_width: usize = 0;
        let mut max_commit_width: usize = 0;
        let mut max_subject_width: usize = 0;
        let mut max_time_width: usize = 0;
        let mut max_author_width: usize = 0;
        let mut max_plugin_width: usize = 0;

        for file in files {
            let path = Path::new(&file.path);
            let name = colorize_file_name(path);
            let name_with_icon = colorize_file_name_with_icon(
                path,
                format_with_icon(path, name.to_string(), show_icons),
            )
            .to_string();

            let name_width = Self::strip_ansi(&name_with_icon).width();
            max_name_width = max_name_width.max(name_width);

            let relative_path = path.strip_prefix(workspace_root).unwrap_or(path);
            let relative_path_str = relative_path.to_string_lossy();

            let file_status = repo_status.status_for(relative_path_str.as_ref());
            let status_display = GitFormatter::format_file_status(&file_status, theme);
            let status_width = Self::strip_ansi(&status_display).width();
            max_status_width = max_status_width.max(status_width);

            let commit_info = GitFormatter::get_last_commit_info(workspace_root, relative_path)
                .unwrap_or_default();
            let hash = commit_info.hash;
            let subject_text = commit_info.subject;
            let time_text = commit_info.time;
            let author_text = commit_info.author;

            let commit_display = if hash == "-" {
                "-".to_string()
            } else {
                format!("@{}", hash).color(hash_color).to_string()
            };
            let commit_width = Self::strip_ansi(&commit_display).width();
            max_commit_width = max_commit_width.max(commit_width);

            let subject_truncated = GitFormatter::truncate_text(&subject_text, subject_limit);
            let subject_display = subject_truncated.color(subject_color).to_string();
            let subject_width = Self::strip_ansi(&subject_display).width();
            max_subject_width = max_subject_width.max(subject_width);

            let time_display = time_text.color(time_color).to_string();
            let time_width = Self::strip_ansi(&time_display).width();
            max_time_width = max_time_width.max(time_width);

            let author_display = if author_text == "-" {
                "-".to_string()
            } else {
                author_text.color(author_color).to_string()
            };
            let author_width = Self::strip_ansi(&author_display).width();
            max_author_width = max_author_width.max(author_width);

            let plugin_fields = if include_plugins {
                plugin_manager.format_fields(file, "git").join(" ")
            } else {
                String::new()
            };

            if include_plugins && !plugin_fields.is_empty() {
                let plugin_width = Self::strip_ansi(&plugin_fields).width();
                max_plugin_width = max_plugin_width.max(plugin_width);
            }

            rows.push(RowData {
                status: status_display,
                name: name_with_icon,
                commit: commit_display,
                subject: subject_display,
                time: time_display,
                author: author_display,
                plugins: plugin_fields,
            });
        }

        PreparedTable {
            rows,
            max_status_width,
            max_name_width,
            max_commit_width,
            max_subject_width,
            max_time_width,
            max_author_width,
            max_plugin_width,
        }
    }

    fn get_repo_status(workspace_root: &Path) -> RepoStatus {
        let mut repo_status = RepoStatus::default();

        if let Ok(output) = Command::new("git")
            .args(["status", "--porcelain=v2", "--untracked-files=all"])
            .current_dir(workspace_root)
            .output()
        {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }

                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.is_empty() {
                        continue;
                    }

                    match parts[0] {
                        "1" | "2" if parts.len() >= 9 => {
                            let xy = parts[1];
                            let path_index = if parts[0] == "2" && parts.len() >= 10 {
                                parts.len().saturating_sub(2)
                            } else {
                                parts.len().saturating_sub(1)
                            };

                            if let Some(path) = parts.get(path_index) {
                                let status = FileGitStatus::from_raw(xy);
                                repo_status.record(path, status);
                            }
                        }
                        "u" if parts.len() >= 10 => {
                            let xy = parts[1];
                            if let Some(path) = parts.last() {
                                let status = FileGitStatus::from_raw(xy);
                                repo_status.record(path, status);
                            }
                        }
                        "?" if parts.len() >= 2 => {
                            let path = parts[1];
                            let status = FileGitStatus::from_raw("??");
                            repo_status.record(path, status);
                        }
                        "!" if parts.len() >= 2 => {
                            let path = parts[1];
                            let status = FileGitStatus::from_raw("!!");
                            repo_status.record(path, status);
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Ok(output) = Command::new("git")
            .args(["ls-files"])
            .current_dir(workspace_root)
            .output()
        {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    repo_status
                        .entries
                        .entry(line.to_string())
                        .or_insert_with(FileGitStatus::clean);
                }
            }
        }

        repo_status
    }

    fn get_last_commit_info(path: &Path, file_path: &Path) -> Option<CommitInfo> {
        let output = Command::new("git")
            .args([
                "log",
                "-1",
                "--format=%h|%s|%cr|%an",
                "--",
                file_path.to_str()?,
            ])
            .current_dir(path)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let log = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = log.trim().splitn(4, '|').collect();
        if parts.len() >= 4 {
            Some(CommitInfo {
                hash: parts[0].to_string(),
                subject: parts[1].to_string(),
                time: parts[2].to_string(),
                author: parts[3].to_string(),
            })
        } else {
            None
        }
    }
}

impl FileFormatter for GitFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        _depth: Option<usize>,
    ) -> Result<String> {
        if files.is_empty() {
            return Ok(String::new());
        }

        let theme = color::get_theme();
        let hash_color = Self::get_theme_color(&theme.colors.symlink);
        let time_color = Self::get_theme_color(&theme.colors.date);
        let author_color = Self::get_theme_color(&theme.colors.user);
        let subject_color = Self::get_theme_color(&theme.colors.file);
        let separator_color = Self::get_theme_color(&theme.colors.permission_none);

        let workspace_root = Path::new(&files[0].path)
            .ancestors()
            .find(|p| p.join(".git").exists())
            .unwrap_or(Path::new("."));

        let git_info = match GitFormatter::get_git_info(workspace_root) {
            Some(info) => info,
            None => return Ok("Not a git repository".red().to_string()),
        };

        let repo_status = GitFormatter::get_repo_status(workspace_root);

        let branch_header = GitFormatter::format_branch_header(&git_info, theme);
        let summary_line = GitFormatter::format_summary(&repo_status, theme);
        let branch_width = Self::strip_ansi(&branch_header).width();
        let summary_width = Self::strip_ansi(&summary_line).width();

        let mut subject_limit = SUBJECT_MAX_LENGTH;
        let mut include_plugins = true;
        let term_width = terminal_size()
            .map(|(Width(w), _)| w as usize)
            .unwrap_or(120);

        let (prepared_rows, header_columns, separator_len, include_plugins_final) = loop {
            let prepared = GitFormatter::prepare_table(
                files,
                plugin_manager,
                workspace_root,
                &repo_status,
                theme,
                hash_color,
                time_color,
                author_color,
                subject_color,
                subject_limit,
                include_plugins,
                self.show_icons,
            );

            let status_header = "Status".bold().to_string();
            let name_header = "Name".bold().to_string();
            let commit_header = "Commit".bold().to_string();
            let subject_header = "Subject".bold().to_string();
            let time_header = "Time".bold().to_string();
            let author_header = "Author".bold().to_string();
            let plugin_header = "Plugins".bold().to_string();

            let status_width = prepared
                .max_status_width
                .max(Self::strip_ansi(&status_header).width());
            let name_width = prepared
                .max_name_width
                .max(Self::strip_ansi(&name_header).width());
            let commit_width = prepared
                .max_commit_width
                .max(Self::strip_ansi(&commit_header).width());
            let subject_width = prepared
                .max_subject_width
                .max(Self::strip_ansi(&subject_header).width());
            let time_width = prepared
                .max_time_width
                .max(Self::strip_ansi(&time_header).width());
            let author_width = prepared
                .max_author_width
                .max(Self::strip_ansi(&author_header).width());

            let include_plugins_effective = include_plugins && prepared.max_plugin_width > 0;
            let plugin_width = if include_plugins_effective {
                prepared
                    .max_plugin_width
                    .max(Self::strip_ansi(&plugin_header).width())
            } else {
                0
            };

            let mut columns = vec![
                (status_header, status_width),
                (name_header, name_width),
                (commit_header, commit_width),
                (subject_header, subject_width),
                (time_header, time_width),
                (author_header, author_width),
            ];

            if include_plugins_effective {
                columns.push((plugin_header, plugin_width));
            }

            let table_width = columns.iter().map(|(_, width)| *width).sum::<usize>()
                + columns.len().saturating_sub(1) * 2;

            let current_separator_len = table_width.max(branch_width).max(summary_width).max(40);

            let fits = term_width == 0 || table_width <= term_width;
            let subject_at_min = subject_limit <= SUBJECT_MIN_LENGTH;

            if fits || (subject_at_min && !include_plugins_effective) {
                break (
                    prepared.rows,
                    columns,
                    current_separator_len,
                    include_plugins_effective,
                );
            }

            if table_width > term_width && !subject_at_min {
                let overflow = table_width - term_width;
                let allowed_reduction = subject_limit - SUBJECT_MIN_LENGTH;
                let reduce_by = overflow.min(allowed_reduction);
                if reduce_by > 0 {
                    subject_limit -= reduce_by;
                    continue;
                }
            }

            if include_plugins_effective {
                include_plugins = false;
                continue;
            }

            break (
                prepared.rows,
                columns,
                current_separator_len,
                include_plugins_effective,
            );
        };

        let separator_line = "─".repeat(separator_len).color(separator_color).to_string();
        let header_row = GitFormatter::render_row(&header_columns);

        let mut output = String::new();
        output.push('\n');
        output.push_str(&branch_header);
        output.push('\n');
        output.push_str(&summary_line);
        output.push('\n');
        output.push_str(&separator_line);
        output.push('\n');
        output.push_str(&header_row);
        output.push('\n');
        output.push_str(&separator_line);
        output.push('\n');

        let mut width_iter = header_columns.iter().map(|(_, width)| *width);
        let status_width = width_iter.next().unwrap_or(0);
        let name_width = width_iter.next().unwrap_or(0);
        let commit_width = width_iter.next().unwrap_or(0);
        let subject_width = width_iter.next().unwrap_or(0);
        let time_width = width_iter.next().unwrap_or(0);
        let author_width = width_iter.next().unwrap_or(0);
        let plugin_width = if include_plugins_final {
            width_iter.next().unwrap_or(0)
        } else {
            0
        };

        for row in prepared_rows {
            let mut columns = vec![
                (row.status, status_width),
                (row.name, name_width),
                (row.commit, commit_width),
                (row.subject, subject_width),
                (row.time, time_width),
                (row.author, author_width),
            ];

            if include_plugins_final {
                columns.push((row.plugins, plugin_width));
            }

            output.push_str(&GitFormatter::render_row(&columns));
            output.push('\n');
        }

        Ok(output)
    }
}
