use super::{render_entry_name, FileFormatter};
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::theme::{self, ColorValue};
use crate::utils::color;
use colored::*;
use lla_plugin_interface::proto::DecoratedEntry;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use terminal_size::{terminal_size, Width};
use unicode_width::UnicodeWidthStr;

const SUBJECT_MAX_LENGTH: usize = 48;
const SUBJECT_MIN_LENGTH: usize = 16;
/// Above this many distinct parent directories, scope history lookups to the
/// whole repository instead of passing every directory as a pathspec.
const MAX_PATHSPECS: usize = 64;
/// Record separator emitted before each commit header in `git log` output.
const COMMIT_MARKER: char = '\x1e';

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

#[derive(Debug, Clone, PartialEq)]
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

/// Row content that does not depend on the layout, computed once per listing.
struct BaseRow {
    status: String,
    status_width: usize,
    name: String,
    name_width: usize,
    commit: String,
    commit_width: usize,
    subject: String,
    time: String,
    time_width: usize,
    author: String,
    author_width: usize,
    plugins: String,
    plugin_width: usize,
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

/// Split NUL-terminated `-z` output into lossy UTF-8 records, dropping empty ones.
fn split_nul(bytes: &[u8]) -> impl Iterator<Item = String> + '_ {
    bytes
        .split(|&b| b == 0)
        .filter(|record| !record.is_empty())
        .map(|record| String::from_utf8_lossy(record).into_owned())
}

/// Parse `git status -b --porcelain=v2 -z` output. Paths are relative to the
/// repository root and are never quoted, so names with spaces or non-ASCII
/// characters survive intact.
fn parse_status_v2_z(bytes: &[u8]) -> (GitInfo, RepoStatus) {
    let mut info = GitInfo::default();
    let mut status = RepoStatus::default();
    let mut records = split_nul(bytes);

    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            let mut parts = header.split(' ');
            let key = parts.next().unwrap_or_default();
            let value = parts.next().unwrap_or_default();
            match key {
                "branch.head" => {
                    info.detached = value == "(detached)";
                    info.branch = if info.detached {
                        "DETACHED".to_string()
                    } else {
                        value.to_string()
                    };
                }
                "branch.oid" => info.head = value.to_string(),
                "branch.upstream" => info.upstream = Some(value.to_string()),
                "branch.ab" => {
                    info.ahead = value.trim_start_matches('+').parse().unwrap_or(0);
                    info.behind = parts
                        .next()
                        .unwrap_or_default()
                        .trim_start_matches('-')
                        .parse()
                        .unwrap_or(0);
                }
                _ => {}
            }
            continue;
        }

        let (kind, rest) = record.split_at(record.find(' ').unwrap_or(record.len()));
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        // Number of space-separated fields that precede the path in each record type.
        let (xy, path) = match kind {
            "1" => split_fields(rest, 7),
            "2" => {
                // Renames and copies are followed by a separate record holding the
                // original path, which is not listed in the working tree.
                records.next();
                split_fields(rest, 8)
            }
            "u" => split_fields(rest, 9),
            "?" => (Some("??"), Some(rest)),
            "!" => (Some("!!"), Some(rest)),
            _ => (None, None),
        };

        if let (Some(xy), Some(path)) = (xy, path) {
            if !path.is_empty() {
                status.record(path, FileGitStatus::from_raw(xy));
            }
        }
    }

    (info, status)
}

/// Return the first field and whatever follows `count` fields (the path, which
/// may itself contain spaces).
fn split_fields(rest: &str, count: usize) -> (Option<&str>, Option<&str>) {
    let mut parts = rest.splitn(count + 1, ' ');
    let first = parts.next();
    (first, parts.nth(count - 1))
}

/// Incremental parser for
/// `git log --format=%x1e%h%x00%s%x00%cr%x00%an --name-only -z` output fed one
/// NUL-terminated token at a time.
#[derive(Default)]
struct LogStreamParser {
    header: Option<Vec<String>>,
}

#[derive(Debug, PartialEq)]
enum LogToken {
    Commit(CommitInfo),
    Path(String),
}

