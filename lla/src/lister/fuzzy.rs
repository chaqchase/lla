#![cfg_attr(test, allow(dead_code))]

use super::graphics::{self, Protocol};
use super::grep;
use super::index_cache;
use super::preview;
use super::FileLister;
use crate::error::Result;
use crate::theme::palette::{self, Tone};
use crate::utils::color::*;
use crate::utils::fs_metadata;
use crate::utils::icons::format_with_icon;
use colored::*;
use crossbeam_channel::{bounded, never, select, unbounded, Receiver, Sender, TryRecvError};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, queue,
    style::Print,
    terminal::{self, ClearType},
};
use ignore::{ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};
use nucleo_matcher::{Config as MatchConfig, Matcher, Utf32Str};
use rayon::prelude::*;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

const CHUNK_SIZE: usize = 1000;
const MAX_RESULTS: usize = 2000;
/// How many greedy matches per result get rescored with the optimal matcher.
const RESCORE_FACTOR: usize = 4;
/// Files searched between checks for a newer query in text mode.
const TEXT_CHUNK: usize = 256;
/// How often text search shows what it has found so far.
const TEXT_PARTIAL_MS: u64 = 120;
const TEXT_MIN_CHARS: usize = 2;
const POLL_INTERVAL_MS: u64 = 16;
const BATCH_CHECK_MS: u64 = 100;
const SPINNER_INTERVAL_MS: u64 = 80;
const STATUS_MESSAGE_TIMEOUT_MS: u64 = 2000;
const PREVIEW_MIN_WIDTH: usize = 120;
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

