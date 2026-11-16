#![cfg_attr(test, allow(dead_code))]

use super::FileLister;
use crate::utils::color::*;
use crate::utils::fs_metadata;
use crate::utils::icons::format_with_icon;
use crate::{error::Result, theme::color_value_to_color};
use colored::*;
use crossbeam_channel::{bounded, Sender};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, queue,
    style::Print,
    terminal::{self, ClearType},
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ignore::{ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

const CHUNK_SIZE: usize = 1000;
const MAX_RESULTS: usize = 2000;
const POLL_INTERVAL_MS: u64 = 16;
const BATCH_CHECK_MS: u64 = 100;
const SPINNER_INTERVAL_MS: u64 = 80;
const STATUS_MESSAGE_TIMEOUT_MS: u64 = 2000;
const PREVIEW_MIN_WIDTH: usize = 120;
const PREVIEW_MAX_BYTES: u64 = 64 * 1024;
const BINARY_SNIFF_BYTES: usize = 8 * 1024;
const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// Per-character weights for the non-fuzzy term kinds, roughly on the same scale
// as the skim matcher so mixed queries rank sensibly.
const EXACT_CHAR_SCORE: i64 = 16;
const ANCHOR_CHAR_SCORE: i64 = 24;
const NAME_CHAR_BONUS: i64 = 16;

fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    // Best-effort cross-platform clipboard support
    // macOS: pbcopy, Linux (common): xclip or xsel, Windows: clip
    #[cfg(target_os = "macos")]
    {
        let mut child = ProcessCommand::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes())?;
        }
        let _ = child.wait();
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        // Try xclip
        if let Ok(mut child) = ProcessCommand::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return Ok(());
        }
        // Try xsel
        if let Ok(mut child) = ProcessCommand::new("xsel")
            .args(["--clipboard", "--input"])
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return Ok(());
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let mut child = ProcessCommand::new("cmd")
            .args(["/C", "clip"])
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes())?;
        }
        let _ = child.wait();
        return Ok(());
    }

    #[allow(unreachable_code)]
    {
        let _ = text;
        Ok(())
    }
}

fn open_path(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = ProcessCommand::new("open").arg(path).spawn()?.wait();
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let _ = ProcessCommand::new("xdg-open").arg(path).spawn()?.wait();
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        let _ = ProcessCommand::new("cmd")
            .args(["/C", "start", path.to_string_lossy().as_ref()])
            .spawn()?
            .wait();
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        let _ = path;
        Ok(())
    }
}

fn open_in_editor(paths: &[PathBuf], config_editor: Option<&str>) -> std::io::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }

    // Priority: config editor > $EDITOR > $VISUAL > fallback
    // Treat empty string as None to allow fallback chain
    let editor = config_editor
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("EDITOR").ok())
        .or_else(|| std::env::var("VISUAL").ok())
        .unwrap_or_else(|| {
            // Try to find a common editor
            #[cfg(target_os = "macos")]
            {
                "nano".to_string()
            }
            #[cfg(target_os = "linux")]
            {
                "nano".to_string()
            }
            #[cfg(target_os = "windows")]
            {
                "notepad".to_string()
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
            {
                "vi".to_string()
            }
        });

    let editor = editor.trim();
    if editor.is_empty() {
        return Ok(());
    }

    // Support simple "editor + args" strings like `code --wait`
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or(editor);
    let mut cmd = ProcessCommand::new(program);
    cmd.args(parts);
    for path in paths {
        cmd.arg(path);
    }
    // When lla's stdout is captured (e.g. `vim $(lla -F)`), give the editor the
    // terminal instead of the pipe.
    #[cfg(unix)]
    if !std::io::IsTerminal::is_terminal(&io::stdout()) {
        if let Ok(tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
            cmd.stdout(tty);
        }
    }
    let _ = cmd.spawn()?.wait();
    Ok(())
}

fn rename_file(old_path: &Path, new_name: &str) -> std::io::Result<PathBuf> {
    if new_name.contains('/') || new_name.contains(std::path::MAIN_SEPARATOR) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "name must not contain a path separator",
        ));
    }
    let parent = old_path.parent().unwrap_or(Path::new("."));
    let new_path = parent.join(new_name);
    // `fs::rename` silently replaces an existing file on Unix.
    if new_path.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", new_path.display()),
        ));
    }
    std::fs::rename(old_path, &new_path)?;
    Ok(new_path)
}

/// Where the interactive UI is drawn. Using the controlling terminal rather
/// than stdout keeps `$(lla -F)` and `lla -F | xargs ...` working.
fn tty_writer() -> Box<dyn Write> {
    #[cfg(unix)]
    {
        if let Ok(tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
            return Box::new(tty);
        }
    }
    Box::new(io::stderr())
}

/// Patterns from `listers.fuzzy.ignore_patterns`, compiled once. Patterns are
/// matched against the path relative to the search root. A plain pattern
/// without a separator names a whole path component, so `.git` does not hide
/// `.github` and `target` does not hide `targets.rs`.
enum IgnoreRule {
    Component(String),
    Substring(String),
    Glob(glob::Pattern),
    Regex(regex::Regex),
}

struct IgnoreRules(Vec<IgnoreRule>);

impl IgnoreRules {
    fn new(patterns: &[String]) -> Self {
        let rules = patterns
            .iter()
            .filter_map(|pattern| {
                if let Some(pattern) = pattern.strip_prefix("regex:") {
                    regex::Regex::new(pattern).ok().map(IgnoreRule::Regex)
                } else if let Some(pattern) = pattern.strip_prefix("glob:") {
                    glob::Pattern::new(pattern).ok().map(IgnoreRule::Glob)
                } else if pattern.is_empty() {
                    None
                } else if !pattern.contains('/') && !pattern.contains(std::path::MAIN_SEPARATOR) {
                    Some(IgnoreRule::Component(pattern.clone()))
                } else {
                    Some(IgnoreRule::Substring(pattern.clone()))
                }
            })
            .collect();
        Self(rules)
    }

    fn matches(&self, rel: &str) -> bool {
        self.0.iter().any(|rule| match rule {
            IgnoreRule::Component(name) => has_component(rel, name),
            IgnoreRule::Substring(pattern) => rel.contains(pattern.as_str()),
            IgnoreRule::Glob(glob) => glob.matches(rel),
            IgnoreRule::Regex(re) => re.is_match(rel),
        })
    }

    /// Whether a whole directory can be skipped. Only component and substring
    /// rules are safe here: every path below a matching directory matches too.
    fn prunes_dir(&self, rel: &str) -> bool {
        self.0.iter().any(|rule| match rule {
            IgnoreRule::Component(name) => has_component(rel, name),
            IgnoreRule::Substring(pattern) => rel.contains(pattern.as_str()),
            _ => false,
        })
    }
}

fn has_component(rel: &str, name: &str) -> bool {
    rel.split(['/', std::path::MAIN_SEPARATOR])
        .any(|component| component == name)
}

#[derive(Clone, Debug)]
struct FileEntry {
    path: PathBuf,
    /// Path relative to the search root; this is what queries match against.
    rel: String,
    rel_lower: String,
    name_lower: String,
    /// Byte and char offsets of the file name within `rel`.
    name_byte: usize,
    name_char: usize,
    depth: u16,
}