impl LogStreamParser {
    fn feed(&mut self, token: &[u8]) -> Option<LogToken> {
        let token = String::from_utf8_lossy(token);

        if let Some(hash) = token.strip_prefix(COMMIT_MARKER) {
            self.header = Some(vec![hash.to_string()]);
            return None;
        }

        if let Some(fields) = self.header.as_mut() {
            fields.push(token.into_owned());
            if fields.len() < 4 {
                return None;
            }
            let mut fields = self.header.take().unwrap_or_default().into_iter();
            let mut next = || fields.next().unwrap_or_default();
            return Some(LogToken::Commit(CommitInfo {
                hash: next(),
                subject: next(),
                time: next(),
                author: next(),
            }));
        }

        let path = token.strip_prefix('\n').unwrap_or(&token);
        (!path.is_empty()).then(|| LogToken::Path(path.to_string()))
    }
}

/// Assign each wanted path (file or directory, repo-relative) the newest commit
/// touching it, consuming tokens until every wanted path is resolved. Returns
/// `true` once all wanted paths are resolved, so the caller can stop reading.
fn resolve_commits<T: AsRef<[u8]>>(
    tokens: impl IntoIterator<Item = T>,
    wanted: &HashSet<String>,
    resolved: &mut HashMap<String, CommitInfo>,
) -> bool {
    let mut parser = LogStreamParser::default();
    let mut current: Option<CommitInfo> = None;

    if wanted.is_empty() {
        return true;
    }

    for token in tokens {
        match parser.feed(token.as_ref()) {
            Some(LogToken::Commit(commit)) => current = Some(commit),
            Some(LogToken::Path(path)) => {
                let Some(commit) = current.as_ref() else {
                    continue;
                };
                if record_path(&path, commit, wanted, resolved) && resolved.len() == wanted.len() {
                    return true;
                }
            }
            None => {}
        }
    }

    resolved.len() == wanted.len()
}

/// Credit `commit` to `path` and every wanted ancestor directory that has not
/// already been resolved by a newer commit. Returns whether anything changed.
fn record_path(
    path: &str,
    commit: &CommitInfo,
    wanted: &HashSet<String>,
    resolved: &mut HashMap<String, CommitInfo>,
) -> bool {
    let mut changed = false;
    let mut candidate = path;
    loop {
        if resolved.contains_key(candidate) {
            // A newer commit already resolved this path, and with it every wanted
            // ancestor, because ancestors are always visited together.
            break;
        }
        if wanted.contains(candidate) {
            resolved.insert(candidate.to_string(), commit.clone());
            changed = true;
        }
        match candidate.rfind('/') {
            Some(index) => candidate = &candidate[..index],
            None => break,
        }
    }
    changed
}

/// Collect all directories (repo-relative) that contain at least one of `files`.
fn ancestor_dirs<'a>(files: impl IntoIterator<Item = &'a String>) -> HashSet<String> {
    let mut dirs = HashSet::new();
    for file in files {
        let mut candidate = file.as_str();
        while let Some(index) = candidate.rfind('/') {
            candidate = &candidate[..index];
            if !dirs.insert(candidate.to_string()) {
                break;
            }
        }
    }
    dirs
}

/// Convert a relative path to the `/`-separated form git prints.
fn to_git_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .args(["-c", "core.quotepath=off", "-c", "log.follow=false"]);
    command
}

/// Paths for the listed entries, expressed relative to the repository root.
struct EntryKeys {
    keys: Vec<Option<String>>,
}

impl EntryKeys {
    fn new(files: &[DecoratedEntry], root: &Path) -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut parents: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();

