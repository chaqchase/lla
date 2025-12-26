use super::FileLister;
use crate::utils::color::*;
use crate::utils::icons::format_with_icon;
use crate::{error::Result, theme::color_value_to_color};
use colored::*;
use crossbeam_channel::{bounded, Sender};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    style::{self},
    terminal::{self, ClearType},
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ignore::WalkBuilder;
use parking_lot::RwLock;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs::Permissions;
use std::io::{self, stdout, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering},
    Arc,
};
use std::thread;
use std::time::{Duration, SystemTime};

const WORKER_THREADS: usize = 8;
const CHUNK_SIZE: usize = 1000;
const SEARCH_DEBOUNCE_MS: u64 = 50;
const RENDER_INTERVAL_MS: u64 = 16;
const STATUS_MESSAGE_TIMEOUT_MS: u64 = 2000;

/// Input mode for the fuzzy finder UI
#[derive(Clone, PartialEq)]
enum InputMode {
    /// Normal browsing/search mode
    Normal,
    /// Renaming a file - contains the new name being edited and cursor position
    Rename { buffer: String, cursor: usize },
}

fn path_contains_git_dir(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".git")
}

fn stream_gitignore_filtered_entries(
    directory: &str,
    sender: Sender<Vec<FileEntry>>,
    total_indexed: &Arc<AtomicUsize>,
) {
    let mut builder = WalkBuilder::new(directory);
    builder
        .hidden(false)
        .follow_links(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true)
        .ignore(true)
        .require_git(false)
        .same_file_system(false)
        .threads(1);

    let mut batch = Vec::with_capacity(CHUNK_SIZE);

    for dent in builder.build() {
        let entry = match dent {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.depth() == 0 {
            continue;
        }

        if path_contains_git_dir(entry.path()) {
            continue;
        }

        if !entry.file_type().map_or(false, |ft| ft.is_file()) {
            continue;
        }

        batch.push(FileEntry::new(entry.into_path()));
        total_indexed.fetch_add(1, AtomicOrdering::SeqCst);

        if batch.len() >= CHUNK_SIZE {
            let _ = sender.send(batch);
            batch = Vec::with_capacity(CHUNK_SIZE);
        }
    }

    if !batch.is_empty() {
        let _ = sender.send(batch);
    }
}

#[derive(Clone)]
#[allow(dead_code)]
struct FileEntry {
    path: PathBuf,
    path_str: String,
    name_str: String,
    modified: SystemTime,
    normalized_path: String,
}
fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    // Best-effort cross-platform clipboard support
    // macOS: pbcopy, Linux (common): xclip or xsel, Windows: clip
    #[cfg(target_os = "macos")]
    {
        let mut child = ProcessCommand::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write as IoWrite;
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
                use std::io::Write as IoWrite;
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
                use std::io::Write as IoWrite;
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
            use std::io::Write as IoWrite;
            stdin.write_all(text.as_bytes())?;
        }
        let _ = child.wait();
        return Ok(());
    }

    #[allow(unreachable_code)]
    {
        Ok(())
    }
}

fn open_path(path: &PathBuf) -> std::io::Result<()> {
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
    let _ = cmd.spawn()?.wait();
    Ok(())
}

fn rename_file(old_path: &Path, new_name: &str) -> std::io::Result<PathBuf> {
    let parent = old_path.parent().unwrap_or(Path::new("."));
    let new_path = parent.join(new_name);
    std::fs::rename(old_path, &new_path)?;
    Ok(new_path)
}

impl FileEntry {
    fn new(path: PathBuf) -> Self {
        let path_str = path.to_string_lossy().into_owned();
        let name_str = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let normalized_path = path_str.to_lowercase();

        Self {
            path_str,
            name_str,
            normalized_path,
            modified: path
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or_else(|_| SystemTime::now()),
            path,
        }
    }
}

#[derive(Clone)]
struct MatchResult {
    score: i64,
    positions: Vec<usize>,
    entry: FileEntry,
}

#[derive(Clone)]
struct SearchIndex {
    entries: Arc<parking_lot::RwLock<Vec<FileEntry>>>,
    matcher: Arc<SkimMatcherV2>,
    last_query: Arc<RwLock<String>>,
    last_results: Arc<RwLock<Vec<MatchResult>>>,
    config: crate::config::Config,
    respect_gitignore: bool,
}