impl FileEntry {
    fn new(path: PathBuf, root: &Path) -> Self {
        let rel_path = path.strip_prefix(root).unwrap_or(&path);
        let rel = rel_path.to_string_lossy().into_owned();
        let name_len = rel_path
            .file_name()
            .map(|name| name.to_string_lossy().len())
            .unwrap_or(rel.len())
            .min(rel.len());
        let mut name_byte = rel.len() - name_len;
        if !rel.is_char_boundary(name_byte) {
            name_byte = 0;
        }
        let name_char = rel[..name_byte].chars().count();
        let depth = rel_path.components().count().min(u16::MAX as usize) as u16;

        Self {
            rel_lower: rel.to_lowercase(),
            name_lower: rel[name_byte..].to_lowercase(),
            rel,
            name_byte,
            name_char,
            depth,
            path,
        }
    }

    fn name(&self) -> &str {
        &self.rel[self.name_byte..]
    }

    /// The parent part of `rel` without the trailing separator.
    fn dir(&self) -> &str {
        let dir = &self.rel[..self.name_byte];
        dir.strip_suffix(['/', std::path::MAIN_SEPARATOR])
            .unwrap_or(dir)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TermKind {
    /// `foo`: skim-style fuzzy match.
    Fuzzy,
    /// `'foo`: substring match.
    Exact,
    /// `^foo`: path or file name starts with.
    Prefix,
    /// `foo$`: path ends with.
    Suffix,
    /// `^foo$`: path or file name is exactly.
    Equal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Term {
    text: String,
    kind: TermKind,
    negate: bool,
    /// Smart case: a term is case-sensitive only when it contains uppercase.
    case_sensitive: bool,
}

impl Term {
    fn parse(word: &str) -> Option<Self> {
        let mut text = word;
        let mut negate = false;
        let mut kind = TermKind::Fuzzy;

        // Like fzf, `!term` is an inverse *exact* match.
        if let Some(rest) = text.strip_prefix('!') {
            negate = true;
            kind = TermKind::Exact;
            text = rest;
        }
        if let Some(rest) = text.strip_prefix('\'') {
            kind = TermKind::Exact;
            text = rest;
        } else if let Some(rest) = text.strip_prefix('^') {
            kind = TermKind::Prefix;
            text = rest;
        }
        if let Some(rest) = text.strip_suffix('$') {
            kind = if kind == TermKind::Prefix {
                TermKind::Equal
            } else {
                TermKind::Suffix
            };
            text = rest;
        }

        if text.is_empty() {
            return None;
        }

        Some(Self {
            case_sensitive: text.chars().any(char::is_uppercase),
            text: text.to_string(),
            kind,
            negate,
        })
    }

    fn haystacks<'a>(&self, entry: &'a FileEntry) -> (&'a str, &'a str) {
        if self.case_sensitive {
            (&entry.rel, entry.name())
        } else {
            (&entry.rel_lower, &entry.name_lower)
        }
    }

    fn score(&self, entry: &FileEntry, matcher: &SkimMatcherV2) -> Option<i64> {
        let len = self.text.chars().count() as i64;
        let (path, name) = self.haystacks(entry);
        match self.kind {
            TermKind::Fuzzy => {
                // The default skim matcher is smart-case already.
                let score = matcher.fuzzy_match(&entry.rel, &self.text)?;
                Some(score + matcher.fuzzy_match(entry.name(), &self.text).unwrap_or(0))
            }
            TermKind::Exact => {
                if !path.contains(self.text.as_str()) {
                    return None;
                }
                let mut score = EXACT_CHAR_SCORE * len;
                if name.starts_with(self.text.as_str()) {
                    score += NAME_CHAR_BONUS * len * 3 / 2;
                } else if name.contains(self.text.as_str()) {
                    score += NAME_CHAR_BONUS * len;
                }
                Some(score)
            }
            TermKind::Prefix => {
                if name.starts_with(self.text.as_str()) {
                    Some((ANCHOR_CHAR_SCORE + NAME_CHAR_BONUS) * len)
                } else if path.starts_with(self.text.as_str()) {
                    Some(ANCHOR_CHAR_SCORE * len)
                } else {
                    None
                }
            }
            TermKind::Suffix => path
                .ends_with(self.text.as_str())
                .then_some(ANCHOR_CHAR_SCORE * len),
            TermKind::Equal => (name == self.text || path == self.text)
                .then_some((ANCHOR_CHAR_SCORE + NAME_CHAR_BONUS) * len * 2),
        }
    }

    /// Char positions in `entry.rel` to highlight for this term.
    fn positions(&self, entry: &FileEntry, matcher: &SkimMatcherV2, out: &mut Vec<usize>) {
        if self.negate {
            return;
        }
        let len = self.text.chars().count();
        let (path, name) = self.haystacks(entry);
        let start = match self.kind {
            TermKind::Fuzzy => {
                // Prefer highlighting inside the file name, matching how the
                // name bonus is scored.
                if let Some((_, indices)) = matcher.fuzzy_indices(entry.name(), &self.text) {
                    out.extend(indices.into_iter().map(|i| i + entry.name_char));
                } else if let Some((_, indices)) = matcher.fuzzy_indices(&entry.rel, &self.text) {
                    out.extend(indices);
                }
                return;
            }
            TermKind::Exact => {
                if let Some(pos) = name.find(self.text.as_str()) {
                    entry.name_char + name[..pos].chars().count()
                } else if let Some(pos) = path.find(self.text.as_str()) {
                    path[..pos].chars().count()
                } else {
                    return;
                }
            }
            TermKind::Prefix | TermKind::Equal => {
                if name.starts_with(self.text.as_str()) {
                    entry.name_char
                } else {
                    0
                }
            }
            TermKind::Suffix => path.chars().count().saturating_sub(len),
        };
        out.extend(start..start + len);
    }
}

/// A parsed search query: whitespace-separated terms that must all match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Query {
    terms: Vec<Term>,
}

impl Query {
    fn parse(input: &str) -> Self {
        Self {
            terms: input.split_whitespace().filter_map(Term::parse).collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    fn score(&self, entry: &FileEntry, matcher: &SkimMatcherV2) -> Option<i64> {
        let mut total = 0;
        for term in &self.terms {
            match (term.score(entry, matcher), term.negate) {
                (Some(score), false) => total += score,
                (None, true) => {}
                _ => return None,
            }
        }
        Some(total)
    }

    fn positions(&self, entry: &FileEntry, matcher: &SkimMatcherV2) -> Vec<usize> {
        let mut positions = Vec::new();
        for term in &self.terms {
            term.positions(entry, matcher, &mut positions);
        }
        positions.sort_unstable();
        positions.dedup();
        positions
    }
}

/// Whether every match of `next` is guaranteed to be a match of `previous`, so
/// the previous match set can be searched instead of the whole index. Appending
/// to a query only narrows it unless a negation or `$` anchor is involved.
fn can_narrow(previous: &str, next: &str) -> bool {
    previous == next
        || (!previous.trim().is_empty()
            && next.starts_with(previous)
            && !next.contains('!')
            && !next.contains('$'))
}

struct MatchCache {
    query: String,
    /// Number of index entries the cached matches were computed over.
    scanned: usize,
    matches: Vec<(u32, i64)>,
}

#[derive(Debug, Default)]
struct SearchResults {
    /// Best matches first, as indices into the index.
    items: Vec<u32>,
    /// Total number of matches (may exceed `items.len()`).
    matched: usize,
}

struct SearchIndex {
    entries: Vec<FileEntry>,
    matcher: SkimMatcherV2,
    cache: Option<MatchCache>,
}

impl SearchIndex {
    fn new() -> Self {
        Self {
            entries: Vec::with_capacity(10_000),
            matcher: SkimMatcherV2::default(),
            cache: None,
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn entry(&self, idx: u32) -> &FileEntry {
        &self.entries[idx as usize]
    }

    fn extend(&mut self, batch: Vec<FileEntry>) {
        self.entries.extend(batch);
    }

    fn replace_path(&mut self, old_path: &Path, new_path: &Path, root: &Path) {
        let replacement = FileEntry::new(new_path.to_path_buf(), root);
        match self.entries.iter_mut().find(|entry| entry.path == old_path) {
            Some(entry) => *entry = replacement,
            None => self.entries.push(replacement),
        }
        self.cache = None;
    }

    fn search(&mut self, raw: &str, query: &Query, limit: usize) -> SearchResults {
        if query.is_empty() {
            self.cache = None;
            return self.browse(limit);
        }

        let entries = &self.entries;
        let matcher = &self.matcher;
        let score = |idx: usize| {
            query
                .score(&entries[idx], matcher)
                .map(|score| (idx as u32, score))
        };

        let matches: Vec<(u32, i64)> = match self.cache.take() {
            Some(cache) if can_narrow(&cache.query, raw) => {
                let mut matches: Vec<_> = cache
                    .matches
                    .par_iter()
                    .filter_map(|&(idx, _)| score(idx as usize))
                    .collect();
                matches.par_extend(
                    (cache.scanned..entries.len())
                        .into_par_iter()
                        .filter_map(score),
                );
                matches
            }
            _ => (0..entries.len())
                .into_par_iter()
                .filter_map(score)
                .collect(),
        };

        let mut ranked = matches.clone();
        let items = take_best(&mut ranked, limit, |a, b| {
            b.1.cmp(&a.1)
                .then_with(|| compare_paths(&entries[a.0 as usize], &entries[b.0 as usize]))
        });

        let matched = matches.len();
        self.cache = Some(MatchCache {
            query: raw.to_string(),
            scanned: entries.len(),
            matches,
        });
        SearchResults {
            items: items.into_iter().map(|(idx, _)| idx).collect(),
            matched,
        }
    }

    /// Results for an empty query: shallow paths first, then alphabetical.
    fn browse(&self, limit: usize) -> SearchResults {
        let entries = &self.entries;
        let mut ids: Vec<u32> = (0..entries.len() as u32).collect();
        let items = take_best(&mut ids, limit, |a, b| {
            let (a, b) = (&entries[*a as usize], &entries[*b as usize]);
            a.depth
                .cmp(&b.depth)
                .then_with(|| a.rel_lower.cmp(&b.rel_lower))
                .then_with(|| a.rel.cmp(&b.rel))
        });
        SearchResults {
            items,
            matched: entries.len(),
        }
    }
}

/// Shorter paths win ties, then plain alphabetical order.
fn compare_paths(a: &FileEntry, b: &FileEntry) -> Ordering {
    a.rel
        .len()
        .cmp(&b.rel.len())
        .then_with(|| a.rel.cmp(&b.rel))
}

/// Sorts only the best `limit` items instead of the whole set.
fn take_best<T, F>(items: &mut Vec<T>, limit: usize, compare: F) -> Vec<T>
where
    T: Send + Copy,
    F: Fn(&T, &T) -> Ordering + Sync,
{
    if items.len() > limit && limit > 0 {
        items.select_nth_unstable_by(limit, &compare);
        items.truncate(limit);
    }
    items.par_sort_unstable_by(&compare);
    std::mem::take(items)
}

fn spawn_indexer(
    root: PathBuf,
    respect_gitignore: bool,
    rules: Arc<IgnoreRules>,
    sender: Sender<Vec<FileEntry>>,
    indexed: Arc<AtomicUsize>,
    done: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let mut builder = WalkBuilder::new(&root);
        builder
            .hidden(false)
            .follow_links(false)
            .same_file_system(false)
            .git_ignore(respect_gitignore)
            .git_global(respect_gitignore)
            .git_exclude(respect_gitignore)
            .parents(respect_gitignore)
            .ignore(respect_gitignore)
            .require_git(false)
            .threads(num_cpus::get());

        // Skip ignored directories instead of walking them and dropping their
        // files afterwards (node_modules, target, ...).
        let filter_root = root.clone();
        let filter_rules = Arc::clone(&rules);
        builder.filter_entry(move |entry| {
            if entry.depth() == 0 || !entry.file_type().is_some_and(|ft| ft.is_dir()) {
                return true;
            }
            if respect_gitignore && entry.file_name() == ".git" {
                return false;
            }
            let rel = entry
                .path()
                .strip_prefix(&filter_root)
                .unwrap_or(entry.path())
                .to_string_lossy();
            !filter_rules.prunes_dir(&rel)
        });

        let mut visitors = IndexVisitorBuilder {
            root: &root,
            rules: &rules,
            sender: &sender,
            indexed: &indexed,
        };
        builder.build_parallel().visit(&mut visitors);
        done.store(true, AtomicOrdering::SeqCst);
    });
}

struct IndexVisitorBuilder<'a> {
    root: &'a Path,
    rules: &'a IgnoreRules,
    sender: &'a Sender<Vec<FileEntry>>,
    indexed: &'a AtomicUsize,
}

impl<'s> ParallelVisitorBuilder<'s> for IndexVisitorBuilder<'s> {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 's> {
        Box::new(IndexVisitor {
            root: self.root,
            rules: self.rules,
            sender: self.sender,
            indexed: self.indexed,
            batch: Vec::with_capacity(CHUNK_SIZE),
        })
    }
}

/// One per walker thread; batches entries locally and flushes on drop.
struct IndexVisitor<'a> {
    root: &'a Path,
    rules: &'a IgnoreRules,
    sender: &'a Sender<Vec<FileEntry>>,
    indexed: &'a AtomicUsize,
    batch: Vec<FileEntry>,
}