        let keys = files
            .iter()
            .map(|file| {
                let path = Path::new(&file.path);
                let absolute = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    cwd.join(path)
                };

                // Resolve the parent directory rather than the entry itself so a
                // symlink is reported as the link git tracks, not its target.
                let resolved = match (absolute.parent(), absolute.file_name()) {
                    (Some(parent), Some(name)) => parents
                        .entry(parent.to_path_buf())
                        .or_insert_with(|| std::fs::canonicalize(parent).ok())
                        .as_ref()
                        .map(|parent| parent.join(name)),
                    _ => std::fs::canonicalize(&absolute).ok(),
                }?;

                resolved.strip_prefix(root).ok().map(to_git_path)
            })
            .collect();

        Self { keys }
    }

    /// Distinct parent directories of the entries, used to scope history walks.
    fn pathspecs(&self) -> Vec<String> {
        let parents: HashSet<String> = self
            .keys
            .iter()
            .flatten()
            .map(|key| match key.rfind('/') {
                Some(index) => key[..index].to_string(),
                None => ".".to_string(),
            })
            .collect();

        if parents.len() > MAX_PATHSPECS || parents.contains(".") {
            return vec![".".to_string()];
        }
        let mut parents: Vec<_> = parents.into_iter().collect();
        parents.sort();
        parents
    }
}

impl GitFormatter {
    fn repo_root(dir: &Path) -> Option<PathBuf> {
        let output = git_command(dir)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        std::fs::canonicalize(root).ok()
    }

    fn load_status(root: &Path) -> (GitInfo, RepoStatus) {
        let output = git_command(root)
            .args([
                "status",
                "-b",
                "--porcelain=v2",
                "-z",
                "--untracked-files=all",
            ])
            .output();
        let (mut info, status) = match output {
            Ok(output) if output.status.success() => parse_status_v2_z(&output.stdout),
            _ => (GitInfo::default(), RepoStatus::default()),
        };

        if info.branch.is_empty() {
            if let Ok(output) = git_command(root)
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
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
            if let Ok(output) = git_command(root)
                .args(["rev-parse", "--short", "HEAD"])
                .output()
            {
                if output.status.success() {
                    info.head = String::from_utf8_lossy(&output.stdout).trim().to_string();
                }
            }
        } else if info.head.starts_with('(') {
            // "(initial)" on a branch with no commits yet.
            info.head.clear();
        } else if info.head.len() > 8 {
            info.head.truncate(8);
        }

        (info, status)
    }

    /// Entries (files and directories) that have committed history and can
    /// therefore be resolved by walking `git log`.
    fn committed_paths(
        root: &Path,
        pathspecs: &[String],
        status: &RepoStatus,
    ) -> Option<HashSet<String>> {
        let output = git_command(root)
            .args(["--literal-pathspecs", "ls-files", "-z", "--"])
            .args(pathspecs)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        // Newly added files are in the index but have no history yet.
        let files: HashSet<String> = split_nul(&output.stdout)
            .filter(|path| {
                status
                    .entries
                    .get(path)
                    .is_none_or(|entry| entry.staged != Some('A'))
            })
            .collect();
        let mut committed = ancestor_dirs(&files);
        committed.extend(files);
        Some(committed)
    }

