use super::FileLister;
use crate::commands::args::Args;
use crate::utils::color::*;
use crate::utils::icons::format_with_icon;
use crate::{error::Result, theme::color_value_to_color};
use colored::*;
use crossbeam_channel::bounded;
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
use std::path::PathBuf;
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
}

impl SearchIndex {
    fn new(config: crate::config::Config) -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::with_capacity(10000))),
            matcher: Arc::new(SkimMatcherV2::default().ignore_case()),
            last_query: Arc::new(RwLock::new(String::new())),
            last_results: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }

    fn should_ignore_path(&self, path: &std::path::Path) -> bool {
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
}

impl FuzzyLister {
    pub fn new(config: crate::config::Config) -> Self {
        Self {
            index: SearchIndex::new(config.clone()),
            config,
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

        thread::spawn(move || {
            let walker = WalkBuilder::new(&directory)
                .hidden(false)
                .git_ignore(false)
                .ignore(false)
                .follow_links(false)
                .same_file_system(false)
                .threads(num_cpus::get())
                .build_parallel();

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

        let search_debounce = Duration::from_millis(SEARCH_DEBOUNCE_MS);
        let render_debounce = Duration::from_millis(16);
        let render_interval = Duration::from_millis(RENDER_INTERVAL_MS);
        let batch_check_interval = Duration::from_millis(100);

        self.render_ui(&search_bar, &mut result_list)?;
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
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL)
                        | (KeyCode::Esc, KeyModifiers::NONE) => break,
                        (KeyCode::Enter, KeyModifiers::NONE) => {
                            if result_list.multi_selected.is_empty() {
                                if let Some(result) = result_list.get_selected() {
                                    selected_paths.push(result.entry.path.clone());
                                }
                            } else {
                                selected_paths.extend(result_list.multi_selected.iter().cloned());
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
                        // Copy selected paths to clipboard (y)
                        (KeyCode::Char('y'), KeyModifiers::NONE) => {
                            let mut paths: Vec<PathBuf> = if result_list.multi_selected.is_empty() {
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
                            pending_render = true;
                            last_render_request = now;
                        }
                        // Open selected paths (o)
                        (KeyCode::Char('o'), KeyModifiers::NONE) => {
                            let paths: Vec<PathBuf> = if result_list.multi_selected.is_empty() {
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
                        _ => {
                            if search_bar.handle_input(key.code, key.modifiers) {
                                last_query = search_bar.query.clone();
                                last_query_time = now;
                                pending_search = true;
                                pending_render = true;
                                last_render_request = now;
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

            if pending_render
                && now.duration_since(last_render_request) >= render_debounce
                && now.duration_since(last_render) >= render_interval
            {
                self.render_ui(&search_bar, &mut result_list)?;
                last_render = now;
                pending_render = false;
            }

            thread::sleep(Duration::from_millis(1));
        }

        execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
        terminal::disable_raw_mode()?;

        Ok(selected_paths)
    }

    fn render_ui(&self, search_bar: &SearchBar, result_list: &mut ResultList) -> io::Result<()> {
        let mut stdout = stdout();
        let (width, height) = terminal::size()?;
        let available_height = height.saturating_sub(4) as usize;

        execute!(
            stdout,
            cursor::MoveTo(0, 0),
            terminal::Clear(ClearType::All),
            style::Print(&search_bar.render(width)),
            cursor::MoveTo(0, 1),
            style::Print("─".repeat(width as usize).bright_black())
        )?;

        let result_lines = result_list.render(width);
        for (i, line) in result_lines.iter().take(available_height).enumerate() {
            execute!(
                stdout,
                cursor::MoveTo(0, (i + 2) as u16),
                style::Print(line)
            )?;
        }

        let status_line = format!(
            "{}{}{}{}{}",
            " Total: ".bold(),
            result_list.results.len().to_string().yellow(),
            format!(
                " (showing {}-{} of {})",
                result_list.window_start + 1,
                (result_list.window_start + available_height).min(result_list.results.len()),
                result_list.total_indexed
            )
            .bright_black(),
            " • ",
            "Space: select, Enter: confirm, y: copy, o: open".bright_black()
        );

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
        _args: &Args,
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