impl SearchIndex {
    fn new(config: crate::config::Config, respect_gitignore: bool) -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::with_capacity(10000))),
            matcher: Arc::new(SkimMatcherV2::default().ignore_case()),
            last_query: Arc::new(RwLock::new(String::new())),
            last_results: Arc::new(RwLock::new(Vec::new())),
            config,
            respect_gitignore,
        }
    }

    fn should_ignore_path(&self, path: &std::path::Path) -> bool {
        if self.respect_gitignore && path_contains_git_dir(path) {
            return true;
        }

        if self.config.listers.fuzzy.ignore_patterns.is_empty() {
            return false;
        }

        let path_str = path.to_string_lossy();
        self.config
            .listers
            .fuzzy
            .ignore_patterns
            .iter()
            .any(|pattern| {
                if pattern.starts_with("regex:") {
                    if let Ok(re) = regex::Regex::new(&pattern[6..]) {
                        re.is_match(&path_str)
                    } else {
                        false
                    }
                } else if pattern.starts_with("glob:") {
                    if let Ok(glob) = glob::Pattern::new(&pattern[5..]) {
                        glob.matches(&path_str)
                    } else {
                        false
                    }
                } else {
                    path_str.contains(pattern)
                }
            })
    }

    fn add_entries(&self, new_entries: Vec<FileEntry>) -> bool {
        let filtered: Vec<_> = new_entries
            .into_iter()
            .filter(|entry| !self.should_ignore_path(&entry.path))
            .collect();

        if filtered.is_empty() {
            return false;
        }

        let mut entries = self.entries.write();
        entries.extend(filtered);
        true
    }

    fn replace_entry_path(&self, old_path: &Path, new_path: &Path) {
        // Update the indexed entry so future searches reflect the rename.
        let mut entries = self.entries.write();
        if let Some(entry) = entries.iter_mut().find(|e| e.path == old_path) {
            *entry = FileEntry::new(new_path.to_path_buf());
        } else {
            // If the old path wasn't in the index (edge case), at least add the new one.
            entries.push(FileEntry::new(new_path.to_path_buf()));
        }

        // Invalidate search cache so the next query recomputes results.
        *self.last_query.write() = String::new();
        self.last_results.write().clear();
    }

    fn search(&self, query: &str, max_results: usize) -> Vec<MatchResult> {
        let entries = self.entries.read();

        if query.is_empty() {
            let mut results: Vec<_> = entries
                .iter()
                .take(max_results)
                .map(|entry| MatchResult {
                    score: 0,
                    positions: vec![],
                    entry: entry.clone(),
                })
                .collect();

            results.par_sort_unstable_by(|a, b| a.entry.name_str.cmp(&b.entry.name_str));
            return results;
        }

        {
            let last_query = self.last_query.read();
            if query.starts_with(&*last_query) {
                let cached_results = self.last_results.read();
                if !cached_results.is_empty() {
                    let filtered: Vec<_> = cached_results
                        .iter()
                        .take(max_results * 2)
                        .filter_map(|result| {
                            self.matcher
                                .fuzzy_match(&result.entry.normalized_path, query)
                                .map(|score| {
                                    let positions = self
                                        .matcher
                                        .fuzzy_indices(&result.entry.normalized_path, query)
                                        .map(|(_, indices)| indices)
                                        .unwrap_or_default();
                                    MatchResult {
                                        score,
                                        positions,
                                        entry: result.entry.clone(),
                                    }
                                })
                        })
                        .collect();

                    if !filtered.is_empty() {
                        let mut results = filtered;
                        results.par_sort_unstable_by(|a, b| {
                            b.score
                                .cmp(&a.score)
                                .then_with(|| a.entry.path_str.len().cmp(&b.entry.path_str.len()))
                        });
                        results.truncate(max_results);
                        return results;
                    }
                }
            }
        }

        let chunk_size = (entries.len() / WORKER_THREADS).max(CHUNK_SIZE);
        let results: Vec<_> = entries
            .par_chunks(chunk_size)
            .flat_map(|chunk| {
                chunk
                    .iter()
                    .filter_map(|entry| {
                        self.matcher
                            .fuzzy_match(&entry.normalized_path, query)
                            .map(|score| {
                                let positions = self
                                    .matcher
                                    .fuzzy_indices(&entry.normalized_path, query)
                                    .map(|(_, indices)| indices)
                                    .unwrap_or_default();
                                MatchResult {
                                    score,
                                    positions,
                                    entry: entry.clone(),
                                }
                            })
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let mut results = results;
        results.par_sort_unstable_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.entry.path_str.len().cmp(&b.entry.path_str.len()))
        });
        results.truncate(max_results);

        *self.last_query.write() = query.to_string();
        *self.last_results.write() = results.clone();

        results
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct FuzzyLister {
    index: SearchIndex,
    config: crate::config::Config,
    respect_gitignore: bool,
}

impl FuzzyLister {
    pub fn new(config: crate::config::Config, respect_gitignore: bool) -> Self {
        Self {
            index: SearchIndex::new(config.clone(), respect_gitignore),
            config,
            respect_gitignore,
        }
    }

    fn run_interactive(
        &self,
        directory: &str,
        _recursive: bool,
        _depth: Option<usize>,
    ) -> Result<Vec<PathBuf>> {
        let mut stdout = stdout();
        terminal::enable_raw_mode()?;
        execute!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            terminal::Clear(ClearType::All)
        )?;

        let mut search_bar = SearchBar::new();
        let mut result_list = ResultList::new(terminal::size()?.1.saturating_sub(4) as usize);
        let mut selected_paths = Vec::new();

        let (sender, receiver) = bounded(50000);
        let total_indexed = Arc::new(AtomicUsize::new(0));
        let indexing_complete = Arc::new(AtomicBool::new(false));

        let index = Arc::new(self.index.clone());
        let total_indexed_clone = Arc::clone(&total_indexed);
        let indexing_complete_clone = Arc::clone(&indexing_complete);
        let directory = directory.to_string();

        let respect_gitignore = self.respect_gitignore;
        thread::spawn(move || {
            if respect_gitignore {
                stream_gitignore_filtered_entries(&directory, sender.clone(), &total_indexed_clone);
                indexing_complete_clone.store(true, AtomicOrdering::SeqCst);
                return;
            }

            let mut builder = WalkBuilder::new(&directory);
            builder
                .hidden(false)
                .git_ignore(false)
                .git_exclude(false)
                .parents(false)
                .ignore(false)
                .follow_links(false)
                .same_file_system(false)
                .threads(num_cpus::get());
            let walker = builder.build_parallel();

            let (tx, rx) = std::sync::mpsc::channel();

            walker.run(|| {
                let tx = tx.clone();
                let total_indexed = Arc::clone(&total_indexed_clone);
                Box::new(move |entry| {
                    if let Ok(entry) = entry {
                        if entry.file_type().map_or(false, |ft| ft.is_file()) {
                            let _ = tx.send(FileEntry::new(entry.into_path()));
                            total_indexed.fetch_add(1, AtomicOrdering::SeqCst);
                        }
                    }
                    ignore::WalkState::Continue
                })
            });

            let mut batch = Vec::with_capacity(1000);
            while let Ok(entry) = rx.recv() {
                batch.push(entry);
                if batch.len() >= 1000 {
                    let _ = sender.send(batch);
                    batch = Vec::with_capacity(1000);
                }
            }

            if !batch.is_empty() {
                let _ = sender.send(batch);
            }

            indexing_complete_clone.store(true, AtomicOrdering::SeqCst);
        });

        let mut last_query = String::new();
        let mut last_query_time = std::time::Instant::now();
        let mut last_render = std::time::Instant::now();
        let mut last_render_request = std::time::Instant::now();
        let mut last_batch_check = std::time::Instant::now();
        let mut pending_search = false;
        let mut pending_render = false;
        let mut input_mode = InputMode::Normal;
        let mut status_message: Option<String> = None;
        let mut status_message_time: Option<std::time::Instant> = None;

        let search_debounce = Duration::from_millis(SEARCH_DEBOUNCE_MS);
        let status_timeout = Duration::from_millis(STATUS_MESSAGE_TIMEOUT_MS);
        let render_debounce = Duration::from_millis(16);
        let render_interval = Duration::from_millis(RENDER_INTERVAL_MS);
        let batch_check_interval = Duration::from_millis(100);

        self.render_ui(&search_bar, &mut result_list, &input_mode, &status_message)?;
        let results = index.search("", 1000);
        result_list.update_results(results);

        loop {
            let now = std::time::Instant::now();
            let should_check_batch =
                !pending_search && now.duration_since(last_batch_check) >= batch_check_interval;

            if should_check_batch {
                let mut received_new_files = false;
                while let Ok(batch) = receiver.try_recv() {
                    if index.add_entries(batch) {
                        received_new_files = true;
                    }
                }

                if received_new_files {
                    result_list.total_indexed = total_indexed.load(AtomicOrdering::SeqCst);
                    result_list.indexing_complete = indexing_complete.load(AtomicOrdering::SeqCst);
                    if !last_query.is_empty() {
                        let results = index.search(&last_query, 1000);
                        result_list.update_results(results);
                    } else {
                        let results = index.search("", 1000);
                        result_list.update_results(results);
                    }
                    pending_render = true;
                    last_render_request = now;
                }

                last_batch_check = now;
            }

            if event::poll(Duration::from_millis(1))? {
                if let Event::Key(key) = event::read()? {
                    // Handle input based on current mode
                    match &input_mode {
                        InputMode::Rename { buffer, cursor } => {
                            // Handle rename mode input
                            match (key.code, key.modifiers) {
                                (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                    // Cancel rename
                                    input_mode = InputMode::Normal;
                                    status_message = Some("Rename cancelled".to_string());
                                    status_message_time = Some(now);
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                (KeyCode::Enter, _) => {
                                    // Perform rename
                                    if let Some(result) = result_list.get_selected() {
                                        let old_path = result.entry.path.clone();
                                        let new_name = buffer.clone();
                                        if !new_name.is_empty()
                                            && new_name
                                                != old_path
                                                    .file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy()
                                        {
                                            match rename_file(&old_path, &new_name) {
                                                Ok(new_path) => {
                                                    status_message = Some(format!(
                                                        "Renamed to: {}",
                                                        new_path.display()
                                                    ));
                                                    status_message_time = Some(now);

                                                    // Keep multi-select consistent if the renamed file was marked.
                                                    if result_list.multi_selected.remove(&old_path)
                                                    {
                                                        result_list
                                                            .multi_selected
                                                            .insert(new_path.clone());
                                                    }

                                                    // Update underlying search index so future searches reflect the rename.
                                                    index.replace_entry_path(&old_path, &new_path);

                                                    // Refresh results against the current query. If the renamed file no longer matches,
                                                    // it will disappear, which is expected.
                                                    let refreshed =
                                                        index.search(&search_bar.query, 1000);
                                                    result_list.update_results(refreshed);
                                                    if let Some(pos) = result_list
                                                        .results
                                                        .iter()
                                                        .position(|r| r.entry.path == new_path)
                                                    {
                                                        result_list.selected_idx = pos;
                                                        result_list.update_window();
                                                    }
                                                }
                                                Err(e) => {
                                                    status_message =
                                                        Some(format!("Rename failed: {}", e));
                                                    status_message_time = Some(now);
                                                }
                                            }
                                        }
                                    }
                                    input_mode = InputMode::Normal;
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                (KeyCode::Backspace, _) => {
                                    let mut new_buffer = buffer.clone();
                                    let mut new_cursor = *cursor;
                                    if new_cursor > 0 {
                                        new_cursor -= 1;
                                        new_buffer.remove(new_cursor);
                                    }
                                    input_mode = InputMode::Rename {
                                        buffer: new_buffer,
                                        cursor: new_cursor,
                                    };
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                (KeyCode::Left, _) => {
                                    let new_cursor = cursor.saturating_sub(1);
                                    input_mode = InputMode::Rename {
                                        buffer: buffer.clone(),
                                        cursor: new_cursor,
                                    };
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                (KeyCode::Right, _) => {
                                    let new_cursor = (*cursor + 1).min(buffer.len());
                                    input_mode = InputMode::Rename {
                                        buffer: buffer.clone(),
                                        cursor: new_cursor,
                                    };
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                                    let mut new_buffer = buffer.clone();
                                    new_buffer.insert(*cursor, c);
                                    input_mode = InputMode::Rename {
                                        buffer: new_buffer,
                                        cursor: cursor + 1,
                                    };
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                _ => {}
                            }
                        }
                        InputMode::Normal => {
                            // Normal mode key handling
                            match (key.code, key.modifiers) {
                                (KeyCode::Char('c'), KeyModifiers::CONTROL)
                                | (KeyCode::Esc, KeyModifiers::NONE) => break,
                                (KeyCode::Enter, KeyModifiers::NONE) => {
                                    if result_list.multi_selected.is_empty() {
                                        if let Some(result) = result_list.get_selected() {
                                            selected_paths.push(result.entry.path.clone());
                                        }
                                    } else {
                                        selected_paths
                                            .extend(result_list.multi_selected.iter().cloned());
                                    }
                                    break;
                                }
                                // Toggle multi-select with Space
                                (KeyCode::Char(' '), KeyModifiers::NONE) => {
                                    if let Some(result) = result_list.get_selected() {
                                        let p = result.entry.path.clone();
                                        result_list.toggle_mark(&p);
                                        pending_render = true;
                                        last_render_request = now;
                                    }
                                }
                                // Copy selected paths to clipboard (Ctrl+Y)
                                (KeyCode::Char(c), modifiers)
                                    if matches!(c, 'y' | 'Y')
                                        && modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    let mut paths: Vec<PathBuf> =
                                        if result_list.multi_selected.is_empty() {
                                            result_list
                                                .get_selected()
                                                .map(|r| vec![r.entry.path.clone()])
                                                .unwrap_or_default()
                                        } else {
                                            result_list.multi_selected.iter().cloned().collect()
                                        };
                                    paths.sort();
                                    let content = paths
                                        .iter()
                                        .map(|p| p.to_string_lossy().to_string())
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    let _ = copy_to_clipboard(&content);
                                    status_message =
                                        Some("Path(s) copied to clipboard".to_string());
                                    status_message_time = Some(now);
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                // Open selected paths with system handler (Ctrl+O)
                                (KeyCode::Char(c), modifiers)
                                    if matches!(c, 'o' | 'O')
                                        && modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    let paths: Vec<PathBuf> =
                                        if result_list.multi_selected.is_empty() {
                                            result_list
                                                .get_selected()
                                                .map(|r| vec![r.entry.path.clone()])
                                                .unwrap_or_default()
                                        } else {
                                            result_list.multi_selected.iter().cloned().collect()
                                        };
                                    for p in paths {
                                        let _ = open_path(&p);
                                    }
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                // Open in external editor (Ctrl+E)
                                (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                                    let paths: Vec<PathBuf> =
                                        if result_list.multi_selected.is_empty() {
                                            result_list
                                                .get_selected()
                                                .map(|r| vec![r.entry.path.clone()])
                                                .unwrap_or_default()
                                        } else {
                                            result_list.multi_selected.iter().cloned().collect()
                                        };

                                    if !paths.is_empty() {
                                        execute!(
                                            stdout,
                                            terminal::LeaveAlternateScreen,
                                            cursor::Show
                                        )?;
                                        terminal::disable_raw_mode()?;

                                        let open_res = open_in_editor(
                                            &paths,
                                            self.config.listers.fuzzy.editor.as_deref(),
                                        );

                                        terminal::enable_raw_mode()?;
                                        execute!(
                                            stdout,
                                            terminal::EnterAlternateScreen,
                                            cursor::Hide
                                        )?;

                                        let after = std::time::Instant::now();
                                        match open_res {
                                            Ok(()) => {
                                                status_message = Some(format!(
                                                    "Opened editor for {} file(s)",
                                                    paths.len()
                                                ));
                                            }
                                            Err(e) => {
                                                status_message =
                                                    Some(format!("Failed to open editor: {}", e));
                                            }
                                        }
                                        status_message_time = Some(after);
                                        pending_render = true;
                                        last_render_request = after;
                                    }
                                }
                                // Rename file (F2)
                                (KeyCode::F(2), _) => {
                                    if let Some(result) = result_list.get_selected() {
                                        let file_name = result
                                            .entry
                                            .path
                                            .file_name()
                                            .map(|n| n.to_string_lossy().to_string())
                                            .unwrap_or_default();
                                        let cursor = file_name.len();
                                        input_mode = InputMode::Rename {
                                            buffer: file_name,
                                            cursor,
                                        };
                                        pending_render = true;
                                        last_render_request = now;
                                    }
                                }
                                (KeyCode::Up, KeyModifiers::NONE) => {
                                    result_list.move_selection(-1);
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                (KeyCode::Down, KeyModifiers::NONE) => {
                                    result_list.move_selection(1);
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                // Vim-style navigation: Ctrl-K (up) and Ctrl-J (down)
                                (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                                    result_list.move_selection(-1);
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                                    result_list.move_selection(1);
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                // Emacs/Vim-style navigation: Ctrl-P (up) and Ctrl-N (down)
                                (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                                    result_list.move_selection(-1);
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                                    result_list.move_selection(1);
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                // Page navigation: Ctrl-U (half page up) and Ctrl-D (half page down)
                                (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                                    result_list.move_page(-1);
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                                    result_list.move_page(1);
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                // Jump to end: Ctrl-G
                                (KeyCode::Char(c), modifiers)
                                    if matches!(c, 'g' | 'G')
                                        && modifiers.contains(KeyModifiers::CONTROL)
                                        && modifiers.contains(KeyModifiers::SHIFT) =>
                                {
                                    result_list.jump_to_start();
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                (KeyCode::Char(c), modifiers)
                                    if matches!(c, 'g' | 'G')
                                        && modifiers.contains(KeyModifiers::CONTROL)
                                        && !modifiers.contains(KeyModifiers::SHIFT) =>
                                {
                                    result_list.jump_to_end();
                                    pending_render = true;
                                    last_render_request = now;
                                }
                                _ => {
                                    if search_bar.handle_input(key.code, key.modifiers) {
                                        last_query = search_bar.query.clone();
                                        last_query_time = now;
                                        pending_search = true;
                                        pending_render = true;
                                        last_render_request = now;
                                        // Clear status message when typing
                                        status_message = None;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if pending_search && now.duration_since(last_query_time) >= search_debounce {
                let results = index.search(&last_query, 1000);
                result_list.selected_idx = 0;
                result_list.window_start = 0;
                result_list.update_results(results);
                pending_search = false;
                pending_render = true;
                last_render_request = now;
            }

            // Clear status message after timeout
            if let Some(msg_time) = status_message_time {
                if now.duration_since(msg_time) >= status_timeout {
                    status_message = None;
                    status_message_time = None;
                    pending_render = true;
                    last_render_request = now;
                }
            }

            if pending_render
                && now.duration_since(last_render_request) >= render_debounce
                && now.duration_since(last_render) >= render_interval
            {
                self.render_ui(&search_bar, &mut result_list, &input_mode, &status_message)?;
                last_render = now;
                pending_render = false;
            }

            thread::sleep(Duration::from_millis(1));
        }

        execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
        terminal::disable_raw_mode()?;

        Ok(selected_paths)
    }

    fn render_ui(
        &self,
        search_bar: &SearchBar,
        result_list: &mut ResultList,
        input_mode: &InputMode,
        status_message: &Option<String>,
    ) -> io::Result<()> {
        let mut stdout = stdout();
        let (width, height) = terminal::size()?;
        let available_height = height.saturating_sub(4) as usize;

        execute!(
            stdout,
            cursor::MoveTo(0, 0),
            terminal::Clear(ClearType::All)
        )?;

        // Render search bar
        let search_bar_str = search_bar.render(width);
        execute!(stdout, cursor::MoveTo(0, 0), style::Print(&search_bar_str))?;

        // Render separator line
        execute!(
            stdout,
            cursor::MoveTo(0, 1),
            style::Print("─".repeat(width as usize).bright_black())
        )?;

        // Render result list
        let result_lines = result_list.render(width);
        for (i, line) in result_lines.iter().take(available_height).enumerate() {
            // Truncate line to fit (ANSI-aware, prevents wrapping)
            let truncated = console::truncate_str(line, width as usize, "");
            execute!(
                stdout,
                cursor::MoveTo(0, (i + 2) as u16),
                style::Print(truncated.as_ref())
            )?;
        }

        let status_line = match input_mode {
            InputMode::Rename { buffer, cursor: _ } => {
                format!(
                    "{} {} {} {}",
                    " RENAME:".bold().yellow(),
                    buffer.clone().bold(),
                    "│".bright_black(),
                    "Enter: confirm, Esc: cancel".bright_black()
                )
            }
            InputMode::Normal => {
                if let Some(msg) = status_message {
                    format!(
                        "{}{} {} {}",
                        " Total: ".bold(),
                        result_list.results.len().to_string().yellow(),
                        "│".bright_black(),
                        msg.clone().green()
                    )
                } else {
                    format!(
                        "{}{}{}{}{}",
                        " Total: ".bold(),
                        result_list.results.len().to_string().yellow(),
                        format!(
                            " (showing {}-{} of {})",
                            result_list.window_start + 1,
                            (result_list.window_start + available_height)
                                .min(result_list.results.len()),
                            result_list.total_indexed
                        )
                        .bright_black(),
                        " • ",
                        "^j/k:nav Space:sel ^e:edit F2:rename ^y:cp ^o:open".bright_black()
                    )
                }
            }
        };

        execute!(
            stdout,
            cursor::MoveTo(0, height - 1),
            terminal::Clear(ClearType::CurrentLine),
            style::Print(&status_line),
            cursor::MoveTo((search_bar.cursor_pos + 4) as u16, 0)
        )?;

        stdout.flush()
    }
}

impl FileLister for FuzzyLister {
    fn list_files(
        &self,
        directory: &str,
        recursive: bool,
        depth: Option<usize>,
    ) -> Result<Vec<PathBuf>> {
        self.run_interactive(directory, recursive, depth)
    }
}

struct SearchBar {
    query: String,
    cursor_pos: usize,
}

impl SearchBar {
    fn new() -> Self {
        Self {
            query: String::new(),
            cursor_pos: 0,
        }
    }

    fn render(&self, width: u16) -> String {
        let theme = get_theme();
        let prompt = "    ".to_string();
        let input = if self.query.is_empty() {
            "Type to search...".to_string().bright_black().to_string()
        } else {
            self.query.clone()
        };

        let cursor = if !self.query.is_empty() && self.cursor_pos == self.query.len() {
            "▎"
                .color(color_value_to_color(&theme.colors.permission_exec))
                .to_string()
        } else {
            " ".to_string()
        };

        let content_len = prompt.len() + input.len() + cursor.len() + 4;
        let padding = " ".repeat((width as usize).saturating_sub(content_len));

        let border_color = color_value_to_color(&theme.colors.permission_none);
        let input_color = if self.query.is_empty() {
            input
        } else {
            input
                .color(color_value_to_color(&theme.colors.file))
                .bold()
                .to_string()
        };

        format!(
            "{}{}{}{}",
            prompt.color(border_color),
            input_color,
            cursor,
            padding
        )
    }

    fn handle_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        match (key, modifiers) {
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                self.query.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
                true
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.query.remove(self.cursor_pos);
                    true
                } else {
                    false
                }
            }
            // Ctrl-H: Delete character (like Backspace)
            (KeyCode::Char('h'), KeyModifiers::CONTROL) => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.query.remove(self.cursor_pos);
                    true
                } else {
                    false
                }
            }
            // Ctrl-W: Delete word backwards
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                if self.cursor_pos > 0 {
                    let original_pos = self.cursor_pos;
                    // Skip trailing whitespace
                    while self.cursor_pos > 0
                        && self.query.chars().nth(self.cursor_pos - 1) == Some(' ')
                    {
                        self.cursor_pos -= 1;
                    }
                    // Delete until whitespace or start
                    while self.cursor_pos > 0
                        && self.query.chars().nth(self.cursor_pos - 1) != Some(' ')
                    {
                        self.cursor_pos -= 1;
                    }
                    // Remove the characters
                    self.query.drain(self.cursor_pos..original_pos);
                    true
                } else {
                    false
                }
            }
            // Ctrl-A: Move cursor to start of line (Home also works)
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                if self.cursor_pos > 0 {
                    self.cursor_pos = 0;
                    true
                } else {
                    false
                }
            }
            // Note: Ctrl-E is reserved for external editor; use End key for end-of-line
            (KeyCode::Left, KeyModifiers::NONE) => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    true
                } else {
                    false
                }
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                if self.cursor_pos < self.query.len() {
                    self.cursor_pos += 1;
                    true
                } else {
                    false
                }
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                if self.cursor_pos > 0 {
                    self.cursor_pos = 0;
                    true
                } else {
                    false
                }
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                if self.cursor_pos < self.query.len() {
                    self.cursor_pos = self.query.len();
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

struct ResultList {
    results: Vec<MatchResult>,
    selected_idx: usize,
    window_start: usize,
    max_visible: usize,
    total_indexed: usize,
    indexing_complete: bool,
    multi_selected: HashSet<PathBuf>,
}

impl ResultList {
    fn new(max_visible: usize) -> Self {
        Self {
            results: Vec::new(),
            selected_idx: 0,
            window_start: 0,
            max_visible,
            total_indexed: 0,
            indexing_complete: false,
            multi_selected: HashSet::new(),
        }
    }

    fn get_selected(&mut self) -> Option<&MatchResult> {
        self.results.get(self.selected_idx)
    }

    fn update_results(&mut self, results: Vec<MatchResult>) -> bool {
        let changed = self.results.len() != results.len()
            || self.results.iter().zip(results.iter()).any(|(a, b)| {
                a.score != b.score || a.positions != b.positions || a.entry.path != b.entry.path
            });

        if changed {
            self.results = results;
            self.selected_idx = self.selected_idx.min(self.results.len().saturating_sub(1));
            self.update_window();
        }

        changed
    }

    fn update_window(&mut self) {
        if self.selected_idx >= self.window_start + self.max_visible {
            self.window_start = self.selected_idx - self.max_visible + 1;
        } else if self.selected_idx < self.window_start {
            self.window_start = self.selected_idx;
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let new_idx = self.selected_idx as i32 + delta;
        if new_idx >= 0 && new_idx < self.results.len() as i32 {
            self.selected_idx = new_idx as usize;
            self.update_window();
        }
    }

    fn move_page(&mut self, direction: i32) {
        let half_page = (self.max_visible / 2).max(1) as i32;
        let delta = direction * half_page;
        let new_idx = (self.selected_idx as i32 + delta)
            .max(0)
            .min(self.results.len().saturating_sub(1) as i32);
        self.selected_idx = new_idx as usize;
        self.update_window();
    }

    fn jump_to_end(&mut self) {
        if !self.results.is_empty() {
            self.selected_idx = self.results.len() - 1;
            self.update_window();
        }
    }

    fn jump_to_start(&mut self) {
        self.selected_idx = 0;
        self.update_window();
    }

    fn toggle_mark(&mut self, path: &PathBuf) {
        if self.multi_selected.contains(path) {
            self.multi_selected.remove(path);
        } else {
            self.multi_selected.insert(path.clone());
        }
    }

    fn render(&mut self, width: u16) -> Vec<String> {
        let theme = get_theme();
        let max_width = width as usize;

        if self.results.is_empty() {
            return vec![format!(
                "  {} {}",
                "".color(color_value_to_color(&theme.colors.directory)),
                if !self.indexing_complete {
                    format!(
                        "Indexing files... {} files found",
                        self.total_indexed.to_string().yellow()
                    )
                } else {
                    format!("No matches found (indexed {} files)", self.total_indexed)
                }
                .color(color_value_to_color(&theme.colors.permission_none))
            )];
        }

        self.results
            .iter()
            .skip(self.window_start)
            .take(self.max_visible)
            .enumerate()
            .map(|(idx, result)| {
                let is_selected = idx + self.window_start == self.selected_idx;
                let path = &result.entry.path;
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let metadata = path.metadata().ok();
                let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified = metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or_else(SystemTime::now);

                let path_str = path.to_string_lossy();
                let truncated_path = if path_str.len() > max_width.saturating_sub(60) {
                    let components: Vec<_> = path.components().collect();
                    if components.len() <= 2 {
                        path_str.to_string()
                    } else {
                        let mut path_parts = Vec::new();
                        path_parts.push(components[0].as_os_str().to_string_lossy().to_string());
                        if components.len() > 3 {
                            path_parts.push("...".to_string());
                        }
                        path_parts.push(
                            components[components.len() - 2]
                                .as_os_str()
                                .to_string_lossy()
                                .to_string(),
                        );
                        path_parts.push(file_name.to_string());
                        path_parts.join("/")
                    }
                } else {
                    path_str.to_string()
                };

                let is_marked = self.multi_selected.contains(path);
                let name_display = if is_selected {
                    format_with_icon(
                        path,
                        file_name
                            .color(color_value_to_color(&theme.colors.directory))
                            .bold()
                            .underline()
                            .to_string(),
                        true,
                    )
                } else {
                    format_with_icon(path, colorize_file_name(path).to_string(), true)
                };

                let prefix = if is_marked {
                    "●".bold()
                } else if is_selected {
                    "→".bold()
                } else {
                    " ".normal()
                };

                let perms = metadata
                    .as_ref()
                    .map(|m| m.permissions())
                    .unwrap_or_else(|| Permissions::from_mode(0o644));
                let perms_display = colorize_permissions(&perms, Some("symbolic"));
                let size_display = colorize_size(size);
                let date_display = colorize_date(&modified);

                format!(
                    "  {} {}  {}  {} {} {}",
                    prefix,
                    name_display,
                    truncated_path.color(if is_selected {
                        color_value_to_color(&theme.colors.directory)
                    } else {
                        color_value_to_color(&theme.colors.permission_none)
                    }),
                    perms_display,
                    size_display,
                    date_display
                )
            })
            .collect()
    }
}