    /// Find the newest commit for every listed entry with a single `git log`
    /// walk, stopping as soon as every entry has been resolved.
    fn last_commits(
        root: &Path,
        keys: &EntryKeys,
        status: &RepoStatus,
    ) -> HashMap<String, CommitInfo> {
        let mut resolved = HashMap::new();
        let pathspecs = keys.pathspecs();
        let Some(committed) = Self::committed_paths(root, &pathspecs, status) else {
            return resolved;
        };

        let wanted: HashSet<String> = keys
            .keys
            .iter()
            .flatten()
            .filter(|key| committed.contains(*key))
            .cloned()
            .collect();
        if wanted.is_empty() {
            return resolved;
        }

        let format = format!("--format={}%h%x00%s%x00%cr%x00%an", COMMIT_MARKER);
        let child = git_command(root)
            .args([
                "--literal-pathspecs",
                "log",
                format.as_str(),
                "--name-only",
                "--no-renames",
                "-z",
                "--",
            ])
            .args(&pathspecs)
            .stdout(Stdio::piped())
            .spawn();
        let Ok(mut child) = child else {
            return resolved;
        };

        if let Some(stdout) = child.stdout.take() {
            let mut reader = BufReader::with_capacity(64 * 1024, stdout);
            let tokens = std::iter::from_fn(|| {
                let mut token = Vec::new();
                match reader.read_until(0, &mut token) {
                    Ok(0) | Err(_) => None,
                    Ok(_) => {
                        if token.last() == Some(&0) {
                            token.pop();
                        }
                        Some(token)
                    }
                }
            });
            resolve_commits(tokens, &wanted, &mut resolved);
        }

        // Either everything resolved early or history ran out; in both cases the
        // rest of the walk is not needed.
        let _ = child.kill();
        let _ = child.wait();
        resolved
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

    #[allow(clippy::too_many_arguments)]
    fn base_rows(
        files: &[DecoratedEntry],
        keys: &EntryKeys,
        plugin_manager: &mut PluginManager,
        repo_status: &RepoStatus,
        commits: &HashMap<String, CommitInfo>,
        theme: &theme::Theme,
        hash_color: Color,
        time_color: Color,
        author_color: Color,
        show_icons: bool,
    ) -> Vec<BaseRow> {
        files
            .iter()
            .zip(&keys.keys)
            .map(|(file, key)| {
                let name = render_entry_name(file, show_icons);

                let file_status = key
                    .as_deref()
                    .map(|key| repo_status.status_for(key))
                    .unwrap_or_else(FileGitStatus::clean);
                let status = GitFormatter::format_file_status(&file_status, theme);

                let commit_info = key
                    .as_ref()
                    .and_then(|key| commits.get(key))
                    .cloned()
                    .unwrap_or_default();

                let commit = if commit_info.hash == "-" {
                    "-".to_string()
                } else {
                    format!("@{}", commit_info.hash)
                        .color(hash_color)
                        .to_string()
                };
                let time = commit_info.time.color(time_color).to_string();
                let author = if commit_info.author == "-" {
                    "-".to_string()
                } else {
                    commit_info.author.color(author_color).to_string()
                };
                let plugins = plugin_manager.format_fields(file, "git").join(" ");

                BaseRow {
                    status_width: Self::strip_ansi(&status).width(),
                    name_width: Self::strip_ansi(&name).width(),
                    commit_width: Self::strip_ansi(&commit).width(),
                    time_width: Self::strip_ansi(&time).width(),
                    author_width: Self::strip_ansi(&author).width(),
                    plugin_width: Self::strip_ansi(&plugins).width(),
                    status,
                    name,
                    commit,
                    subject: commit_info.subject,
                    time,
                    author,
                    plugins,
                }
            })
            .collect()
    }

    /// Lay out the precomputed rows for a given subject width. This only
    /// truncates text and measures columns; it never touches git or the disk.
    fn prepare_table(
        rows: &[BaseRow],
        subject_color: Color,
        subject_limit: usize,
        include_plugins: bool,
    ) -> PreparedTable {
        let mut table = PreparedTable {
            rows: Vec::with_capacity(rows.len()),
            max_status_width: 0,
            max_name_width: 0,
            max_commit_width: 0,
            max_subject_width: 0,
            max_time_width: 0,
            max_author_width: 0,
            max_plugin_width: 0,
        };

        for row in rows {
            let subject = GitFormatter::truncate_text(&row.subject, subject_limit);
            let subject_width = subject.width();
            let subject = subject.color(subject_color).to_string();

            table.max_status_width = table.max_status_width.max(row.status_width);
            table.max_name_width = table.max_name_width.max(row.name_width);
            table.max_commit_width = table.max_commit_width.max(row.commit_width);
            table.max_subject_width = table.max_subject_width.max(subject_width);
            table.max_time_width = table.max_time_width.max(row.time_width);
            table.max_author_width = table.max_author_width.max(row.author_width);
            if include_plugins {
                table.max_plugin_width = table.max_plugin_width.max(row.plugin_width);
            }

            table.rows.push(RowData {
                status: row.status.clone(),
                name: row.name.clone(),
                commit: row.commit.clone(),
                subject,
                time: row.time.clone(),
                author: row.author.clone(),
                plugins: if include_plugins {
                    row.plugins.clone()
                } else {
                    String::new()
                },
            });
        }

        table
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
        plugin_manager.prepare_format_fields(files, "git");

        let theme = color::get_theme();
        let hash_color = Self::get_theme_color(&theme.colors.symlink);
        let time_color = Self::get_theme_color(&theme.colors.date);
        let author_color = Self::get_theme_color(&theme.colors.user);
        let subject_color = Self::get_theme_color(&theme.colors.file);
        let separator_color = Self::get_theme_color(&theme.colors.permission_none);

        let first = Path::new(&files[0].path);
        let probe_dir = match first.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let root = match GitFormatter::repo_root(probe_dir) {
            Some(root) => root,
            None => return Ok("Not a git repository".red().to_string()),
        };

        let (git_info, repo_status) = GitFormatter::load_status(&root);
        let keys = EntryKeys::new(files, &root);
        let commits = GitFormatter::last_commits(&root, &keys, &repo_status);
        let base_rows = GitFormatter::base_rows(
            files,
            &keys,
            plugin_manager,
            &repo_status,
            &commits,
            theme,
            hash_color,
            time_color,
            author_color,
            self.show_icons,
        );

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
                &base_rows,
                subject_color,
                subject_limit,
                include_plugins,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str) -> CommitInfo {
        CommitInfo {
            hash: hash.to_string(),
            subject: format!("subject {hash}"),
            time: "1 day ago".to_string(),
            author: "dev".to_string(),
        }
    }

    fn log_bytes(commits: &[(&str, &[&str])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (hash, paths) in commits {
            out.extend_from_slice(
                format!("\x1e{hash}\0subject {hash}\x001 day ago\0dev\0").as_bytes(),
            );
            for (index, path) in paths.iter().enumerate() {
                if index == 0 {
                    out.push(b'\n');
                }
                out.extend_from_slice(path.as_bytes());
                out.push(0);
            }
        }
        out
    }

    fn tokens(bytes: &[u8]) -> Vec<&[u8]> {
        let mut tokens: Vec<&[u8]> = bytes.split(|&b| b == 0).collect();
        if tokens.last().is_some_and(|t| t.is_empty()) {
            tokens.pop();
        }
        tokens
    }

    #[test]
    fn status_parser_keeps_spaces_unicode_and_renames() {
        let raw = concat!(
            "# branch.oid 0123456789abcdef\0",
            "# branch.head main\0",
            "# branch.upstream origin/main\0",
            "# branch.ab +2 -1\0",
            "1 .M N... 100644 100644 100644 aaa aaa my notes.txt\0",
            "2 R. N... 100644 100644 100644 aaa aaa R100 new name.md\0old name.md\0",
            "u UU N... 100644 100644 100644 100644 a b c sub/conflict file.rs\0",
            "? café.txt\0",
            "? dir/with space/x\0",
            "! target/out\0",
        );
        let (info, status) = parse_status_v2_z(raw.as_bytes());

        assert_eq!(info.branch, "main");
        assert_eq!(info.upstream.as_deref(), Some("origin/main"));
        assert_eq!((info.ahead, info.behind), (2, 1));
        assert_eq!(info.head, "0123456789abcdef");

        assert_eq!(status.status_for("my notes.txt").raw, ".M");
        assert_eq!(status.status_for("new name.md").raw, "R.");
        assert!(!status.entries.contains_key("old name.md"));
        assert!(status.status_for("sub/conflict file.rs").is_conflict());
        assert!(status.status_for("café.txt").is_untracked());
        assert!(status.status_for("dir/with space/x").is_untracked());
        assert!(status.status_for("target/out").is_ignored());
        assert_eq!(status.untracked, 2);
        assert_eq!(status.unstaged, 1);
        assert_eq!(status.conflicts, 1);
    }

    #[test]
    fn log_parser_reads_commits_and_paths() {
        let bytes = log_bytes(&[("abc", &["a b.txt", "café.txt"]), ("def", &[])]);
        let mut parser = LogStreamParser::default();
        let parsed: Vec<_> = tokens(&bytes)
            .into_iter()
            .filter_map(|t| parser.feed(t))
            .collect();
        assert_eq!(
            parsed,
            vec![
                LogToken::Commit(commit("abc")),
                LogToken::Path("a b.txt".to_string()),
                LogToken::Path("café.txt".to_string()),
                LogToken::Commit(commit("def")),
            ]
        );
    }

    #[test]
    fn resolve_keeps_newest_commit_and_credits_directories() {
        let bytes = log_bytes(&[
            ("c3", &["src/deep/new.rs"]),
            ("c2", &["README.md", "src/lib.rs"]),
            ("c1", &["README.md", "docs/guide.md"]),
        ]);
        let wanted: HashSet<String> = ["README.md", "src", "src/lib.rs", "docs"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut resolved = HashMap::new();
        assert!(resolve_commits(tokens(&bytes), &wanted, &mut resolved));
        assert_eq!(resolved["src"].hash, "c3");
        assert_eq!(resolved["src/lib.rs"].hash, "c2");
        assert_eq!(resolved["README.md"].hash, "c2");
        assert_eq!(resolved["docs"].hash, "c1");
    }

    #[test]
    fn resolve_stops_once_everything_is_found() {
        let bytes = log_bytes(&[("new", &["a"]), ("old", &["a", "b"])]);
        let wanted: HashSet<String> = ["a".to_string()].into_iter().collect();
        let mut resolved = HashMap::new();
        let mut consumed = 0;
        let counted = tokens(&bytes).into_iter().inspect(|_| consumed += 1);
        assert!(resolve_commits(counted, &wanted, &mut resolved));
        assert_eq!(resolved["a"].hash, "new");
        assert_eq!(consumed, 5, "walk should stop after the first commit");
    }

    #[test]
    fn ancestor_dirs_lists_every_parent() {
        let files = vec!["a/b/c.txt".to_string(), "a/d.txt".to_string()];
        let dirs = ancestor_dirs(&files);
        let mut dirs: Vec<_> = dirs.into_iter().collect();
        dirs.sort();
        assert_eq!(dirs, vec!["a", "a/b"]);
    }

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(["-c", "user.name=lla", "-c", "user.email=lla@example.com"])
            .args(args)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[test]
    fn real_repo_reports_spaced_and_unicode_names() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        if !git(dir, &["init", "-q"]) {
            eprintln!("git not available; skipping");
            return;
        }
        std::fs::create_dir(dir.join("sub dir")).unwrap();
        std::fs::write(dir.join("tracked file.txt"), "one").unwrap();
        std::fs::write(dir.join("sub dir/inner.txt"), "one").unwrap();
        assert!(git(dir, &["add", "."]));
        assert!(git(dir, &["commit", "-qm", "first commit"]));
        std::fs::write(dir.join("tracked file.txt"), "two").unwrap();
        std::fs::write(dir.join("my notes.txt"), "").unwrap();
        std::fs::write(dir.join("café.txt"), "").unwrap();

        let root = GitFormatter::repo_root(dir).unwrap();
        let (_, status) = GitFormatter::load_status(&root);
        assert!(status.status_for("my notes.txt").is_untracked());
        assert!(status.status_for("café.txt").is_untracked());
        assert!(status.status_for("tracked file.txt").has_worktree_change());

        let entries: Vec<DecoratedEntry> =
            ["tracked file.txt", "sub dir", "my notes.txt", "café.txt"]
                .iter()
                .map(|name| DecoratedEntry {
                    path: dir.join(name).to_string_lossy().into_owned(),
                    ..Default::default()
                })
                .collect();
        let keys = EntryKeys::new(&entries, &root);
        assert_eq!(keys.keys[3].as_deref(), Some("café.txt"));
        let commits = GitFormatter::last_commits(&root, &keys, &status);
        assert_eq!(commits["tracked file.txt"].subject, "first commit");
        assert_eq!(commits["sub dir"].subject, "first commit");
        assert!(!commits.contains_key("my notes.txt"));
    }
}