fn open_in_editor(
    paths: &[PathBuf],
    line: Option<u32>,
    config_editor: Option<&str>,
) -> std::io::Result<()> {
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
    match (line, paths) {
        (Some(line), [path]) => {
            cmd.args(editor_line_args(program, path, line));
        }
        _ => {
            cmd.args(paths);
        }
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

/// Arguments that open `path` at `line`, in the form the editor understands.
fn editor_line_args(program: &str, path: &Path, line: u32) -> Vec<std::ffi::OsString> {
    let name = Path::new(program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let with_line = || {
        let mut arg = path.as_os_str().to_os_string();
        arg.push(format!(":{line}"));
        arg
    };
    match name.as_str() {
        "vi" | "vim" | "nvim" | "gvim" | "mvim" | "nano" | "pico" | "emacs" | "emacsclient"
        | "micro" | "kak" | "joe" | "ne" | "mg" => {
            vec![format!("+{line}").into(), path.as_os_str().to_os_string()]
        }
        "code" | "code-insiders" | "codium" | "cursor" | "windsurf" => {
            vec!["-g".into(), with_line()]
        }
        "hx" | "helix" | "zed" | "subl" | "sublime_text" | "mate" => vec![with_line()],
        _ => vec![path.as_os_str().to_os_string()],
    }
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
    folder: bool,
}

impl FileEntry {
    fn folder(path: PathBuf, root: &Path) -> Self {
        Self {
            folder: true,
            ..Self::new(path, root)
        }
    }

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
            folder: false,
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
    /// `text` as the matcher wants it: lowercased unless case-sensitive.
    needle: String,
    needle_chars: Vec<char>,
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

        let case_sensitive = text.chars().any(char::is_uppercase);
        let needle = if case_sensitive {
            text.to_string()
        } else {
            text.to_lowercase()
        };
        Some(Self {
            case_sensitive,
            text: text.to_string(),
            kind,
            negate,
            needle_chars: needle.chars().collect(),
            needle,
        })
    }

    /// nucleo treats a Unicode needle as non-ASCII, so ASCII must stay ASCII.
    fn needle(&self) -> Utf32Str<'_> {
        if self.needle.is_ascii() {
            Utf32Str::Ascii(self.needle.as_bytes())
        } else {
            Utf32Str::Unicode(&self.needle_chars)
        }
    }

    fn haystacks<'a>(&self, entry: &'a FileEntry) -> (&'a str, &'a str) {
        if self.case_sensitive {
            (&entry.rel, entry.name())
        } else {
            (&entry.rel_lower, &entry.name_lower)
        }
    }

    /// `precise` picks the optimal alignment. The greedy one matches exactly
    /// the same entries several times faster, so it is used to filter.
    fn score(
        &self,
        entry: &FileEntry,
        matcher: &mut Matcher,
        buf: &mut Vec<char>,
        precise: bool,
    ) -> Option<i64> {
        let len = self.text.chars().count() as i64;
        let (path, name) = self.haystacks(entry);
        match self.kind {
            TermKind::Fuzzy => {
                matcher.config.ignore_case = !self.case_sensitive;
                let mut fuzzy = |haystack: &str, buf: &mut Vec<char>| {
                    let haystack = Utf32Str::new(haystack, buf);
                    if precise {
                        matcher.fuzzy_match(haystack, self.needle())
                    } else {
                        matcher.fuzzy_match_greedy(haystack, self.needle())
                    }
                };
                let score = fuzzy(&entry.rel, buf)?;
                // Matches inside the file name count twice.
                let in_name = fuzzy(entry.name(), buf).unwrap_or(0);
                Some(i64::from(score) + i64::from(in_name))
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
    fn positions(&self, entry: &FileEntry, matcher: &mut Matcher, out: &mut Vec<usize>) {
        if self.negate {
            return;
        }
        let len = self.text.chars().count();
        let (path, name) = self.haystacks(entry);
        let start = match self.kind {
            TermKind::Fuzzy => {
                // Prefer highlighting inside the file name, matching how the
                // name bonus is scored.
                matcher.config.ignore_case = !self.case_sensitive;
                let mut buf = Vec::new();
                let mut indices = Vec::new();
                if matcher
                    .fuzzy_indices(
                        Utf32Str::new(entry.name(), &mut buf),
                        self.needle(),
                        &mut indices,
                    )
                    .is_some()
                {
                    out.extend(indices.iter().map(|&i| i as usize + entry.name_char));
                } else if matcher
                    .fuzzy_indices(
                        Utf32Str::new(&entry.rel, &mut buf),
                        self.needle(),
                        &mut indices,
                    )
                    .is_some()
                {
                    out.extend(indices.iter().map(|&i| i as usize));
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

    fn score(
        &self,
        entry: &FileEntry,
        matcher: &mut Matcher,
        buf: &mut Vec<char>,
        precise: bool,
    ) -> Option<i64> {
        let mut total = 0;
        for term in &self.terms {
            match (term.score(entry, matcher, buf, precise), term.negate) {
                (Some(score), false) => total += score,
                (None, true) => {}
                _ => return None,
            }
        }
        Some(total)
    }

    fn positions(&self, entry: &FileEntry, matcher: &mut Matcher) -> Vec<usize> {
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
    folders: bool,
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
    cache: Option<MatchCache>,
}

fn match_config() -> MatchConfig {
    MatchConfig::DEFAULT.match_paths()
}

impl SearchIndex {
    fn new() -> Self {
        Self {
            entries: Vec::with_capacity(10_000),
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

    /// Searches files, or folders when `folders` is set.
    fn search(&mut self, raw: &str, query: &Query, limit: usize, folders: bool) -> SearchResults {
        if query.is_empty() {
            self.cache = None;
            return self.browse(limit, folders);
        }

        let entries = &self.entries;
        // Matchers keep scratch memory, so each worker thread gets its own.
        let init = || (Matcher::new(match_config()), Vec::new());
        let score = |(matcher, buf): &mut (Matcher, Vec<char>), idx: usize| {
            let entry = &entries[idx];
            if entry.folder != folders {
                return None;
            }
            query
                .score(entry, matcher, buf, false)
                .map(|score| (idx as u32, rank_key(score, entry)))
        };

        let matches: Vec<(u32, i64)> = match self.cache.take() {
            Some(cache) if cache.folders == folders && can_narrow(&cache.query, raw) => {
                let mut matches: Vec<_> = cache
                    .matches
                    .par_iter()
                    .map_init(init, |state, &(idx, _)| score(state, idx as usize))
                    .flatten()
                    .collect();
                matches.par_extend(
                    (cache.scanned..entries.len())
                        .into_par_iter()
                        .map_init(init, score)
                        .flatten(),
                );
                matches
            }
            _ => (0..entries.len())
                .into_par_iter()
                .map_init(init, score)
                .flatten()
                .collect(),
        };

        // The key already orders by score and then path length, so picking the
        // best never touches the entries. Only the final page gets the full
        // comparison.
        let cheap = |a: &(u32, i64), b: &(u32, i64)| b.1.cmp(&a.1);
        let shortlist = limit.max(1) * RESCORE_FACTOR;
        let mut ranked: Vec<(u32, i64)> = matches
            .par_chunks(16 * 1024)
            .flat_map_iter(|chunk| {
                let mut best = chunk.to_vec();
                if best.len() > shortlist {
                    best.select_nth_unstable_by(shortlist, cheap);
                    best.truncate(shortlist);
                }
                best
            })
            .collect();
        if ranked.len() > shortlist {
            ranked.select_nth_unstable_by(shortlist, cheap);
            ranked.truncate(shortlist);
        }
        // Order the shortlist by the optimal alignment.
        ranked
            .par_iter_mut()
            .for_each_init(init, |(matcher, buf), item| {
                let entry = &entries[item.0 as usize];
                if let Some(score) = query.score(entry, matcher, buf, true) {
                    item.1 = rank_key(score, entry);
                }
            });
        let items = take_best(&mut ranked, limit, |a, b| {
            b.1.cmp(&a.1)
                .then_with(|| compare_paths(&entries[a.0 as usize], &entries[b.0 as usize]))
        });

        let matched = matches.len();
        self.cache = Some(MatchCache {
            query: raw.to_string(),
            folders,
            scanned: entries.len(),
            matches,
        });
        SearchResults {
            items: items.into_iter().map(|(idx, _)| idx).collect(),
            matched,
        }
    }

    /// Results for an empty query: shallow paths first, then alphabetical.
    fn browse(&self, limit: usize, folders: bool) -> SearchResults {
        let entries = &self.entries;
        let mut ids: Vec<u32> = (0..entries.len() as u32)
            .into_par_iter()
            .filter(|&id| entries[id as usize].folder == folders)
            .collect();
        let matched = ids.len();
        let items = take_best(&mut ids, limit, |a, b| {
            let (a, b) = (&entries[*a as usize], &entries[*b as usize]);
            a.depth
                .cmp(&b.depth)
                .then_with(|| a.rel_lower.cmp(&b.rel_lower))
                .then_with(|| a.rel.cmp(&b.rel))
        });
        SearchResults { items, matched }
    }
}

/// Packs a match score and the path length into one sortable number: higher
/// scores first, and shorter paths first among equal scores.
fn rank_key(score: i64, entry: &FileEntry) -> i64 {
    let len = entry.rel.len().min(0xFFFF) as i64;
    (score << 16) | (0xFFFF - len)
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

/// What the finder lists: file names, folder names, or lines inside files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchMode {
    #[default]
    Files,
    Folders,
    Text,
}

impl SearchMode {
    fn next(self) -> Self {
        match self {
            Self::Files => Self::Folders,
            Self::Folders => Self::Text,
            Self::Text => Self::Files,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Folders => "folders",
            Self::Text => "text",
        }
    }
}

enum SearchRequest {
    Query { text: String, mode: SearchMode },
    Rename { old: PathBuf, new: PathBuf },
}

/// One result: a file or folder, and in text mode the matching line.
#[derive(Clone, Debug)]
struct Row {
    entry: FileEntry,
    hit: Option<grep::Hit>,
}

impl Row {
    fn same(&self, other: &Row) -> bool {
        self.entry.path == other.entry.path
            && self.hit.as_ref().map(|hit| hit.line) == other.hit.as_ref().map(|hit| hit.line)
    }
}

/// What the search thread sends back. The rows are copied out so the UI never
/// has to touch the index.
struct SearchUpdate {
    query: String,
    mode: SearchMode,
    rows: Vec<Row>,
    matched: usize,
    total: usize,
    /// The query changed, so the selection goes back to the top.
    reset: bool,
    indexing_done: bool,
    /// A text search is still running; more rows will follow.
    busy: bool,
}

/// Text search results kept so newly indexed files can be searched without
/// starting over.
struct TextCache {
    query: String,
    scanned: usize,
    rows: Vec<Row>,
    /// Stopped at the result limit.
    full: bool,
}

struct Searcher {
    root: PathBuf,
    index: SearchIndex,
    /// The fresh walk, built up while a remembered index answers searches.
    pending: Option<SearchIndex>,
    started: Instant,
    /// Where to remember the index once the walk is done.
    save_to: Option<PathBuf>,
    query: String,
    mode: SearchMode,
    /// The query and mode of the last update sent.
    shown: Option<(String, SearchMode)>,
    /// Files arrived since the last update.
    grown: bool,
    /// Something other than new files changed the results.
    stale: bool,
    indexing_done: bool,
    text: Option<TextCache>,
    /// Bumped by the UI for every query, so a text search can tell that it
    /// has been overtaken.
    generation: Arc<AtomicU64>,
    /// `generation` when the requests were last read.
    seen: u64,
}

impl Searcher {
    fn new(root: PathBuf, generation: Arc<AtomicU64>) -> Self {
        Self {
            root,
            index: SearchIndex::new(),
            pending: None,
            started: Instant::now(),
            save_to: None,
            query: String::new(),
            mode: SearchMode::Files,
            shown: None,
            grown: false,
            stale: true,
            indexing_done: false,
            text: None,
            generation,
            seen: 0,
        }
    }

    fn apply(&mut self, request: SearchRequest) {
        match request {
            SearchRequest::Query { text, mode } => {
                self.query = text;
                self.mode = mode;
            }
            SearchRequest::Rename { old, new } => {
                self.index.replace_path(&old, &new, &self.root);
                if let Some(pending) = &mut self.pending {
                    pending.replace_path(&old, &new, &self.root);
                }
                self.text = None;
                self.stale = true;
            }
        }
    }

    /// Files from the walk.
    fn add(&mut self, batch: Vec<FileEntry>) {
        match &mut self.pending {
            Some(pending) => pending.extend(batch),
            None => {
                self.index.extend(batch);
                self.grown = true;
            }
        }
    }

    /// Files from the remembered index.
    fn add_remembered(&mut self, batch: Vec<FileEntry>) {
        if self.pending.is_some() {
            self.index.extend(batch);
            self.grown = true;
        }
    }

    /// The remembered index has been read. If it turned out empty or
    /// unreadable, the walk so far takes over.
    fn remembered_done(&mut self) {
        if self.index.len() == 0 {
            if let Some(pending) = self.pending.take() {
                self.index = pending;
                self.grown = true;
            }
        }
    }

    /// The walk is complete: it replaces the remembered index, and is
    /// remembered itself when it took a while.
    fn finish(&mut self) {
        if let Some(fresh) = self.pending.take() {
            self.index = fresh;
            self.text = None;
        }
        self.indexing_done = true;
        self.stale = true;

        let Some(save_to) = self.save_to.take() else {
            return;
        };
        if self.started.elapsed() < index_cache::MIN_WALK && !save_to.exists() {
            return;
        }
        let entries: Vec<(String, bool)> = self
            .index
            .entries
            .iter()
            // A path that is not valid Unicode can't be rebuilt from `rel`.
            .filter(|entry| entry.path.to_str().is_some())
            .map(|entry| (entry.rel.clone(), entry.folder))
            .collect();
        thread::spawn(move || {
            let _ = index_cache::save(&save_to, entries);
        });
    }

    fn current(&self) -> (String, SearchMode) {
        (self.query.clone(), self.mode)
    }

    /// `None` when a search is due now, otherwise how long to wait for more
    /// input. New files are shown at most every `interval`.
    fn wait(&self, last: Option<Instant>, interval: Duration) -> Option<Duration> {
        if self.stale || self.shown.as_ref() != Some(&self.current()) {
            return None;
        }
        if !self.grown {
            return Some(Duration::from_secs(3600));
        }
        let since = last.map_or(interval, |at| at.elapsed());
        interval.checked_sub(since).filter(|left| !left.is_zero())
    }

    fn update_with(&self, rows: Vec<Row>, matched: usize, reset: bool, busy: bool) -> SearchUpdate {
        SearchUpdate {
            query: self.query.clone(),
            mode: self.mode,
            rows,
            matched,
            total: self.index.len(),
            reset,
            indexing_done: self.indexing_done,
            busy,
        }
    }

    /// Runs the current search and sends the results. Returns false once the
    /// finder has closed.
    fn update(&mut self, updates: &Sender<SearchUpdate>) -> bool {
        let reset = self.shown.as_ref() != Some(&self.current());
        self.shown = Some(self.current());
        self.grown = false;
        self.stale = false;
        if self.mode == SearchMode::Text {
            return self.search_text(reset, updates);
        }
        self.text = None;
        let parsed = Query::parse(&self.query);
        let folders = self.mode == SearchMode::Folders;
        let results = self
            .index
            .search(&self.query, &parsed, MAX_RESULTS, folders);
        let rows = results
            .items
            .iter()
            .map(|&id| Row {
                entry: self.index.entry(id).clone(),
                hit: None,
            })
            .collect();
        updates
            .send(self.update_with(rows, results.matched, reset, false))
            .is_ok()
    }

    fn search_text(&mut self, reset: bool, updates: &Sender<SearchUpdate>) -> bool {
        let pattern = (self.query.trim().chars().count() >= TEXT_MIN_CHARS)
            .then(|| grep::pattern(&self.query))
            .flatten();
        let Some(pattern) = pattern else {
            self.text = None;
            return updates
                .send(self.update_with(Vec::new(), 0, reset, false))
                .is_ok();
        };

        // Carry on over newly indexed files when the text is unchanged.
        let mut cache = match self.text.take() {
            Some(cache) if cache.query == self.query => cache,
            _ => TextCache {
                query: self.query.clone(),
                scanned: 0,
                rows: Vec::new(),
                full: false,
            },
        };
        let mut reset = reset;
        let mut last_sent = Instant::now();
        let total = self.index.len();
        while !cache.full && cache.scanned < total {
            if self.generation.load(AtomicOrdering::SeqCst) != self.seen {
                // A newer query is waiting; drop this one.
                return true;
            }
            let end = (cache.scanned + TEXT_CHUNK).min(total);
            let found: Vec<Row> = self.index.entries[cache.scanned..end]
                .par_iter()
                .filter(|entry| !entry.folder)
                .flat_map_iter(|entry| {
                    grep::search_file(&entry.path, &pattern, grep::MAX_HITS_PER_FILE)
                        .into_iter()
                        .map(|hit| Row {
                            entry: entry.clone(),
                            hit: Some(hit),
                        })
                })
                .collect();
            cache.rows.extend(found);
            cache.scanned = end;
            if cache.rows.len() >= MAX_RESULTS {
                cache.rows.truncate(MAX_RESULTS);
                cache.full = true;
            }
            let more = !cache.full && cache.scanned < total;
            if more && last_sent.elapsed() >= Duration::from_millis(TEXT_PARTIAL_MS) {
                let update = self.update_with(cache.rows.clone(), cache.rows.len(), reset, true);
                if updates.send(update).is_err() {
                    return false;
                }
                reset = false;
                last_sent = Instant::now();
            }
        }
        let update = self.update_with(cache.rows.clone(), cache.rows.len(), reset, false);
        self.text = Some(cache);
        updates.send(update).is_ok()
    }
}

/// Where the search thread gets its files from.
struct Sources {
    /// The walk of the folder.
    walk: Receiver<Vec<FileEntry>>,
    /// The index remembered from an earlier run, if any.
    remembered: Option<Receiver<Vec<FileEntry>>>,
    /// Where to remember this run's index.
    save_to: Option<PathBuf>,
}

/// Runs searches away from the UI thread, so typing never waits for one.
/// Queries and files that arrive during a search are folded into the next.
fn spawn_searcher(
    root: PathBuf,
    sources: Sources,
    requests: Receiver<SearchRequest>,
    updates: Sender<SearchUpdate>,
    generation: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        let mut searcher = Searcher::new(root, generation);
        searcher.save_to = sources.save_to;
        let mut remembered = sources.remembered;
        if remembered.is_some() {
            searcher.pending = Some(SearchIndex::new());
        }
        let mut batches = Some(sources.walk);
        let interval = Duration::from_millis(BATCH_CHECK_MS);
        let mut last_search: Option<Instant> = None;
        loop {
            // Read the generation first: a query sent after this point
            // cancels the search about to start.
            searcher.seen = searcher.generation.load(AtomicOrdering::SeqCst);
            while let Ok(request) = requests.try_recv() {
                searcher.apply(request);
            }
            if let Some(receiver) = &remembered {
                if drain(receiver, |batch| searcher.add_remembered(batch)) {
                    searcher.remembered_done();
                    remembered = None;
                }
            }
            if let Some(receiver) = &batches {
                if drain(receiver, |batch| searcher.add(batch)) {
                    searcher.finish();
                    batches = None;
                    // The fresh walk replaced it; stop reading it.
                    remembered = None;
                }
            }

            let Some(timeout) = searcher.wait(last_search, interval) else {
                if !searcher.update(&updates) {
                    return;
                }
                last_search = Some(Instant::now());
                continue;
            };
            let walk = batches.clone().unwrap_or_else(never);
            let old = remembered.clone().unwrap_or_else(never);
            select! {
                recv(requests) -> request => match request {
                    Ok(request) => searcher.apply(request),
                    // The finder has closed.
                    Err(_) => return,
                },
                recv(walk) -> batch => match batch {
                    Ok(batch) => searcher.add(batch),
                    Err(_) => {
                        searcher.finish();
                        batches = None;
                        remembered = None;
                    }
                },
                recv(old) -> batch => match batch {
                    Ok(batch) => searcher.add_remembered(batch),
                    Err(_) => {
                        searcher.remembered_done();
                        remembered = None;
                    }
                },
                default(timeout) => {}
            }
        }
    });
}

/// Takes everything queued on `receiver`. True once the sender is gone.
fn drain(receiver: &Receiver<Vec<FileEntry>>, mut take: impl FnMut(Vec<FileEntry>)) -> bool {
    loop {
        match receiver.try_recv() {
            Ok(batch) => take(batch),
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => return true,
        }
    }
}

/// Reads the remembered index for `root` on its own thread.
fn spawn_remembered(root: PathBuf, path: PathBuf) -> Receiver<Vec<FileEntry>> {
    let (sender, receiver) = bounded(64);
    thread::spawn(move || {
        let _ = index_cache::load(&path, |batch| {
            let entries = batch
                .into_par_iter()
                .map(|(rel, folder)| {
                    let path = root.join(rel);
                    if folder {
                        FileEntry::folder(path, &root)
                    } else {
                        FileEntry::new(path, &root)
                    }
                })
                .collect();
            sender.send(entries).is_ok()
        });
    });
    receiver
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
        let Some(kind) = entry.file_type() else {
            return WalkState::Continue;
        };
        if entry.depth() == 0 || !(kind.is_file() || kind.is_dir()) {
            return WalkState::Continue;
        }
        let file = if kind.is_dir() {
            FileEntry::folder(entry.into_path(), self.root)
        } else {
            FileEntry::new(entry.into_path(), self.root)
        };
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
    items: Vec<Row>,
    matched: usize,
    selected: usize,
    offset: usize,
    height: usize,
    marked: HashSet<PathBuf>,
}

impl ResultList {
    fn selected_row(&self) -> Option<&Row> {
        self.items.get(self.selected)
    }

    fn selected_entry(&self) -> Option<&FileEntry> {
        self.selected_row().map(|row| &row.entry)
    }

    /// Replaces the results. Unless `reset`, keeps the same row selected when
    /// it is still present (new files arriving while indexing).
    fn set(&mut self, items: Vec<Row>, matched: usize, reset: bool) {
        let previous = if reset {
            None
        } else {
            self.selected_row().cloned()
        };
        self.items = items;
        self.matched = matched;
        self.selected = previous
            .and_then(|previous| self.items.iter().position(|item| item.same(&previous)))
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
    /// Paths, and the line to open at when there is a single one.
    Edit(Vec<PathBuf>, Option<u32>),
}

struct App {
    searches: Sender<SearchRequest>,
    generation: Arc<AtomicU64>,
    mode_kind: SearchMode,
    /// The mode of the rows on screen, which lags behind `mode_kind` until
    /// the search answers.
    shown_mode: SearchMode,
    busy: bool,
    /// For highlighting the visible rows.
    matcher: RefCell<Matcher>,
    total: usize,
    query: LineEditor,
    parsed: Query,
    results: ResultList,
    mode: InputMode,
    status: Option<(String, Instant)>,
    show_icons: bool,
    preview_enabled: bool,
    previews: Sender<preview::Request>,
    preview: Option<preview::Preview>,
    preview_pending: Option<preview::Request>,
    /// The Kitty image on screen: file, column, row, width and height.
    placed: Option<(PathBuf, u16, u16, u16, u16)>,
    indexed: usize,
    indexing_done: bool,
    started: Instant,
}

impl App {
    fn new(
        show_icons: bool,
        mode: SearchMode,
        searches: Sender<SearchRequest>,
        generation: Arc<AtomicU64>,
        previews: Sender<preview::Request>,
    ) -> Self {
        Self {
            searches,
            generation,
            mode_kind: mode,
            shown_mode: mode,
            busy: false,
            matcher: RefCell::new(Matcher::new(match_config())),
            total: 0,
            query: LineEditor::default(),
            parsed: Query::default(),
            results: ResultList::default(),
            mode: InputMode::Normal,
            status: None,
            show_icons,
            preview_enabled: true,
            previews,
            preview: None,
            preview_pending: None,
            placed: None,
            indexed: 0,
            indexing_done: false,
            started: Instant::now(),
        }
    }

    fn search(&self) {
        self.generation.fetch_add(1, AtomicOrdering::SeqCst);
        let _ = self.searches.send(SearchRequest::Query {
            text: self.query.text.clone(),
            mode: self.mode_kind,
        });
    }

    fn apply(&mut self, update: SearchUpdate, reset: bool) {
        // Highlights follow the query the results are for, which can lag
        // behind what has been typed.
        self.parsed = Query::parse(&update.query);
        self.shown_mode = update.mode;
        self.results.set(update.rows, update.matched, reset);
        self.total = update.total;
        self.indexing_done = update.indexing_done;
        self.busy = update.busy;
    }

    fn show_preview(&mut self, preview: preview::Preview) {
        if self.preview_pending.as_ref() == Some(&preview.request) {
            self.preview_pending = None;
        }
        self.preview = Some(preview);
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
        self.results.selected_entry()
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
                    let line = if self.results.marked.is_empty() {
                        self.results
                            .selected_row()
                            .and_then(|row| row.hit.as_ref())
                            .map(|hit| hit.line)
                    } else {
                        None
                    };
                    Action::Edit(paths, line)
                }
            }
            KeyCode::Char('f') if ctrl => {
                self.mode_kind = self.mode_kind.next();
                self.status = None;
                Action::Search
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
                let _ = self.searches.send(SearchRequest::Rename {
                    old: old_path.clone(),
                    new: new_path.clone(),
                });
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
            Print(tint("─".repeat(width), palette::current().faint))
        )?;

        let rows = self.render_rows(list_width);
        let preview = if preview_width > 0 {
            self.preview_lines(list_height, preview_width)
        } else {
            Vec::new()
        };
        // Where the preview image belongs, if there is one.
        let preview_x = (list_width + 2) as u16;
        let wanted = self
            .preview
            .as_ref()
            .filter(|_| preview_width > 0 && graphics::protocol() == Protocol::Kitty)
            .and_then(|shown| {
                let (graphic, top) = shown.graphic.as_ref()?;
                Some((
                    shown.request.path.clone(),
                    preview_x,
                    (top + 2) as u16,
                    graphic.cols,
                    graphic.rows,
                ))
            });
        // Rows under an image that is already up are left alone, so it is
        // not painted over.
        let keep = |row: usize| {
            let row = (row + 2) as u16;
            matches!((&wanted, &self.placed), (Some(a), Some(b)) if a == b && row >= a.2 && row < a.2 + a.4)
        };
        for row in 0..list_height {
            queue!(out, cursor::MoveTo(0, (row + 2) as u16))?;
            let line = rows.get(row).map(String::as_str).unwrap_or("");
            queue!(out, Print(fit(line, list_width)))?;
            if preview_width > 0 {
                queue!(out, Print(tint(" │", palette::current().faint)))?;
                if !keep(row) {
                    let text = preview.get(row).map(String::as_str).unwrap_or("");
                    queue!(out, Print(fit(text, preview_width)))?;
                }
            }
        }
        if wanted != self.placed {
            if self.placed.is_some() {
                queue!(out, Print(graphics::kitty_delete()))?;
            }
            if let (Some(place), Some((graphic, _))) = (
                &wanted,
                self.preview
                    .as_ref()
                    .and_then(|shown| shown.graphic.as_ref()),
            ) {
                queue!(
                    out,
                    cursor::MoveTo(place.1, place.2),
                    Print(graphics::kitty_place(graphic))
                )?;
            }
            self.placed = wanted;
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
                console::measure_text_width(&self.prompt_prefix())
                    + console::measure_text_width(self.query.before_cursor()),
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

    /// The mode name and the prompt arrow.
    fn prompt_prefix(&self) -> String {
        let ui = palette::current();
        format!(
            " {}{}",
            tint(self.mode_kind.label(), ui.muted),
            PROMPT.color(ui.accent.colored()).bold()
        )
    }

    fn render_prompt(&self, width: usize) -> String {
        let ui = palette::current();
        let accent = ui.accent.colored();
        let input = if self.query.text.is_empty() {
            let placeholder = match self.mode_kind {
                SearchMode::Files => "Type to search  ('exact ^prefix suffix$ !exclude)",
                SearchMode::Folders => "Type to search folders",
                SearchMode::Text => "Type to search inside files",
            };
            tint(placeholder, ui.muted)
        } else {
            self.query.text.color(ui.text.colored()).bold().to_string()
        };

        let spinner = if self.indexing_done && !self.busy {
            String::new()
        } else {
            let frame = (self.started.elapsed().as_millis() / SPINNER_INTERVAL_MS as u128) as usize
                % SPINNER_FRAMES.len();
            format!("{} ", SPINNER_FRAMES[frame].color(accent))
        };
        let count = if self.shown_mode == SearchMode::Text {
            let more = if self.results.matched >= MAX_RESULTS {
                "+"
            } else {
                ""
            };
            format!("{}{} lines", self.results.matched, more)
        } else {
            format!("{}/{}", self.results.matched, self.total)
        };
        let counter = format!("{}{} ", spinner, tint(count, ui.muted));

        let left = format!("{}{}", self.prompt_prefix(), input);
        let room = width.saturating_sub(console::measure_text_width(&counter));
        format!("{}{}", fit(&left, room), counter)
    }

    fn render_rows(&self, width: usize) -> Vec<String> {
        if self.results.items.is_empty() {
            let short = self.query.text.trim().chars().count() < TEXT_MIN_CHARS;
            let message = if !self.indexing_done && self.total == 0 {
                format!("Indexing files... {} found", self.indexed)
            } else if self.total == 0 {
                "No files found".to_string()
            } else if self.mode_kind == SearchMode::Text && short {
                format!("Type at least {TEXT_MIN_CHARS} characters to search inside files")
            } else if self.busy || self.shown_mode != self.mode_kind {
                "Searching...".to_string()
            } else {
                format!("No matches ({} files indexed)", self.total)
            };
            return vec![format!("  {}", tint(message, palette::current().muted))];
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
            .map(|(idx, row)| {
                let entry = &row.entry;
                let selected = idx == self.results.selected;
                let marked = self.results.marked.contains(&entry.path);
                if let Some(hit) = &row.hit {
                    return render_hit_row(entry, hit, selected, marked, self.show_icons, width);
                }
                let positions = self.parsed.positions(entry, &mut self.matcher.borrow_mut());
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
        let ui = palette::current();
        let hint = |s: &str| tint(s, ui.muted);
        match &self.mode {
            InputMode::Rename(editor) => {
                let label = " RENAME ".color(ui.warning.colored()).bold().to_string();
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
                            .color(ui.warning.colored())
                            .bold()
                    )
                };
                let body = match &self.status {
                    Some((message, _)) => tint(message, ui.success),
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

    /// The preview for the selected file. Previews are built on another
    /// thread; until the new one arrives the previous one stays up.
    fn preview_lines(&mut self, height: usize, width: usize) -> Vec<String> {
        let Some(row) = self.results.selected_row() else {
            return Vec::new();
        };
        let request = preview::Request {
            path: row.entry.path.clone(),
            height,
            width,
            line: row.hit.as_ref().map(|hit| hit.line),
        };
        let ready = self
            .preview
            .as_ref()
            .is_some_and(|shown| shown.request == request);
        if !ready && self.preview_pending.as_ref() != Some(&request) {
            let _ = self.previews.send(request.clone());
            self.preview_pending = Some(request);
        }
        self.preview
            .as_ref()
            .map(|shown| shown.lines.clone())
            .unwrap_or_default()
    }
}

const PROMPT: &str = " ❯ ";

fn tint(text: impl AsRef<str>, tone: Tone) -> String {
    text.as_ref().color(tone.colored()).to_string()
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

/// A text-mode row: `path:line  matching text`.
fn render_hit_row(
    entry: &FileEntry,
    hit: &grep::Hit,
    selected: bool,
    marked: bool,
    show_icons: bool,
    width: usize,
) -> String {
    let ui = palette::current();
    let pointer = if selected {
        "▌".color(ui.accent.colored()).bold().to_string()
    } else {
        " ".to_string()
    };
    let mark = if marked {
        "●".color(ui.warning.colored()).bold().to_string()
    } else {
        " ".to_string()
    };
    let base = colorize_file_name(&entry.path).fgcolor();
    let name = match base {
        Some(color) => entry.name().color(color),
        None => entry.name().normal(),
    };
    let name = if selected { name.bold() } else { name };
    let dir = if entry.dir().is_empty() {
        String::new()
    } else {
        tint(
            format!("{}{}", entry.dir(), std::path::MAIN_SEPARATOR),
            ui.muted,
        )
    };
    let location = format_with_icon(
        &entry.path,
        format!("{dir}{name}{}", tint(format!(":{}", hit.line), ui.faint)),
        show_icons,
    );

    // Keep the path to at most 45% of the row so the line stays readable.
    let location = fit_trimmed(&location, width * 45 / 100);
    let text = highlight_ranges(
        &hit.text,
        &hit.ranges,
        |s| tint(s, ui.text),
        |s| s.color(ui.warning.colored()).bold().to_string(),
    );
    format!(" {pointer}{mark} {location}  {text}")
}

/// Like `fit` but without padding.
fn fit_trimmed(text: &str, width: usize) -> String {
    if console::measure_text_width(text) > width {
        console::truncate_str(text, width, "…").into_owned()
    } else {
        text.to_string()
    }
}

/// Styles the char `ranges` of `text` with `highlighted`, the rest with `plain`.
fn highlight_ranges(
    text: &str,
    ranges: &[(usize, usize)],
    plain: impl Fn(&str) -> String,
    highlighted: impl Fn(&str) -> String,
) -> String {
    let positions: Vec<usize> = ranges.iter().flat_map(|&(from, to)| from..to).collect();
    highlight_runs(text, 0, &positions, plain, highlighted)
}

fn entry_meta(path: &Path, show_perms: bool) -> String {
    let Ok(metadata) = path.metadata() else {
        return String::new();
    };
    let modified = metadata
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let size = if metadata.is_dir() {
        format!("      {}", tint("-", palette::current().faint))
    } else {
        format!("{:>7}", colorize_size(metadata.len()))
    };
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
    let ui = palette::current();
    let base = colorize_file_name(&entry.path).fgcolor();

    let pointer = if selected {
        "▌".color(ui.accent.colored()).bold().to_string()
    } else {
        " ".to_string()
    };
    let mark = if marked {
        "●".color(ui.warning.colored()).bold().to_string()
    } else {
        " ".to_string()
    };

    let hl = |s: &str| s.color(ui.warning.colored()).bold().underline().to_string();
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
    let name = if entry.folder {
        format!("{name}{}", tint(std::path::MAIN_SEPARATOR_STR, ui.muted))
    } else {
        name
    };
    let name = format_with_icon(&entry.path, name, show_icons);
    let dir = highlight_runs(
        entry.dir(),
        0,
        positions,
        |s| {
            if selected {
                tint(s, ui.accent)
            } else {
                tint(s, ui.muted)
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
        clear_images(&mut self.out);
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

fn clear_images(out: &mut impl Write) {
    if graphics::protocol() == Protocol::Kitty {
        let _ = execute!(out, Print(graphics::kitty_delete()));
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        clear_images(&mut self.out);
        let _ = execute!(self.out, terminal::LeaveAlternateScreen, cursor::Show);
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Clone)]
pub struct FuzzyLister {
    config: crate::config::Config,
    respect_gitignore: bool,
    show_icons: bool,
    mode: SearchMode,
}

impl FuzzyLister {
    pub fn new(
        config: crate::config::Config,
        respect_gitignore: bool,
        show_icons: bool,
        mode: SearchMode,
    ) -> Self {
        Self {
            show_icons,
            config,
            respect_gitignore,
            mode,
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

        let fuzzy = &self.config.listers.fuzzy;
        let save_to = fuzzy
            .remember_index
            .then(|| index_cache::location(&root, self.respect_gitignore, &fuzzy.ignore_patterns))
            .flatten();
        let remembered = save_to
            .clone()
            .filter(|path| path.exists())
            .map(|path| spawn_remembered(root.clone(), path));
        let (search_sender, search_requests) = unbounded();
        let (update_sender, updates) = unbounded();
        let generation = Arc::new(AtomicU64::new(0));
        spawn_searcher(
            root,
            Sources {
                walk: receiver,
                remembered,
                save_to,
            },
            search_requests,
            update_sender,
            Arc::clone(&generation),
        );
        let (preview_sender, preview_requests) = unbounded();
        let (preview_done, previews) = unbounded();
        preview::spawn_worker(preview_requests, preview_done);
        preview::warm_up();

        let mut session = TerminalSession::enter()?;
        let mut app = App::new(
            self.show_icons,
            self.mode,
            search_sender,
            generation,
            preview_sender,
        );
        if self.mode != SearchMode::Files {
            app.search();
        }
        let poll_interval = Duration::from_millis(POLL_INTERVAL_MS);
        let spinner_interval = Duration::from_millis(SPINNER_INTERVAL_MS);
        let mut last_draw = Instant::now();
        let mut dirty = true;

        loop {
            let mut latest = None;
            let mut reset = false;
            while let Ok(update) = updates.try_recv() {
                reset |= update.reset;
                latest = Some(update);
            }
            if let Some(update) = latest {
                app.apply(update, reset);
                dirty = true;
            }
            while let Ok(preview) = previews.try_recv() {
                app.show_preview(preview);
                dirty = true;
            }
            app.indexed = indexed.load(AtomicOrdering::SeqCst);

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
                                    app.search();
                                    dirty = true;
                                }
                                Action::Accept => return Ok(app.accept()),
                                Action::Cancel => return Ok(Vec::new()),
                                Action::Edit(paths, line) => {
                                    session.suspend()?;
                                    app.placed = None;
                                    let opened = open_in_editor(
                                        &paths,
                                        line,
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
            .search(raw, &query, MAX_RESULTS, false)
            .items
            .into_iter()
            .map(|id| index.entry(id).rel.clone())
            .collect()
    }

    /// Times typing a query one key at a time over a real tree:
    /// `LLA_FUZZY_BENCH=~ cargo test --release fuzzy_typing_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn fuzzy_typing_speed() {
        let root = PathBuf::from(std::env::var("LLA_FUZZY_BENCH").unwrap());
        let (sender, receiver) = bounded::<Vec<FileEntry>>(256);
        let indexed = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicBool::new(false));
        spawn_indexer(
            root.clone(),
            true,
            Arc::new(IgnoreRules::new(&[])),
            sender,
            indexed,
            done,
        );
        let mut index = SearchIndex::new();
        for batch in receiver.iter() {
            index.extend(batch);
        }
        println!("indexed {} files", index.len());
        let mut typed = String::new();
        let mut total = Duration::ZERO;
        for step in "srcmainrs\u{8}\u{8}ts".chars() {
            if step == '\u{8}' {
                typed.pop();
            } else {
                typed.push(step);
            }
            let started = Instant::now();
            let results = index.search(&typed, &Query::parse(&typed), MAX_RESULTS, false);
            let took = started.elapsed();
            total += took;
            println!(
                "{typed:>10}  {:>8} matches  {:>7.1} ms",
                results.matched,
                took.as_secs_f64() * 1000.0
            );
        }
        println!("total {:.1} ms", total.as_secs_f64() * 1000.0);
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
        assert_eq!(
            index.search("a", &first, MAX_RESULTS, false).matched,
            refs.len()
        );
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
        assert_eq!(index.search("", &Query::default(), 2, false).matched, 4);
        assert_eq!(index.search("", &Query::default(), 2, false).items.len(), 2);
    }

    #[test]
    fn highlight_positions_prefer_the_file_name() {
        let matcher = &mut Matcher::new(match_config());
        let e = entry("main/src/main.rs");
        let positions = Query::parse("main").positions(&e, matcher);
        assert_eq!(positions, (9..13).collect::<Vec<_>>());
        let positions = Query::parse("'src").positions(&e, matcher);
        assert_eq!(positions, (5..8).collect::<Vec<_>>());
        let positions = Query::parse(".rs$").positions(&e, matcher);
        assert_eq!(positions, (13..16).collect::<Vec<_>>());
        assert!(Query::parse("!main").positions(&e, matcher).is_empty());
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
        let entries = |names: &[&str]| {
            names
                .iter()
                .map(|name| Row {
                    entry: entry(name),
                    hit: None,
                })
                .collect::<Vec<_>>()
        };
        let selected = |list: &ResultList| list.selected_entry().map(|entry| entry.rel.clone());
        let mut list = ResultList::default();
        list.set_height(2);
        list.set(entries(&["a", "b", "c"]), 3, true);
        list.move_by(2);
        assert_eq!(selected(&list).as_deref(), Some("c"));
        assert_eq!(list.offset, 1);
        list.set(entries(&["z", "a", "b", "c"]), 4, false);
        assert_eq!(selected(&list).as_deref(), Some("c"));
        list.set(entries(&["q"]), 1, true);
        assert_eq!(list.selected, 0);
        assert_eq!(list.offset, 0);
        list.move_by(-5);
        assert_eq!(list.selected, 0);
    }

    /// Indexes `root` like the finder does and runs one query in `mode`.
    fn search_tree(root: &Path, text: &str, mode: SearchMode) -> SearchUpdate {
        let (sender, batches) = bounded(16);
        spawn_indexer(
            root.to_path_buf(),
            false,
            Arc::new(IgnoreRules::new(&[])),
            sender,
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicBool::new(false)),
        );
        let (requests, request_receiver) = unbounded();
        let (update_sender, updates) = unbounded();
        spawn_searcher(
            root.to_path_buf(),
            Sources {
                walk: batches,
                remembered: None,
                save_to: None,
            },
            request_receiver,
            update_sender,
            Arc::new(AtomicU64::new(0)),
        );
        requests
            .send(SearchRequest::Query {
                text: text.into(),
                mode,
            })
            .unwrap();
        let update = updates
            .iter()
            .find(|update| update.query == text && update.indexing_done && !update.busy)
            .unwrap();
        drop(requests);
        update
    }

    #[test]
    fn folder_and_text_modes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/lister")).unwrap();
        std::fs::write(
            root.join("src/lister/fuzzy.rs"),
            "fn search() {}\n// TODO later\n",
        )
        .unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("notes.txt"), "nothing\nTODO: write docs\n").unwrap();

        let folders = search_tree(root, "lister", SearchMode::Folders);
        let names: Vec<_> = folders
            .rows
            .iter()
            .map(|row| row.entry.name().to_string())
            .collect();
        assert_eq!(names, vec!["lister"]);
        assert!(folders.rows.iter().all(|row| row.entry.folder));

        let files = search_tree(root, "lister", SearchMode::Files);
        assert!(files.rows.iter().all(|row| !row.entry.folder));
        assert_eq!(files.rows[0].entry.name(), "fuzzy.rs");

        let text = search_tree(root, "todo", SearchMode::Text);
        let mut found: Vec<_> = text
            .rows
            .iter()
            .map(|row| (row.entry.name().to_string(), row.hit.as_ref().unwrap().line))
            .collect();
        found.sort();
        assert_eq!(
            found,
            vec![("fuzzy.rs".to_string(), 2), ("notes.txt".to_string(), 2)]
        );

        // One character is not enough to search file contents.
        assert!(search_tree(root, "t", SearchMode::Text).rows.is_empty());
    }

    /// `LLA_FUZZY_BENCH=~ cargo test --release remembered_index_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn remembered_index_speed() {
        let root = PathBuf::from(std::env::var("LLA_FUZZY_BENCH").unwrap());
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("bench.idx");
        let first_results = |remembered: bool| {
            let started = Instant::now();
            let (sender, walk) = bounded(256);
            spawn_indexer(
                root.clone(),
                true,
                Arc::new(IgnoreRules::new(&[])),
                sender,
                Arc::new(AtomicUsize::new(0)),
                Arc::new(AtomicBool::new(false)),
            );
            let (requests, request_receiver) = unbounded();
            let (update_sender, updates) = unbounded();
            spawn_searcher(
                root.clone(),
                Sources {
                    walk,
                    remembered: remembered.then(|| spawn_remembered(root.clone(), cache.clone())),
                    save_to: Some(cache.clone()),
                },
                request_receiver,
                update_sender,
                Arc::new(AtomicU64::new(0)),
            );
            requests
                .send(SearchRequest::Query {
                    text: "main.rs".into(),
                    mode: SearchMode::Files,
                })
                .unwrap();
            let mut first = None;
            let mut most = None;
            let mut total = 0;
            for update in updates.iter() {
                if update.query == "main.rs" && first.is_none() && update.total > 100_000 {
                    first = Some(started.elapsed());
                }
                if most.is_none() && update.total > 2_400_000 {
                    most = Some(started.elapsed());
                    println!("  2.4M+ searchable after {most:?}");
                }
                total = update.total;
                if update.indexing_done {
                    break;
                }
            }
            (first, started.elapsed(), total)
        };
        let (first, all, total) = first_results(false);
        println!("cold: {total} entries, 100k+ searchable after {first:?}, walk done {all:?}");
        let deadline = Instant::now() + Duration::from_secs(60);
        while !cache.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        thread::sleep(Duration::from_millis(500));
        println!(
            "cache file: {:.1} MB",
            std::fs::metadata(&cache).unwrap().len() as f64 / 1e6
        );
        let started = Instant::now();
        let mut loaded = 0;
        index_cache::load(&cache, |batch| {
            loaded += batch.len();
            true
        })
        .unwrap();
        println!("load alone: {loaded} entries in {:?}", started.elapsed());
        let (first, all, total) = first_results(true);
        println!("warm: {total} entries, 100k+ searchable after {first:?}, walk done {all:?}");
    }

    #[test]
    fn remembered_index_answers_first_then_the_walk_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("kept.txt"), "").unwrap();
        let cache = dir.path().join("cache.idx");
        index_cache::save(
            &cache,
            vec![("kept.txt".into(), false), ("deleted.txt".into(), false)],
        )
        .unwrap();

        let (walk_sender, walk) = bounded(4);
        let (requests, request_receiver) = unbounded();
        let (update_sender, updates) = unbounded();
        spawn_searcher(
            root.clone(),
            Sources {
                walk,
                remembered: Some(spawn_remembered(root.clone(), cache.clone())),
                save_to: Some(cache.clone()),
            },
            request_receiver,
            update_sender,
            Arc::new(AtomicU64::new(0)),
        );
        let names = |update: &SearchUpdate| {
            let mut names: Vec<_> = update
                .rows
                .iter()
                .map(|row| row.entry.rel.clone())
                .collect();
            names.sort();
            names
        };
        // Before the walk has sent anything, the remembered files are there.
        let early = updates.iter().find(|update| update.total == 2).unwrap();
        assert!(!early.indexing_done);
        assert_eq!(names(&early), vec!["deleted.txt", "kept.txt"]);

        walk_sender
            .send(vec![FileEntry::new(root.join("kept.txt"), &root)])
            .unwrap();
        drop(walk_sender);
        let done = updates.iter().find(|update| update.indexing_done).unwrap();
        assert_eq!(names(&done), vec!["kept.txt"]);
        drop(requests);

        // The cache is rewritten from the walk.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut saved = Vec::new();
            let _ = index_cache::load(&cache, |batch| {
                saved.extend(batch);
                true
            });
            if saved == vec![("kept.txt".to_string(), false)] {
                break;
            }
            assert!(Instant::now() < deadline, "cache not rewritten: {saved:?}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn editors_get_the_line_in_their_own_syntax() {
        let path = Path::new("src/main.rs");
        let args = |program: &str| {
            editor_line_args(program, path, 42)
                .into_iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(args("nvim"), vec!["+42", "src/main.rs"]);
        assert_eq!(args("/usr/local/bin/code"), vec!["-g", "src/main.rs:42"]);
        assert_eq!(args("hx"), vec!["src/main.rs:42"]);
        assert_eq!(args("notepad"), vec!["src/main.rs"]);
    }

    #[test]
    fn searches_run_on_their_own_thread() {
        let (batch_sender, batches) = bounded(4);
        let (requests, request_receiver) = unbounded();
        let (update_sender, updates) = unbounded();
        spawn_searcher(
            PathBuf::from("/root"),
            Sources {
                walk: batches,
                remembered: None,
                save_to: None,
            },
            request_receiver,
            update_sender,
            Arc::new(AtomicU64::new(0)),
        );
        batch_sender
            .send(vec![entry("src/main.rs"), entry("README.md")])
            .unwrap();
        drop(batch_sender);
        requests
            .send(SearchRequest::Query {
                text: "main".into(),
                mode: SearchMode::Files,
            })
            .unwrap();
        let update = updates
            .iter()
            .find(|update| update.query == "main" && update.indexing_done)
            .unwrap();
        assert_eq!(update.total, 2);
        assert_eq!(update.rows.len(), 1);
        assert_eq!(update.rows[0].entry.rel, "src/main.rs");
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
    fn ignore_rules_skip_whole_components() {
        let rules = IgnoreRules::new(&[".git".to_string(), "target".to_string()]);
        assert!(rules.matches(".git/HEAD"));
        assert!(rules.matches("crate/target/debug/x"));
        assert!(!rules.matches(".github/workflows/ci.yml"));
        assert!(!rules.matches("src/targets.rs"));
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