impl IndexVisitor<'_> {
    /// Returns false once the UI has gone away.
    fn flush(&mut self) -> bool {
        if self.batch.is_empty() {
            return true;
        }
        let count = self.batch.len();
        let batch = std::mem::replace(&mut self.batch, Vec::with_capacity(CHUNK_SIZE));
        let sent = self.sender.send(batch).is_ok();
        if sent {
            self.indexed.fetch_add(count, AtomicOrdering::SeqCst);
        }
        sent
    }
}

impl ParallelVisitor for IndexVisitor<'_> {
    fn visit(&mut self, entry: std::result::Result<ignore::DirEntry, ignore::Error>) -> WalkState {
        let Ok(entry) = entry else {
            return WalkState::Continue;
        };
        if entry.depth() == 0 || !entry.file_type().is_some_and(|ft| ft.is_file()) {
            return WalkState::Continue;
        }
        let file = FileEntry::new(entry.into_path(), self.root);
        if self.rules.matches(&file.rel) {
            return WalkState::Continue;
        }
        self.batch.push(file);
        if self.batch.len() >= CHUNK_SIZE && !self.flush() {
            return WalkState::Quit;
        }
        WalkState::Continue
    }
}

impl Drop for IndexVisitor<'_> {
    fn drop(&mut self) {
        self.flush();
    }
}

/// A single-line text input whose cursor is a char index, so editing is safe
/// for non-ASCII input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LineEditor {
    text: String,
    cursor: usize,
}

impl LineEditor {
    fn with_text(text: String) -> Self {
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    fn byte_index(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(self.text.len())
    }

    fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    fn before_cursor(&self) -> &str {
        &self.text[..self.byte_index(self.cursor)]
    }

    fn insert(&mut self, c: char) {
        let idx = self.byte_index(self.cursor);
        self.text.insert(idx, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        let idx = self.byte_index(self.cursor);
        self.text.remove(idx);
        true
    }

    fn delete(&mut self) -> bool {
        if self.cursor >= self.char_len() {
            return false;
        }
        let idx = self.byte_index(self.cursor);
        self.text.remove(idx);
        true
    }

    fn delete_word(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let mut start = self.cursor;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let (from, to) = (self.byte_index(start), self.byte_index(self.cursor));
        self.text.drain(from..to);
        self.cursor = start;
        true
    }

    /// Applies an editing key. `None` if the key is not an editing key,
    /// `Some(true)` if the text changed, `Some(false)` if only the cursor moved.
    fn handle_key(&mut self, key: &KeyEvent) -> Option<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('h') if ctrl => Some(self.backspace()),
            KeyCode::Char('w') if ctrl => Some(self.delete_word()),
            KeyCode::Backspace if alt => Some(self.delete_word()),
            KeyCode::Char('a') if ctrl => {
                self.cursor = 0;
                Some(false)
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                self.insert(c);
                Some(true)
            }
            KeyCode::Backspace => Some(self.backspace()),
            KeyCode::Delete => Some(self.delete()),
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                Some(false)
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.char_len());
                Some(false)
            }
            KeyCode::Home => {
                self.cursor = 0;
                Some(false)
            }
            KeyCode::End => {
                self.cursor = self.char_len();
                Some(false)
            }
            _ => None,
        }
    }
}

#[derive(Default)]
struct ResultList {
    items: Vec<u32>,
    matched: usize,
    selected: usize,
    offset: usize,
    height: usize,
    marked: HashSet<PathBuf>,
}

impl ResultList {
    fn selected_id(&self) -> Option<u32> {
        self.items.get(self.selected).copied()
    }

    /// Replaces the results. Unless `reset`, keeps the same entry selected
    /// when it is still present (new files arriving while indexing).
    fn set(&mut self, results: SearchResults, reset: bool) {
        let previous = if reset { None } else { self.selected_id() };
        self.items = results.items;
        self.matched = results.matched;
        self.selected = previous
            .and_then(|id| self.items.iter().position(|item| *item == id))
            .unwrap_or(if reset { 0 } else { self.selected });
        if reset {
            self.offset = 0;
        }
        self.clamp();
    }

    fn set_height(&mut self, height: usize) {
        self.height = height.max(1);
        self.clamp();
    }

    fn clamp(&mut self) {
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.height > 0 && self.selected >= self.offset + self.height {
            self.offset = self.selected + 1 - self.height;
        }
        let max_offset = self.items.len().saturating_sub(self.height.max(1));
        self.offset = self.offset.min(max_offset);
    }

    fn move_by(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() - 1;
        self.selected = self.selected.saturating_add_signed(delta).min(last);
        self.clamp();
    }

    fn page(&mut self, direction: isize) {
        let half = (self.height / 2).max(1) as isize;
        self.move_by(direction * half);
    }

    fn first(&mut self) {
        self.selected = 0;
        self.clamp();
    }

    fn last(&mut self) {
        self.selected = self.items.len().saturating_sub(1);
        self.clamp();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InputMode {
    Normal,
    Rename(LineEditor),
}

enum Action {
    None,
    Redraw,
    Search,
    Accept,
    Cancel,
    Edit(Vec<PathBuf>),
}

struct PreviewCache {
    path: PathBuf,
    height: usize,
    lines: Vec<String>,
}

struct App {
    root: PathBuf,
    index: SearchIndex,
    query: LineEditor,
    parsed: Query,
    results: ResultList,
    mode: InputMode,
    status: Option<(String, Instant)>,
    show_icons: bool,
    preview_enabled: bool,
    preview: Option<PreviewCache>,
    indexed: usize,
    indexing_done: bool,
    started: Instant,
}

impl App {
    fn new(root: PathBuf, show_icons: bool) -> Self {
        Self {
            root,
            index: SearchIndex::new(),
            query: LineEditor::default(),
            parsed: Query::default(),
            results: ResultList::default(),
            mode: InputMode::Normal,
            status: None,
            show_icons,
            preview_enabled: true,
            preview: None,
            indexed: 0,
            indexing_done: false,
            started: Instant::now(),
        }
    }

    fn refresh(&mut self, reset: bool) {
        self.parsed = Query::parse(&self.query.text);
        let results = self
            .index
            .search(&self.query.text, &self.parsed, MAX_RESULTS);
        self.results.set(results, reset);
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some((message.into(), Instant::now()));
    }

    /// Clears an expired status message; returns true if a redraw is needed.
    fn expire_status(&mut self) -> bool {
        let expired = self.status.as_ref().is_some_and(|(_, at)| {
            at.elapsed() >= Duration::from_millis(STATUS_MESSAGE_TIMEOUT_MS)
        });
        if expired {
            self.status = None;
        }
        expired
    }

    fn selected_entry(&self) -> Option<&FileEntry> {
        self.results.selected_id().map(|id| self.index.entry(id))
    }

    /// The marked paths, or the selected one when nothing is marked.
    fn target_paths(&self) -> Vec<PathBuf> {
        if self.results.marked.is_empty() {
            self.selected_entry()
                .map(|entry| vec![entry.path.clone()])
                .unwrap_or_default()
        } else {
            let mut paths: Vec<_> = self.results.marked.iter().cloned().collect();
            paths.sort();
            paths
        }
    }

    fn toggle_mark(&mut self) {
        if let Some(path) = self.selected_entry().map(|entry| entry.path.clone()) {
            if !self.results.marked.remove(&path) {
                self.results.marked.insert(path);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        if let InputMode::Rename(_) = self.mode {
            return self.handle_rename_key(key);
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Esc => Action::Cancel,
            KeyCode::Char('c') if ctrl => Action::Cancel,
            KeyCode::Enter => Action::Accept,
            KeyCode::Up => self.navigate(|list| list.move_by(-1)),
            KeyCode::Down => self.navigate(|list| list.move_by(1)),
            KeyCode::Char('k' | 'p') if ctrl => self.navigate(|list| list.move_by(-1)),
            KeyCode::Char('j' | 'n') if ctrl => self.navigate(|list| list.move_by(1)),
            KeyCode::PageUp => self.navigate(|list| list.page(-1)),
            KeyCode::PageDown => self.navigate(|list| list.page(1)),
            KeyCode::Char('u') if ctrl => self.navigate(|list| list.page(-1)),
            KeyCode::Char('d') if ctrl => self.navigate(|list| list.page(1)),
            KeyCode::Home if ctrl => self.navigate(ResultList::first),
            KeyCode::End if ctrl => self.navigate(ResultList::last),
            // Vim-like: ^G goes to the top, ^⇧G (or Alt-G, for terminals that
            // can't tell ^G and ^⇧G apart) to the bottom.
            KeyCode::Char('g' | 'G') if ctrl && shift => self.navigate(ResultList::last),
            KeyCode::Char('G') if ctrl => self.navigate(ResultList::last),
            KeyCode::Char('g') if ctrl => self.navigate(ResultList::first),
            KeyCode::Char('g' | 'G') if alt => self.navigate(ResultList::last),
            KeyCode::Tab => {
                self.toggle_mark();
                self.navigate(|list| list.move_by(1))
            }
            KeyCode::BackTab => {
                self.toggle_mark();
                self.navigate(|list| list.move_by(-1))
            }
            KeyCode::Char('t') if ctrl => {
                self.preview_enabled = !self.preview_enabled;
                Action::Redraw
            }
            KeyCode::Char(c) if ctrl && c.eq_ignore_ascii_case(&'y') => {
                let paths = self.target_paths();
                if !paths.is_empty() {
                    let content = paths
                        .iter()
                        .map(|path| path.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("\n");
                    match copy_to_clipboard(&content) {
                        Ok(()) => self.set_status(format!("Copied {} path(s)", paths.len())),
                        Err(e) => self.set_status(format!("Copy failed: {}", e)),
                    }
                }
                Action::Redraw
            }
            KeyCode::Char(c) if ctrl && c.eq_ignore_ascii_case(&'o') => {
                for path in self.target_paths() {
                    if let Err(e) = open_path(&path) {
                        self.set_status(format!("Open failed: {}", e));
                    }
                }
                Action::Redraw
            }
            KeyCode::Char('e') if ctrl => {
                let paths = self.target_paths();
                if paths.is_empty() {
                    Action::None
                } else {
                    Action::Edit(paths)
                }
            }
            KeyCode::F(2) => {
                if let Some(entry) = self.selected_entry() {
                    self.mode = InputMode::Rename(LineEditor::with_text(entry.name().to_string()));
                }
                Action::Redraw
            }
            _ => match self.query.handle_key(&key) {
                Some(true) => {
                    self.status = None;
                    Action::Search
                }
                Some(false) => Action::Redraw,
                None => Action::None,
            },
        }
    }

    fn navigate(&mut self, f: impl FnOnce(&mut ResultList)) -> Action {
        f(&mut self.results);
        Action::Redraw
    }

    fn handle_rename_key(&mut self, key: KeyEvent) -> Action {
        let InputMode::Rename(editor) = &mut self.mode else {
            return Action::None;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.set_status("Rename cancelled");
            }
            KeyCode::Char('c') if ctrl => {
                self.mode = InputMode::Normal;
                self.set_status("Rename cancelled");
            }
            KeyCode::Char('e') if ctrl => editor.cursor = editor.char_len(),
            KeyCode::Enter => {
                let new_name = editor.text.trim().to_string();
                self.mode = InputMode::Normal;
                self.rename_selected(&new_name);
            }
            _ => {
                editor.handle_key(&key);
            }
        }
        Action::Redraw
    }

    fn rename_selected(&mut self, new_name: &str) {
        let Some(old_path) = self.selected_entry().map(|entry| entry.path.clone()) else {
            return;
        };
        let unchanged = old_path
            .file_name()
            .is_some_and(|name| name.to_string_lossy() == new_name);
        if new_name.is_empty() || unchanged {
            return;
        }
        match rename_file(&old_path, new_name) {
            Ok(new_path) => {
                if self.results.marked.remove(&old_path) {
                    self.results.marked.insert(new_path.clone());
                }
                self.index.replace_path(&old_path, &new_path, &self.root);
                self.refresh(false);
                self.set_status(format!("Renamed to {}", new_path.display()));
            }
            Err(e) => self.set_status(format!("Rename failed: {}", e)),
        }
    }

    fn accept(&self) -> Vec<PathBuf> {
        self.target_paths()
    }

    fn draw(&mut self, out: &mut impl Write) -> io::Result<()> {
        let (width, height) = terminal::size()?;
        let (width, height) = (width as usize, height as usize);
        if width < 10 || height < 4 {
            return Ok(());
        }
        let list_height = height - 3;
        self.results.set_height(list_height);

        let preview_width = if self.preview_enabled && width >= PREVIEW_MIN_WIDTH {
            width * 45 / 100
        } else {
            0
        };
        let list_width = if preview_width > 0 {
            width - preview_width - 2
        } else {
            width
        };

        queue!(out, cursor::Hide)?;
        let prompt = self.render_prompt(width);
        queue!(out, cursor::MoveTo(0, 0), Print(fit(&prompt, width)))?;
        queue!(
            out,
            cursor::MoveTo(0, 1),
            Print("─".repeat(width).bright_black())
        )?;

        let rows = self.render_rows(list_width);
        let preview = if preview_width > 0 {
            self.preview_lines(list_height)
        } else {
            Vec::new()
        };
        for row in 0..list_height {
            queue!(out, cursor::MoveTo(0, (row + 2) as u16))?;
            let line = rows.get(row).map(String::as_str).unwrap_or("");
            queue!(out, Print(fit(line, list_width)))?;
            if preview_width > 0 {
                let text = preview.get(row).map(String::as_str).unwrap_or("");
                queue!(
                    out,
                    Print(" │".bright_black()),
                    Print(fit(text, preview_width))
                )?;
            }
        }

        let (status, status_cursor) = self.render_status(width);
        queue!(
            out,
            cursor::MoveTo(0, (height - 1) as u16),
            Print(fit(&status, width - 1)),
            terminal::Clear(ClearType::UntilNewLine)
        )?;

        let (col, row) = match status_cursor {
            Some(col) => (col, height - 1),
            None => (
                prompt_width() + console::measure_text_width(self.query.before_cursor()),
                0,
            ),
        };
        queue!(
            out,
            cursor::MoveTo(col.min(width - 1) as u16, row as u16),
            cursor::Show
        )?;
        out.flush()
    }

    fn render_prompt(&self, width: usize) -> String {
        let theme = get_theme();
        let accent = color_value_to_color(&theme.colors.permission_exec);
        let input = if self.query.text.is_empty() {
            "Type to search  ('exact ^prefix suffix$ !exclude)"
                .bright_black()
                .to_string()
        } else {
            self.query
                .text
                .color(color_value_to_color(&theme.colors.file))
                .bold()
                .to_string()
        };

        let spinner = if self.indexing_done {
            String::new()
        } else {
            let frame = (self.started.elapsed().as_millis() / SPINNER_INTERVAL_MS as u128) as usize
                % SPINNER_FRAMES.len();
            format!("{} ", SPINNER_FRAMES[frame].color(accent))
        };
        let counter = format!(
            "{}{} ",
            spinner,
            format!("{}/{}", self.results.matched, self.index.len()).bright_black()
        );

        let left = format!("{}{}", PROMPT.color(accent).bold(), input);
        let room = width.saturating_sub(console::measure_text_width(&counter));
        format!("{}{}", fit(&left, room), counter)
    }

    fn render_rows(&self, width: usize) -> Vec<String> {
        let theme = get_theme();
        if self.results.items.is_empty() {
            let message = if !self.indexing_done && self.index.len() == 0 {
                format!("Indexing files... {} found", self.indexed)
            } else if self.index.len() == 0 {
                "No files found".to_string()
            } else {
                format!("No matches ({} files indexed)", self.index.len())
            };
            return vec![format!(
                "  {}",
                message.color(color_value_to_color(&theme.colors.permission_none))
            )];
        }

        let show_meta = width >= 70;
        let show_perms = width >= 100;
        let window = self
            .results
            .items
            .iter()
            .enumerate()
            .skip(self.results.offset)
            .take(self.results.height);

        window
            .map(|(idx, id)| {
                let entry = self.index.entry(*id);
                let positions = self.parsed.positions(entry, &self.index.matcher);
                let selected = idx == self.results.selected;
                let marked = self.results.marked.contains(&entry.path);
                let meta = if show_meta {
                    entry_meta(&entry.path, show_perms)
                } else {
                    String::new()
                };
                render_row(
                    entry,
                    &positions,
                    selected,
                    marked,
                    self.show_icons,
                    &meta,
                    width,
                )
            })
            .collect()
    }

    /// The status line, plus the cursor column when the cursor belongs there.
    fn render_status(&self, width: usize) -> (String, Option<usize>) {
        let hint = |s: &str| s.bright_black().to_string();
        match &self.mode {
            InputMode::Rename(editor) => {
                let label = " RENAME ".bold().yellow().to_string();
                let line = format!(
                    "{} {}  {}",
                    label,
                    editor.text.bold(),
                    hint("enter confirm · esc cancel")
                );
                let col = console::measure_text_width(&label)
                    + 1
                    + console::measure_text_width(editor.before_cursor());
                (line, Some(col))
            }
            InputMode::Normal => {
                let marked = if self.results.marked.is_empty() {
                    String::new()
                } else {
                    format!(
                        "{} ",
                        format!(" {} marked ", self.results.marked.len())
                            .bold()
                            .yellow()
                    )
                };
                let body = match &self.status {
                    Some((message, _)) => message.green().to_string(),
                    None => {
                        let preview_hint = if width >= PREVIEW_MIN_WIDTH {
                            " ^t preview ·"
                        } else {
                            ""
                        };
                        hint(&format!(
                            " ↵ select · tab mark ·{} ^e edit · F2 rename · ^y copy · ^o open · ^g/M-g top/end · esc quit",
                            preview_hint
                        ))
                    }
                };
                (format!("{}{}", marked, body), None)
            }
        }
    }

    fn preview_lines(&mut self, height: usize) -> Vec<String> {
        let Some(path) = self.selected_entry().map(|entry| entry.path.clone()) else {
            return Vec::new();
        };
        let cached = self
            .preview
            .as_ref()
            .is_some_and(|cache| cache.path == path && cache.height == height);
        if !cached {
            self.preview = Some(PreviewCache {
                lines: build_preview(&path, height),
                path,
                height,
            });
        }
        self.preview
            .as_ref()
            .map(|cache| cache.lines.clone())
            .unwrap_or_default()
    }
}

const PROMPT: &str = " ❯ ";

fn prompt_width() -> usize {
    console::measure_text_width(PROMPT)
}

/// Truncates (ANSI-aware) and pads `text` to exactly `width` columns, so rows
/// overwrite the previous frame without clearing the screen.
fn fit(text: &str, width: usize) -> String {
    let measured = console::measure_text_width(text);
    if measured > width {
        // `truncate_str` also cuts text that fits exactly, so only call it
        // when the text is really too wide.
        return console::truncate_str(text, width, "…").into_owned();
    }
    format!("{}{}", text, " ".repeat(width - measured))
}

/// Splits `text` into runs of highlighted / plain chars and styles each run.
/// `positions` are sorted char indices relative to `offset`.
fn highlight_runs(
    text: &str,
    offset: usize,
    positions: &[usize],
    plain: impl Fn(&str) -> String,
    highlighted: impl Fn(&str) -> String,
) -> String {
    let mut out = String::new();
    let mut run = String::new();
    let mut run_highlighted = false;
    for (idx, c) in text.chars().enumerate() {
        let is_highlighted = positions.binary_search(&(idx + offset)).is_ok();
        if is_highlighted != run_highlighted && !run.is_empty() {
            out.push_str(&if run_highlighted {
                highlighted(&run)
            } else {
                plain(&run)
            });
            run.clear();
        }
        run_highlighted = is_highlighted;
        run.push(c);
    }
    if !run.is_empty() {
        out.push_str(&if run_highlighted {
            highlighted(&run)
        } else {
            plain(&run)
        });
    }
    out
}

fn entry_meta(path: &Path, show_perms: bool) -> String {
    let Ok(metadata) = path.metadata() else {
        return String::new();
    };
    let modified = metadata
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let size = format!("{:>7}", colorize_size(metadata.len()));
    let date = colorize_date(&modified);
    if show_perms {
        let perms = colorize_permissions(fs_metadata::permission_mode(&metadata), Some("symbolic"));
        format!("{}  {}  {}", perms, size, date)
    } else {
        format!("{}  {}", size, date)
    }
}

fn render_row(
    entry: &FileEntry,
    positions: &[usize],
    selected: bool,
    marked: bool,
    show_icons: bool,
    meta: &str,
    width: usize,
) -> String {
    let theme = get_theme();
    let accent = color_value_to_color(&theme.colors.permission_exec);
    let base = colorize_file_name(&entry.path).fgcolor();

    let pointer = if selected {
        "→".color(accent).bold().to_string()
    } else {
        " ".to_string()
    };
    let mark = if marked {
        "●".yellow().bold().to_string()
    } else {
        " ".to_string()
    };

    let hl = |s: &str| s.color(accent).bold().underline().to_string();
    let name = highlight_runs(
        entry.name(),
        entry.name_char,
        positions,
        |s| {
            let styled = match base {
                Some(color) => s.color(color),
                None => s.normal(),
            };
            if selected {
                styled.bold().to_string()
            } else {
                styled.to_string()
            }
        },
        hl,
    );
    let name = format_with_icon(&entry.path, name, show_icons);
    let dir = highlight_runs(
        entry.dir(),
        0,
        positions,
        |s| {
            if selected {
                s.color(color_value_to_color(&theme.colors.directory))
                    .to_string()
            } else {
                s.bright_black().to_string()
            }
        },
        hl,
    );

    let left = if dir.is_empty() {
        format!(" {}{} {}", pointer, mark, name)
    } else {
        format!(" {}{} {}  {}", pointer, mark, name, dir)
    };
    if meta.is_empty() {
        return left;
    }
    let meta_width = console::measure_text_width(meta);
    let room = width.saturating_sub(meta_width + 2);
    format!("{}  {}", fit(&left, room), meta)
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0)
}

/// Expands tabs and drops control characters, so file content cannot emit
/// terminal escape sequences into the UI.
fn sanitize_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        match c {
            '\t' => out.push_str("    "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn build_preview(path: &Path, height: usize) -> Vec<String> {
    let dim = |s: String| s.bright_black().to_string();
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(e) => return vec![dim(format!("cannot preview: {}", e))],
    };

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let modified = metadata
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let mut lines = vec![
        format!(
            "{}  {}  {}",
            name.bold(),
            colorize_size(metadata.len()),
            colorize_date(&modified)
        ),
        String::new(),
    ];

    let mut buffer = Vec::new();
    let read = std::fs::File::open(path)
        .and_then(|file| file.take(PREVIEW_MAX_BYTES).read_to_end(&mut buffer));
    if let Err(e) = read {
        lines.push(dim(format!("cannot preview: {}", e)));
        return lines;
    }
    if buffer.is_empty() {
        lines.push(dim("empty file".to_string()));
    } else if is_binary(&buffer) {
        lines.push(dim("binary file".to_string()));
    } else {
        let room = height.saturating_sub(lines.len());
        lines.extend(
            String::from_utf8_lossy(&buffer)
                .lines()
                .take(room)
                .map(sanitize_line),
        );
    }
    lines
}

/// Owns the terminal while the finder is open and restores it on drop, even
/// when an error unwinds out of the event loop.
struct TerminalSession {
    out: BufWriter<Box<dyn Write>>,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut session = Self {
            out: BufWriter::with_capacity(64 * 1024, tty_writer()),
        };
        execute!(
            session.out,
            terminal::EnterAlternateScreen,
            terminal::Clear(ClearType::All)
        )?;
        Ok(session)
    }

    fn suspend(&mut self) -> io::Result<()> {
        execute!(self.out, terminal::LeaveAlternateScreen, cursor::Show)?;
        terminal::disable_raw_mode()
    }

    fn resume(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        execute!(
            self.out,
            terminal::EnterAlternateScreen,
            terminal::Clear(ClearType::All)
        )
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = execute!(self.out, terminal::LeaveAlternateScreen, cursor::Show);
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Clone)]
pub struct FuzzyLister {
    config: crate::config::Config,
    respect_gitignore: bool,
    show_icons: bool,
}

impl FuzzyLister {
    pub fn new(config: crate::config::Config, respect_gitignore: bool, show_icons: bool) -> Self {
        Self {
            show_icons,
            config,
            respect_gitignore,
        }
    }

    fn run_interactive(&self, directory: &str) -> Result<Vec<PathBuf>> {
        let root = PathBuf::from(directory);
        let rules = Arc::new(IgnoreRules::new(&self.config.listers.fuzzy.ignore_patterns));
        let (sender, receiver) = bounded::<Vec<FileEntry>>(256);
        let indexed = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicBool::new(false));
        spawn_indexer(
            root.clone(),
            self.respect_gitignore,
            rules,
            sender,
            Arc::clone(&indexed),
            Arc::clone(&done),
        );

        let mut session = TerminalSession::enter()?;
        let mut app = App::new(root, self.show_icons);
        let poll_interval = Duration::from_millis(POLL_INTERVAL_MS);
        let batch_interval = Duration::from_millis(BATCH_CHECK_MS);
        let spinner_interval = Duration::from_millis(SPINNER_INTERVAL_MS);
        let mut last_batch: Option<Instant> = None;
        let mut last_draw = Instant::now();
        let mut pending_search = false;
        let mut dirty = true;

        loop {
            if last_batch.is_none_or(|at| at.elapsed() >= batch_interval) {
                let mut received = false;
                while let Ok(batch) = receiver.try_recv() {
                    app.index.extend(batch);
                    received = true;
                }
                app.indexed = indexed.load(AtomicOrdering::SeqCst);
                let finished = done.load(AtomicOrdering::SeqCst) && receiver.is_empty();
                if received {
                    app.refresh(false);
                    dirty = true;
                }
                if finished != app.indexing_done {
                    app.indexing_done = finished;
                    dirty = true;
                }
                last_batch = Some(Instant::now());
            }

            // Wait for input, then drain everything already queued so a burst
            // of keystrokes triggers a single search and a single redraw.
            if event::poll(poll_interval)? {
                loop {
                    match event::read()? {
                        Event::Key(key) if key.kind != KeyEventKind::Release => {
                            match app.handle_key(key) {
                                Action::None => {}
                                Action::Redraw => dirty = true,
                                Action::Search => {
                                    pending_search = true;
                                    dirty = true;
                                }
                                Action::Accept => return Ok(app.accept()),
                                Action::Cancel => return Ok(Vec::new()),
                                Action::Edit(paths) => {
                                    session.suspend()?;
                                    let opened = open_in_editor(
                                        &paths,
                                        self.config.listers.fuzzy.editor.as_deref(),
                                    );
                                    session.resume()?;
                                    match opened {
                                        Ok(()) => app.set_status(format!(
                                            "Opened editor for {} file(s)",
                                            paths.len()
                                        )),
                                        Err(e) => {
                                            app.set_status(format!("Failed to open editor: {}", e))
                                        }
                                    }
                                    dirty = true;
                                }
                            }
                        }
                        Event::Resize(_, _) => dirty = true,
                        _ => {}
                    }
                    if !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }

            if pending_search {
                app.refresh(true);
                pending_search = false;
                dirty = true;
            }
            if app.expire_status() {
                dirty = true;
            }
            if !app.indexing_done && last_draw.elapsed() >= spinner_interval {
                dirty = true;
            }
            if dirty {
                app.draw(&mut session.out)?;
                last_draw = Instant::now();
                dirty = false;
            }
        }
    }
}

impl FileLister for FuzzyLister {
    fn list_files(
        &self,
        directory: &str,
        _recursive: bool,
        _depth: Option<usize>,
    ) -> Result<Vec<PathBuf>> {
        self.run_interactive(directory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(rel: &str) -> FileEntry {
        FileEntry::new(Path::new("/root").join(rel), Path::new("/root"))
    }

    fn index_of(paths: &[&str]) -> SearchIndex {
        let mut index = SearchIndex::new();
        index.extend(paths.iter().map(|p| entry(p)).collect());
        index
    }

    fn search(index: &mut SearchIndex, raw: &str) -> Vec<String> {
        let query = Query::parse(raw);
        index
            .search(raw, &query, MAX_RESULTS)
            .items
            .into_iter()
            .map(|id| index.entry(id).rel.clone())
            .collect()
    }

    #[test]
    fn file_entry_splits_name_and_dir() {
        let e = entry("src/über/main.rs");
        assert_eq!(e.name(), "main.rs");
        assert_eq!(e.dir(), "src/über");
        assert_eq!(e.name_char, "src/über/".chars().count());
        assert_eq!(e.depth, 3);
        let top = entry("Cargo.toml");
        assert_eq!(top.dir(), "");
        assert_eq!(top.name_char, 0);
    }

    #[test]
    fn parses_term_kinds() {
        let q = Query::parse("foo 'bar ^baz qux$ ^exact$ !neg !^np");
        let kinds: Vec<_> = q
            .terms
            .iter()
            .map(|t| (t.text.as_str(), t.kind, t.negate))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("foo", TermKind::Fuzzy, false),
                ("bar", TermKind::Exact, false),
                ("baz", TermKind::Prefix, false),
                ("qux", TermKind::Suffix, false),
                ("exact", TermKind::Equal, false),
                ("neg", TermKind::Exact, true),
                ("np", TermKind::Prefix, true),
            ]
        );
    }

    #[test]
    fn bare_operators_are_ignored() {
        assert!(Query::parse("! ' ^ $  ").is_empty());
    }

    #[test]
    fn smart_case() {
        let q = Query::parse("readme README");
        assert!(!q.terms[0].case_sensitive);
        assert!(q.terms[1].case_sensitive);

        let mut index = index_of(&["README.md", "readme.txt"]);
        assert_eq!(search(&mut index, "'readme").len(), 2);
        assert_eq!(search(&mut index, "'README"), vec!["README.md"]);
        assert_eq!(search(&mut index, "README").len(), 1);
    }

    #[test]
    fn terms_are_anded_and_negation_excludes() {
        let mut index = index_of(&[
            "src/main.rs",
            "src/lib.rs",
            "tests/main_test.rs",
            "docs/main.md",
        ]);
        let mut results = search(&mut index, "main .rs$");
        results.sort();
        assert_eq!(results, vec!["src/main.rs", "tests/main_test.rs"]);
        assert_eq!(search(&mut index, "main .rs$ !test"), vec!["src/main.rs"]);
        assert_eq!(search(&mut index, "^src lib"), vec!["src/lib.rs"]);
        assert_eq!(search(&mut index, "^main.md$"), vec!["docs/main.md"]);
    }

    #[test]
    fn file_name_matches_rank_above_directory_matches() {
        let mut index = index_of(&["config/zzz.txt", "src/config.rs"]);
        assert_eq!(search(&mut index, "config")[0], "src/config.rs");
    }

    #[test]
    fn shorter_paths_win_ties() {
        let mut index = index_of(&["a/b/c/notes.md", "notes.md"]);
        assert_eq!(search(&mut index, "'notes")[0], "notes.md");
    }

    #[test]
    fn narrowing_finds_matches_beyond_the_result_limit() {
        // More matches for "a" than MAX_RESULTS, with the only "ab" match
        // ranked last for "a".
        let mut paths: Vec<String> = (0..MAX_RESULTS + 500).map(|i| format!("a{i}")).collect();
        paths.push("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz/x_a_y_b".to_string());
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let mut index = index_of(&refs);

        let first = Query::parse("a");
        assert_eq!(index.search("a", &first, MAX_RESULTS).matched, refs.len());
        let narrowed = search(&mut index, "ab");
        assert_eq!(
            narrowed,
            vec!["zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz/x_a_y_b"]
        );
    }

    #[test]
    fn narrowing_includes_entries_indexed_later() {
        let mut index = index_of(&["one.rs"]);
        assert_eq!(search(&mut index, "rs").len(), 1);
        index.extend(vec![entry("two.rs")]);
        assert_eq!(search(&mut index, "rs").len(), 2);
        assert_eq!(search(&mut index, "rs ").len(), 2);
    }

    #[test]
    fn negation_and_anchors_force_full_rescan() {
        assert!(can_narrow("ma", "mai"));
        assert!(can_narrow("main", "main rs"));
        assert!(can_narrow("!x", "!x"));
        assert!(!can_narrow("!x", "!xy"));
        assert!(!can_narrow("rs", "rs$"));
        assert!(!can_narrow("", "a"));
        assert!(!can_narrow("ab", "a"));

        let mut index = index_of(&["x.rs", "xy.rs", "z.rs"]);
        assert_eq!(search(&mut index, "!x").len(), 1);
        assert_eq!(search(&mut index, "!xy").len(), 2);
    }

    #[test]
    fn empty_query_lists_shallow_paths_first() {
        let mut index = index_of(&["b/deep.rs", "z.rs", "a/b/c.rs", "a.rs"]);
        assert_eq!(
            search(&mut index, ""),
            vec!["a.rs", "z.rs", "b/deep.rs", "a/b/c.rs"]
        );
        assert_eq!(index.search("", &Query::default(), 2).matched, 4);
        assert_eq!(index.search("", &Query::default(), 2).items.len(), 2);
    }

    #[test]
    fn highlight_positions_prefer_the_file_name() {
        let index = index_of(&[]);
        let e = entry("main/src/main.rs");
        let positions = Query::parse("main").positions(&e, &index.matcher);
        assert_eq!(positions, (9..13).collect::<Vec<_>>());
        let positions = Query::parse("'src").positions(&e, &index.matcher);
        assert_eq!(positions, (5..8).collect::<Vec<_>>());
        let positions = Query::parse(".rs$").positions(&e, &index.matcher);
        assert_eq!(positions, (13..16).collect::<Vec<_>>());
        assert!(Query::parse("!main")
            .positions(&e, &index.matcher)
            .is_empty());
    }

    #[test]
    fn highlight_runs_groups_consecutive_chars() {
        let out = highlight_runs(
            "main.rs",
            10,
            &[10, 11, 15],
            |s| s.to_string(),
            |s| format!("[{s}]"),
        );
        assert_eq!(out, "[ma]in.[r]s");
        let out = highlight_runs("über", 0, &[0], |s| s.to_string(), |s| format!("[{s}]"));
        assert_eq!(out, "[ü]ber");
    }

    #[test]
    fn line_editor_is_char_safe() {
        let mut editor = LineEditor::default();
        for c in "héllo 🦀".chars() {
            editor.insert(c);
        }
        assert_eq!(editor.text, "héllo 🦀");
        assert_eq!(editor.cursor, 7);
        assert!(editor.backspace());
        assert_eq!(editor.text, "héllo ");
        editor.cursor = 2;
        assert!(editor.backspace());
        assert_eq!(editor.text, "hllo ");
        editor.insert('é');
        assert_eq!(editor.text, "héllo ");
        assert_eq!(editor.before_cursor(), "hé");
        assert!(editor.delete());
        assert_eq!(editor.text, "hélo ");
    }

    #[test]
    fn line_editor_delete_word() {
        let mut editor = LineEditor::with_text("src/ünï  main".to_string());
        assert!(editor.delete_word());
        assert_eq!(editor.text, "src/ünï  ");
        assert!(editor.delete_word());
        assert_eq!(editor.text, "");
        assert!(!editor.delete_word());
    }

    #[test]
    fn line_editor_keys() {
        let key = |code, modifiers| KeyEvent::new(code, modifiers);
        let mut editor = LineEditor::with_text("ab".to_string());
        assert_eq!(
            editor.handle_key(&key(KeyCode::Home, KeyModifiers::NONE)),
            Some(false)
        );
        assert_eq!(
            editor.handle_key(&key(KeyCode::Char('X'), KeyModifiers::SHIFT)),
            Some(true)
        );
        assert_eq!(editor.text, "Xab");
        assert_eq!(
            editor.handle_key(&key(KeyCode::Char('e'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            editor.handle_key(&key(KeyCode::End, KeyModifiers::NONE)),
            Some(false)
        );
        assert_eq!(editor.cursor, 3);
    }

    #[test]
    fn result_list_keeps_selection_when_results_grow() {
        let mut list = ResultList::default();
        list.set_height(2);
        list.set(
            SearchResults {
                items: vec![1, 2, 3],
                matched: 3,
            },
            true,
        );
        list.move_by(2);
        assert_eq!(list.selected_id(), Some(3));
        assert_eq!(list.offset, 1);
        list.set(
            SearchResults {
                items: vec![0, 1, 2, 3],
                matched: 4,
            },
            false,
        );
        assert_eq!(list.selected_id(), Some(3));
        list.set(
            SearchResults {
                items: vec![5],
                matched: 1,
            },
            true,
        );
        assert_eq!(list.selected, 0);
        assert_eq!(list.offset, 0);
        list.move_by(-5);
        assert_eq!(list.selected, 0);
    }

    #[test]
    fn ignore_rules_match_relative_paths() {
        let rules = IgnoreRules::new(&[
            "node_modules".to_string(),
            "glob:*.min.js".to_string(),
            "regex:\\.pyc$".to_string(),
            String::new(),
            "regex:(".to_string(),
        ]);
        assert!(rules.matches("web/node_modules/x.js"));
        assert!(rules.matches("dist/app.min.js"));
        assert!(rules.matches("pkg/mod.pyc"));
        assert!(!rules.matches("src/main.rs"));
        assert!(rules.prunes_dir("web/node_modules"));
        assert!(!rules.prunes_dir("dist"));
    }

    #[test]
    fn fit_pads_and_truncates_to_the_exact_width() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdef", 6), "abcdef");
        assert_eq!(fit("abcdefg", 6), "abcde…");
        assert_eq!(console::measure_text_width(&fit("über🦀", 4)), 4);
    }

    #[test]
    fn preview_helpers() {
        assert!(is_binary(b"abc\0def"));
        assert!(!is_binary(b"plain text"));
        let rules = IgnoreRules::new(&[".git".to_string(), "target".to_string()]);
        assert!(rules.matches(".git/HEAD"));
        assert!(rules.matches("crate/target/debug/x"));
        assert!(!rules.matches(".github/workflows/ci.yml"));
        assert!(!rules.matches("src/targets.rs"));
        assert_eq!(sanitize_line("a\tb\x1b[31mc"), "a    b[31mc");
    }

    #[test]
    fn rename_refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        assert_eq!(
            rename_file(&a, "b.txt").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(rename_file(&a, "sub/c.txt").is_err());
        let renamed = rename_file(&a, "c.txt").unwrap();
        assert_eq!(std::fs::read_to_string(renamed).unwrap(), "a");
    }
}
