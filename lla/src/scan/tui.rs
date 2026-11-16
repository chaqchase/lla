//! Interactive view for `lla scan`.

use super::clean::{self, Method, Target};
use super::disk::{self, Volume};
use super::format;
use super::model::{self, culprit, Report, Scan};
use super::rules::{artifact_info, Category, Rules, Safety};
use super::walk::Node;
use super::{now, unreadable_hint, Job, RootPhase};
use crate::error::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::{cursor, execute, terminal};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::{Frame, Terminal};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAP_ROWS: usize = 3;

type Rgb = (u8, u8, u8);

const ACCENT: Rgb = (125, 170, 255);
const GREEN: Rgb = (120, 212, 140);
const AMBER: Rgb = (240, 190, 90);
const RED: Rgb = (248, 100, 100);
const DIM: Rgb = (120, 123, 136);
const FAINT: Rgb = (64, 66, 76);

fn category_rgb(category: Category) -> Rgb {
    match category {
        Category::Apps => (94, 145, 255),
        Category::Developer => (104, 211, 124),
        Category::Caches => (255, 153, 72),
        Category::Logs => (186, 168, 124),
        Category::Downloads => (64, 201, 183),
        Category::Documents => (88, 196, 240),
        Category::Photos => (244, 114, 182),
        Category::Videos => (167, 124, 250),
        Category::Music => (250, 204, 82),
        Category::Trash => (248, 93, 93),
        Category::Other => (170, 174, 186),
        Category::System => (108, 112, 124),
    }
}

fn safety_rgb(safety: Safety) -> Rgb {
    match safety {
        Safety::Safe => GREEN,
        Safety::Review => AMBER,
        Safety::Keep => DIM,
    }
}

#[derive(Clone, Copy)]
struct Palette {
    truecolor: bool,
    mono: bool,
}

impl Palette {
    fn detect() -> Self {
        let mono = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let truecolor = colorterm.contains("truecolor")
            || colorterm.contains("24bit")
            || std::env::var_os("WT_SESSION").is_some()
            || matches!(
                program.as_str(),
                "iTerm.app" | "WezTerm" | "ghostty" | "vscode" | "Hyper" | "rio" | "Tabby"
            );
        Self { truecolor, mono }
    }

    fn color(&self, rgb: Rgb) -> Color {
        if self.mono {
            Color::Reset
        } else if self.truecolor {
            Color::Rgb(rgb.0, rgb.1, rgb.2)
        } else {
            Color::Indexed(ansi256(rgb))
        }
    }

    fn fg(&self, rgb: Rgb) -> Style {
        Style::default().fg(self.color(rgb))
    }

    fn dim(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            self.fg(DIM)
        }
    }
}

fn ansi256((r, g, b): Rgb) -> u8 {
    if r == g && g == b {
        return match r {
            0..=7 => 16,
            249..=255 => 231,
            _ => 232 + ((u16::from(r) - 8) * 24 / 241) as u8,
        };
    }
    let level = |v: u8| ((u16::from(v) * 5 + 127) / 255) as u8;
    16 + 36 * level(r) + 6 * level(g) + level(b)
}

fn shade((r, g, b): Rgb, factor: f32) -> Rgb {
    let scale = |v: u8| (f32::from(v) * factor).clamp(0.0, 255.0) as u8;
    (scale(r), scale(g), scale(b))
}

/// Deterministic per-cell noise so the disk map has some texture.
fn noise(x: usize, y: usize) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h & 0xffff) as f32 / 65_535.0
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Categories,
    Items,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Size,
    Age,
    Name,
}

impl Sort {
    fn next(self) -> Self {
        match self {
            Sort::Size => Sort::Age,
            Sort::Age => Sort::Name,
            Sort::Name => Sort::Size,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Sort::Size => "size",
            Sort::Age => "oldest first",
            Sort::Name => "name",
        }
    }
}

#[derive(Clone)]
struct Browse {
    path: PathBuf,
    /// The item this drill-down started from.
    base: PathBuf,
    safety: Safety,
    reason: &'static str,
    permanent: bool,
    sel: usize,
    offset: usize,
}

#[derive(Clone)]
struct Row {
    name: String,
    location: String,
    path: Option<PathBuf>,
    size: u64,
    files: u64,
    newest: i64,
    dir: bool,
    safety: Safety,
    reason: &'static str,
    permanent: bool,
    protected: bool,
    loose: bool,
}

impl Row {
    fn markable(&self) -> bool {
        self.path.is_some() && !self.loose && !self.protected
    }
}

#[derive(Clone, Copy)]
struct Mark {
    size: u64,
    permanent: bool,
}

enum Modal {
    Confirm {
        method: Method,
        targets: Vec<Target>,
    },
    Help,
}

#[derive(Clone, Copy)]
enum Tone {
    Good,
    Warn,
    Info,
}

struct Toast {
    text: String,
    tone: Tone,
    at: Instant,
}

struct App {
    rules: Rules,
    targets: Vec<PathBuf>,
    custom: bool,
    job: Option<Job>,
    scan: Option<Scan>,
    report: Option<Report>,
    volume: Option<Volume>,
    order: Vec<Category>,
    cat: usize,
    focus: Focus,
    sel: usize,
    offset: usize,
    browse: Vec<Browse>,
    rows: Vec<Row>,
    marks: BTreeMap<PathBuf, Mark>,
    filter: bool,
    sort: Sort,
    modal: Option<Modal>,
    toast: Option<Toast>,
    tick: u64,
    done_tick: u64,
    pal: Palette,
    quit: bool,
    /// Rows that fit in the list, updated on every draw.
    page: usize,
}

struct Guard;

impl Guard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(error) = execute!(io::stdout(), terminal::EnterAlternateScreen, cursor::Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        Ok(Self)
    }
}

fn restore_terminal() {
    let _ = execute!(io::stdout(), cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

impl Drop for Guard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

pub fn run(rules: Rules, targets: Vec<PathBuf>, custom: bool) -> Result<()> {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));

    let guard = Guard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let job = Job::start(targets.clone(), custom);
    let mut app = App::new(rules, targets, custom, Some(job), Palette::detect());
    let result = app.event_loop(&mut terminal);
    if let Some(job) = &app.job {
        job.cancel();
    }
    drop(terminal);
    drop(guard);
    result
}

impl App {
    fn new(
        rules: Rules,
        targets: Vec<PathBuf>,
        custom: bool,
        job: Option<Job>,
        pal: Palette,
    ) -> Self {
        let volume = targets.first().and_then(|path| disk::volume(path));
        Self {
            job,
            rules,
            targets,
            custom,
            scan: None,
            report: None,
            volume,
            order: Vec::new(),
            cat: 0,
            focus: Focus::Categories,
            sel: 0,
            offset: 0,
            browse: Vec::new(),
            rows: Vec::new(),
            marks: BTreeMap::new(),
            filter: false,
            sort: Sort::Size,
            modal: None,
            toast: None,
            tick: 0,
            done_tick: 0,
            pal,
            quit: false,
            page: 10,
        }
    }

    fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        while !self.quit {
            self.poll_job();
            if self
                .toast
                .as_ref()
                .is_some_and(|toast| toast.at.elapsed() > Duration::from_secs(6))
            {
                self.toast = None;
            }
            terminal.draw(|frame| self.draw(frame))?;
            let animating = self.job.is_some() || self.tick < self.done_tick + 30;
            let wait = if animating { 60 } else { 250 };
            if event::poll(Duration::from_millis(wait))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Release {
                        self.key(key);
                    }
                }
            }
            self.tick += 1;
        }
        Ok(())
    }

    fn poll_job(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        let Some(scan) = job.try_finish() else {
            return;
        };
        self.job = None;
        self.finish(scan);
    }

    fn finish(&mut self, scan: Scan) {
        self.volume = scan.volume.clone().or(self.volume.take());
        self.scan = Some(scan);
        self.done_tick = self.tick;
        self.rebuild();
        self.focus = Focus::Categories;
        self.cat = 0;
        self.sel = 0;
        self.offset = 0;
    }

    fn rebuild(&mut self) {
        let Some(scan) = &self.scan else {
            return;
        };
        let report = model::build(scan, &self.rules, now());
        self.order = report.ordered();
        self.report = Some(report);
        self.cat = self.cat.min(self.order.len().saturating_sub(1));
        // Drop drill-downs into things that are gone.
        while let Some(top) = self.browse.last() {
            if scan.locate(&top.path).is_some() {
                break;
            }
            self.browse.pop();
        }
        self.refresh_rows();
    }

    fn label(&self, category: Category) -> &'static str {
        self.scan
            .as_ref()
            .map_or(category.label(), |scan| scan.label(category))
    }

    fn category(&self) -> Option<Category> {
        self.order.get(self.cat).copied()
    }

    fn refresh_rows(&mut self) {
        self.rows = match self.browse.last() {
            Some(browse) => self.browse_rows(browse),
            None => self.category_rows(),
        };
        let len = self.rows.len();
        let sel = match self.browse.last_mut() {
            Some(browse) => &mut browse.sel,
            None => &mut self.sel,
        };
        *sel = (*sel).min(len.saturating_sub(1));
    }

    fn category_rows(&self) -> Vec<Row> {
        let (Some(report), Some(category)) = (&self.report, self.category()) else {
            return Vec::new();
        };
        let home = self.rules.home.as_deref();
        let now = report.now;
        let items = &report.group(category).items;
        // Only show where things live when they do not all live in the same place.
        let first_parent = items.first().and_then(|item| item.path.parent());
        let one_place = items
            .iter()
            .all(|item| item.loose || item.path.parent() == first_parent);
        let mut rows: Vec<Row> = items
            .iter()
            .filter(|item| !self.filter || item.cleanable(now))
            .map(|item| {
                let (name, location) = if item.loose {
                    (
                        format!("{} smaller files", format::count(item.files)),
                        format::tilde(&item.path, home),
                    )
                } else {
                    let name = item.path.file_name().map_or_else(
                        || item.path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    let parent = if one_place {
                        String::new()
                    } else {
                        item.path
                            .parent()
                            .map(|p| format::tilde(p, home))
                            .unwrap_or_default()
                    };
                    (if item.dir { format!("{name}/") } else { name }, parent)
                };
                Row {
                    name,
                    location,
                    path: (!item.loose).then(|| item.path.clone()),
                    size: item.size,
                    files: item.files,
                    newest: item.newest,
                    dir: item.dir,
                    safety: item.safety,
                    reason: item.reason,
                    permanent: item.permanent,
                    protected: item.protected,
                    loose: item.loose,
                }
            })
            .collect();
        self.sort_rows(&mut rows);
        rows
    }

    fn browse_rows(&self, browse: &Browse) -> Vec<Row> {
        let Some(node) = self
            .scan
            .as_ref()
            .and_then(|scan| scan.locate(&browse.path))
        else {
            return Vec::new();
        };
        let mut rows: Vec<Row> = node
            .children
            .iter()
            .map(|child| {
                let path = browse.path.join(&*child.name);
                let (safety, reason) = match (artifact_info(child.tag), self.rules.exact(&path)) {
                    (_, Some(rule)) => (rule.safety, rule.reason),
                    (Some(artifact), None) => (artifact.safety, artifact.reason),
                    (None, None) => (browse.safety, browse.reason),
                };
                let name = child.name.to_string_lossy();
                Row {
                    name: if child.dir {
                        format!("{name}/")
                    } else {
                        name.into_owned()
                    },
                    location: String::new(),
                    protected: self.rules.is_protected(&path),
                    path: Some(path),
                    size: child.size,
                    files: child.files,
                    newest: child.newest,
                    dir: child.dir,
                    safety,
                    reason,
                    permanent: browse.permanent,
                    loose: false,
                }
            })
            .collect();
        if node.small > 0 {
            rows.push(Row {
                name: format!("{} smaller files", format::count(node.small_count)),
                location: String::new(),
                path: None,
                size: node.small,
                files: node.small_count,
                newest: 0,
                dir: false,
                safety: browse.safety,
                reason: browse.reason,
                permanent: browse.permanent,
                protected: true,
                loose: true,
            });
        }
        self.sort_rows(&mut rows);
        rows
    }

    fn sort_rows(&self, rows: &mut [Row]) {
        match self.sort {
            Sort::Size => rows.sort_by_key(|row| std::cmp::Reverse(row.size)),
            Sort::Age => rows.sort_by(|a, b| {
                let key = |row: &Row| {
                    if row.newest == 0 {
                        i64::MAX
                    } else {
                        row.newest
                    }
                };
                key(a).cmp(&key(b)).then(b.size.cmp(&a.size))
            }),
            Sort::Name => rows.sort_by_key(|a| a.name.to_lowercase()),
        }
    }

    fn selected(&self) -> usize {
        self.browse.last().map_or(self.sel, |browse| browse.sel)
    }

    fn set_selected(&mut self, value: usize) {
        let value = value.min(self.rows.len().saturating_sub(1));
        match self.browse.last_mut() {
            Some(browse) => browse.sel = value,
            None => self.sel = value,
        }
    }

    fn current_row(&self) -> Option<&Row> {
        self.rows.get(self.selected())
    }

    fn toast(&mut self, text: impl Into<String>, tone: Tone) {
        self.toast = Some(Toast {
            text: text.into(),
            tone,
            at: Instant::now(),
        });
    }

    // ── input ────────────────────────────────────────────────────────────

    fn key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c')) {
            self.quit = true;
            return;
        }
        if let Some(modal) = self.modal.take() {
            match (modal, key.code) {
                (Modal::Confirm { method, targets }, KeyCode::Char('y') | KeyCode::Enter) => {
                    self.clean(method, targets)
                }
                (Modal::Help, KeyCode::Char('?')) => {}
                (Modal::Help, _) => {}
                (modal @ Modal::Confirm { .. }, code)
                    if !matches!(code, KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q')) =>
                {
                    self.modal = Some(modal)
                }
                _ => {}
            }
            return;
        }

        if self.report.is_none() {
            if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                self.quit = true;
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Esc => {
                if self.browse.pop().is_some() {
                    self.refresh_rows();
                } else if self.focus == Focus::Items {
                    self.focus = Focus::Categories;
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.step(-1),
            KeyCode::Down | KeyCode::Char('j') => self.step(1),
            KeyCode::PageUp => self.step(-(self.page as isize)),
            KeyCode::PageDown => self.step(self.page as isize),
            KeyCode::Home => self.step(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.step(isize::MAX / 2),
            KeyCode::Tab => self.cycle_category(1),
            KeyCode::BackTab => self.cycle_category(-1),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                if self.browse.pop().is_some() {
                    self.refresh_rows();
                } else {
                    self.focus = Focus::Categories;
                }
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => self.open(),
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Char('a') => self.mark_all(),
            KeyCode::Char('u') => {
                self.marks.clear();
                self.toast("Cleared all marks", Tone::Info);
            }
            KeyCode::Char('d') | KeyCode::Delete => self.ask_clean(Method::Trash),
            KeyCode::Char('D') => self.ask_clean(Method::Delete),
            KeyCode::Char('f') => {
                self.filter = !self.filter;
                self.sel = 0;
                self.offset = 0;
                self.refresh_rows();
            }
            KeyCode::Char('s') => {
                self.sort = self.sort.next();
                self.refresh_rows();
                self.toast(format!("Sorted by {}", self.sort.label()), Tone::Info);
            }
            KeyCode::Char('o') => self.reveal(),
            KeyCode::Char('g') => self.go_to_culprit(),
            KeyCode::Char('r') => self.rescan(),
            _ => {}
        }
    }

    fn step(&mut self, delta: isize) {
        if self.focus == Focus::Categories && self.browse.is_empty() {
            let len = self.order.len() as isize;
            if len == 0 {
                return;
            }
            let next = (self.cat as isize + delta).clamp(0, len - 1) as usize;
            if next != self.cat {
                self.cat = next;
                self.sel = 0;
                self.offset = 0;
                self.refresh_rows();
            }
            return;
        }
        let len = self.rows.len() as isize;
        if len == 0 {
            return;
        }
        let next = (self.selected() as isize)
            .saturating_add(delta)
            .clamp(0, len - 1);
        self.set_selected(next as usize);
    }

    fn cycle_category(&mut self, delta: isize) {
        if self.order.is_empty() {
            return;
        }
        let len = self.order.len() as isize;
        self.cat = (self.cat as isize + delta).rem_euclid(len) as usize;
        self.browse.clear();
        self.sel = 0;
        self.offset = 0;
        self.refresh_rows();
    }

    fn open(&mut self) {
        if self.focus == Focus::Categories && self.browse.is_empty() {
            if self.category() != Some(Category::System) && !self.rows.is_empty() {
                self.focus = Focus::Items;
            }
            return;
        }
        let Some(row) = self.current_row().cloned() else {
            return;
        };
        if !row.dir {
            return;
        }
        let Some(path) = row.path.clone() else {
            return;
        };
        self.push_browse(path, &row);
    }

    fn push_browse(&mut self, path: PathBuf, row: &Row) {
        let base = self
            .browse
            .first()
            .map_or_else(|| path.clone(), |browse| browse.base.clone());
        self.browse.push(Browse {
            path,
            base,
            safety: row.safety,
            reason: row.reason,
            permanent: row.permanent,
            sel: 0,
            offset: 0,
        });
        self.focus = Focus::Items;
        self.refresh_rows();
    }

    fn go_to_culprit(&mut self) {
        let Some(row) = self.current_row().cloned() else {
            return;
        };
        let (Some(path), true) = (row.path.clone(), row.dir) else {
            return;
        };
        let Some(node) = self.scan.as_ref().and_then(|scan| scan.locate(&path)) else {
            return;
        };
        let Some((rel, _)) = culprit(node) else {
            self.toast("No single folder dominates this one", Tone::Info);
            return;
        };
        self.push_browse(path.clone(), &row);
        let mut current = path;
        let parts: Vec<_> = rel.iter().map(|part| part.to_os_string()).collect();
        for (index, part) in parts.iter().enumerate() {
            let target = current.join(part);
            let position = self
                .rows
                .iter()
                .position(|candidate| candidate.path.as_deref() == Some(target.as_path()));
            let Some(position) = position else {
                break;
            };
            self.set_selected(position);
            let last = index + 1 == parts.len();
            let candidate = self.rows[position].clone();
            if last || !candidate.dir {
                break;
            }
            self.push_browse(target.clone(), &candidate);
            current = target;
        }
    }

    fn toggle_mark(&mut self) {
        if self.focus == Focus::Categories && self.browse.is_empty() {
            self.focus = Focus::Items;
            return;
        }
        let Some(row) = self.current_row().cloned() else {
            return;
        };
        if !row.markable() {
            let why = if row.loose {
                "Loose files can only be removed one by one; press ⏎ on the folder"
            } else {
                "This folder holds things your system expects, so lla will not remove it"
            };
            self.toast(why, Tone::Warn);
            return;
        }
        let path = row.path.clone().unwrap_or_default();
        if self.marks.remove(&path).is_none() {
            self.marks.insert(
                path,
                Mark {
                    size: row.size,
                    permanent: row.permanent,
                },
            );
        }
        self.step(1);
    }

    fn mark_all(&mut self) {
        let now = now();
        let in_category = self.browse.is_empty();
        let wanted: Vec<Row> = self
            .rows
            .iter()
            .filter(|row| row.markable())
            .filter(|row| {
                let stale = row.newest > 0 && now - row.newest > model::STALE_AFTER;
                !in_category
                    || match row.safety {
                        Safety::Safe => true,
                        Safety::Review => stale,
                        Safety::Keep => false,
                    }
            })
            .cloned()
            .collect();
        if wanted.is_empty() {
            self.toast("Nothing here is safe to mark in bulk", Tone::Info);
            return;
        }
        let all_marked = wanted.iter().all(|row| {
            row.path
                .as_ref()
                .is_some_and(|p| self.marks.contains_key(p))
        });
        for row in &wanted {
            let path = row.path.clone().unwrap_or_default();
            if all_marked {
                self.marks.remove(&path);
            } else {
                self.marks.insert(
                    path,
                    Mark {
                        size: row.size,
                        permanent: row.permanent,
                    },
                );
            }
        }
        if all_marked {
            self.toast("Unmarked them", Tone::Info);
        } else {
            let label = if in_category { "cleanable" } else { "" };
            let size: u64 = wanted.iter().map(|row| row.size).sum();
            self.toast(
                format!(
                    "Marked {} {label} items, {}",
                    wanted.len(),
                    format::size(size)
                )
                .replace("  ", " "),
                Tone::Info,
            );
        }
        self.focus = Focus::Items;
    }

    /// Marked paths without the ones already covered by a marked parent.
    fn marked_targets(&self) -> Vec<Target> {
        let mut targets: Vec<Target> = Vec::new();
        for (path, mark) in &self.marks {
            // BTreeMap order puts parents before their children.
            if targets.iter().any(|target| path.starts_with(&target.path)) {
                continue;
            }
            targets.push(Target {
                path: path.clone(),
                size: mark.size,
                permanent: mark.permanent,
            });
        }
        targets
    }

    fn ask_clean(&mut self, method: Method) {
        let mut targets = self.marked_targets();
        if targets.is_empty() {
            let Some(row) = self.current_row().cloned() else {
                return;
            };
            if self.focus == Focus::Categories && self.browse.is_empty() {
                self.toast(
                    "Mark items with space first, or move into the list",
                    Tone::Info,
                );
                return;
            }
            if !row.markable() {
                self.toast("lla will not remove this one", Tone::Warn);
                return;
            }
            targets.push(Target {
                path: row.path.clone().unwrap_or_default(),
                size: row.size,
                permanent: row.permanent,
            });
        }
        targets.sort_by_key(|target| std::cmp::Reverse(target.size));
        self.modal = Some(Modal::Confirm { method, targets });
    }

    fn clean(&mut self, method: Method, targets: Vec<Target>) {
        let outcome = clean::clean(&targets, method);
        let trash_folder = self.rules.home.as_deref().and_then(clean::trash_folder);
        if let Some(scan) = &mut self.scan {
            for path in &outcome.trashed {
                if let Some(node) = scan.remove(path) {
                    if let Some(folder) = &trash_folder {
                        scan.insert(folder, node);
                    }
                }
            }
            for path in &outcome.deleted {
                scan.remove(path);
            }
            if let Some(volume) = self.targets.first().and_then(|path| disk::volume(path)) {
                scan.volume = Some(volume.clone());
                self.volume = Some(volume);
            }
        }
        for path in outcome.trashed.iter().chain(&outcome.deleted) {
            self.marks.remove(path);
        }
        self.rebuild();

        let count = outcome.trashed.len() + outcome.deleted.len();
        let size = format::size(outcome.bytes);
        let mut text = if count == 0 {
            String::new()
        } else if !outcome.trashed.is_empty() && outcome.deleted.is_empty() {
            let place = if cfg!(target_os = "macos") {
                "the Trash"
            } else {
                "the trash"
            };
            format!(
                "Moved {} to {place}, {size}. Empty it to get the space back.",
                plural(count, "item")
            )
        } else {
            format!("Removed {}, freed {size}.", plural(count, "item"))
        };
        let tone = if outcome.failed.is_empty() {
            Tone::Good
        } else {
            let (path, error) = &outcome.failed[0];
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&format!(
                "{} failed: {name}: {error}",
                plural(outcome.failed.len(), "item")
            ));
            Tone::Warn
        };
        self.toast(text, tone);
    }

    fn reveal(&mut self) {
        let Some(path) = self
            .current_row()
            .and_then(|row| row.path.clone())
            .or_else(|| self.browse.last().map(|browse| browse.path.clone()))
        else {
            return;
        };
        if let Err(error) = clean::reveal(&path) {
            self.toast(
                format!("Could not open the file manager: {error}"),
                Tone::Warn,
            );
        }
    }

    fn rescan(&mut self) {
        if self.job.is_some() {
            return;
        }
        self.job = Some(Job::start(self.targets.clone(), self.custom));
        self.scan = None;
        self.report = None;
        self.rows.clear();
        self.browse.clear();
        self.marks.clear();
        self.order.clear();
    }

    // ── drawing ──────────────────────────────────────────────────────────

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let inner = Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(1),
        };
        if inner.width < 40 || inner.height < 12 {
            frame.render_widget(
                Paragraph::new("Make the window a little bigger").style(self.pal.dim()),
                inner,
            );
            return;
        }
        let show_map = inner.height >= 22;
        let [header, _, map, _, body, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(if show_map { MAP_ROWS as u16 } else { 0 }),
            Constraint::Length(if show_map { 1 } else { 0 }),
            Constraint::Min(6),
            Constraint::Length(1),
        ])
        .areas(inner);

        self.draw_header(frame, header);
        if show_map {
            self.draw_map(frame, map);
        }
        if self.report.is_some() {
            self.draw_body(frame, body);
        } else {
            self.draw_scanning(frame, body);
        }
        self.draw_footer(frame, footer);

        match &self.modal {
            Some(Modal::Confirm { method, targets }) => {
                self.draw_confirm(frame, area, *method, targets)
            }
            Some(Modal::Help) => self.draw_help(frame, area),
            None => {}
        }
    }

    fn spread(
        &self,
        left: Vec<Span<'static>>,
        right: Vec<Span<'static>>,
        width: u16,
    ) -> Line<'static> {
        let used: usize = left
            .iter()
            .chain(right.iter())
            .map(|span| format::width(&span.content))
            .sum();
        let gap = (width as usize).saturating_sub(used).max(1);
        let mut spans = left;
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(right);
        Line::from(spans)
    }

    fn scope(&self) -> String {
        let home = self.rules.home.as_deref();
        if self.custom {
            return self
                .targets
                .first()
                .map(|path| format::tilde(path, home))
                .unwrap_or_default();
        }
        let mut parts = vec!["home".to_string()];
        parts.extend(
            self.targets
                .iter()
                .skip(1)
                .map(|path| format::tilde(path, home)),
        );
        parts.join(" + ")
    }

    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let pal = self.pal;
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let mut left = vec![
            Span::styled("lla scan", bold),
            Span::styled(format!("  {}", self.scope()), pal.dim()),
        ];
        if let Some(volume) = &self.volume {
            if !volume.label.is_empty() {
                left.push(Span::styled(format!("  ·  {}", volume.label), pal.dim()));
            }
        }
        let right = match &self.job {
            Some(job) => {
                let spinner = SPINNER[(self.tick as usize / 2) % SPINNER.len()];
                vec![
                    Span::styled(format!("{spinner} "), pal.fg(ACCENT)),
                    Span::styled(
                        format!("scanning {:.0}s", job.started.elapsed().as_secs_f64()),
                        pal.dim(),
                    ),
                ]
            }
            None => {
                let unreadable = self.scan.as_ref().map_or(0, Scan::unreadable);
                if unreadable > 0 {
                    vec![
                        Span::styled("▲ ", pal.fg(AMBER)),
                        Span::styled(
                            format!("{} folders unreadable", format::count(unreadable)),
                            pal.fg(AMBER),
                        ),
                        Span::styled(format!(" · {}", unreadable_hint()), pal.dim()),
                    ]
                } else {
                    Vec::new()
                }
            }
        };
        let line1 = self.spread(left, right, area.width);

        let mut stats = Vec::new();
        if let Some(volume) = &self.volume {
            stats.extend([
                Span::styled(format::size(volume.used()), bold),
                Span::styled(
                    format!(" used of {}", format::size(volume.total)),
                    pal.dim(),
                ),
                Span::styled("  ·  ", pal.dim()),
                Span::styled(format::size(volume.free), bold),
                Span::styled(" free", pal.dim()),
                Span::styled("  ·  ", pal.dim()),
                Span::styled(format::percent(volume.used(), volume.total), bold),
                Span::styled(" full", pal.dim()),
            ]);
        }
        let mut right = Vec::new();
        if let Some(report) = &self.report {
            let safe = report.safe_total();
            if safe > 0 {
                right.extend([
                    Span::styled("✦ ", pal.fg(GREEN)),
                    Span::styled(
                        format::size(safe),
                        pal.fg(GREEN).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" safe to clean", pal.dim()),
                ]);
            }
        }
        let line2 = self.spread(stats, right, area.width);
        frame.render_widget(Paragraph::new(vec![line1, line2]), area);
    }

    /// A dotted map of the whole disk, one column block per category.
    fn draw_map(&self, frame: &mut Frame, area: Rect) {
        let pal = self.pal;
        let cols = area.width as usize;
        let cells = cols * MAP_ROWS;
        if cells == 0 {
            return;
        }
        // Which category owns each cell, filled column by column.
        let mut owner: Vec<Option<Category>> = vec![None; cells];
        let total = self.volume.as_ref().map_or(0, |volume| volume.total);
        let selected = if self.focus == Focus::Items || !self.browse.is_empty() {
            self.category()
        } else {
            self.category().filter(|_| self.report.is_some())
        };
        let mut scanned_cells = 0;
        if let (Some(report), true) = (&self.report, total > 0) {
            let mut running = 0u64;
            let mut start = 0usize;
            for category in &self.order {
                running += report.group(*category).size;
                let end = ((running as f64 / total as f64) * cells as f64).round() as usize;
                for cell in owner.iter_mut().take(end.min(cells)).skip(start) {
                    *cell = Some(*category);
                }
                start = start.max(end.min(cells));
            }
        } else if let (Some(job), true) = (&self.job, total > 0) {
            let bytes = job.progress.bytes.load(Ordering::Relaxed);
            scanned_cells = ((bytes as f64 / total as f64) * cells as f64) as usize;
        }

        let reveal = if self.report.is_some() {
            ((self.tick.saturating_sub(self.done_tick)) as f32 / 18.0).min(1.0)
        } else {
            0.0
        };
        let mut lines = Vec::with_capacity(MAP_ROWS);
        for row in 0..MAP_ROWS {
            let mut spans = Vec::with_capacity(cols);
            for col in 0..cols {
                let index = col * MAP_ROWS + row;
                let texture = 0.62 + 0.38 * noise(col, row);
                let (glyph, rgb) = match owner[index] {
                    Some(category) if (col as f32) <= reveal * cols as f32 => {
                        let mut factor = texture;
                        if let Some(selected) = selected {
                            if selected != category {
                                factor *= 0.45;
                            } else {
                                factor = factor.max(0.9);
                            }
                        }
                        ("•", shade(category_rgb(category), factor))
                    }
                    Some(_) => ("•", shade(ACCENT, 0.35 + 0.3 * texture)),
                    None if index < scanned_cells => {
                        let wave =
                            ((self.tick as f32 * 0.25) - col as f32 * 0.12).sin() * 0.5 + 0.5;
                        ("•", shade(ACCENT, 0.35 + 0.65 * wave * texture))
                    }
                    None => ("·", shade(FAINT, 0.7 + 0.3 * texture)),
                };
                spans.push(Span::styled(glyph, pal.fg(rgb)));
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn draw_scanning(&self, frame: &mut Frame, area: Rect) {
        let pal = self.pal;
        let Some(job) = &self.job else {
            return;
        };
        let home = self.rules.home.as_deref();
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let files = job.progress.files.load(Ordering::Relaxed);
        let bytes = job.progress.bytes.load(Ordering::Relaxed);
        let unreadable = job.progress.unreadable.load(Ordering::Relaxed);
        let spinner = SPINNER[(self.tick as usize / 2) % SPINNER.len()];

        let mut lines = vec![
            Line::from(Span::styled("scanning", pal.dim())),
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("{:>14}", format::count(files)), bold),
                Span::styled("  files", pal.dim()),
            ]),
            Line::from(vec![
                Span::styled(format!("{:>14}", format::size(bytes)), bold),
                Span::styled("  found so far", pal.dim()),
            ]),
            Line::from(vec![
                Span::styled(
                    format!(
                        "{:>14}",
                        format!("{:.1}s", job.started.elapsed().as_secs_f64())
                    ),
                    bold,
                ),
                Span::styled("  elapsed", pal.dim()),
            ]),
        ];
        if unreadable > 0 {
            lines.push(Line::from(vec![
                Span::styled(format!("{:>14}", format::count(unreadable)), pal.fg(AMBER)),
                Span::styled("  folders unreadable", pal.dim()),
            ]));
        }
        lines.push(Line::from(""));
        for (path, phase) in job.roots.lock().iter() {
            let (mark, style, detail) = match phase {
                RootPhase::Waiting => ("·", pal.dim(), String::new()),
                RootPhase::Scanning => (spinner, pal.fg(ACCENT), String::new()),
                RootPhase::Done(size) => ("✓", pal.fg(GREEN), format::size(*size)),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {mark} "), style),
                Span::raw(format!(
                    "{:<28}",
                    format::fit(&format::tilde(path, home), 28)
                )),
                Span::styled(detail, pal.dim()),
            ]));
        }
        lines.push(Line::from(""));
        let current = job.progress.current.lock().clone();
        let current = format::tilde(&current, home);
        lines.push(Line::from(Span::styled(
            format!(
                "  {}",
                format::fit_start(&current, area.width.saturating_sub(4) as usize)
            ),
            pal.dim(),
        )));
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn draw_body(&mut self, frame: &mut Frame, area: Rect) {
        let left_width = if area.width >= 110 {
            38
        } else if area.width >= 80 {
            32
        } else {
            0
        };
        let [left, _, right] = Layout::horizontal([
            Constraint::Length(left_width),
            Constraint::Length(if left_width > 0 { 3 } else { 0 }),
            Constraint::Min(30),
        ])
        .areas(area);
        if left_width > 0 {
            self.draw_categories(frame, left);
        }
        self.draw_items(frame, right);
    }

    fn draw_categories(&self, frame: &mut Frame, area: Rect) {
        let pal = self.pal;
        let Some(report) = &self.report else {
            return;
        };
        let focused = self.focus == Focus::Categories && self.browse.is_empty();
        let largest = self
            .order
            .iter()
            .map(|category| report.group(*category).size)
            .max()
            .unwrap_or(1)
            .max(1);
        let bar_width = (area.width as usize)
            .saturating_sub(2 + 2 + 16 + 9 + 2)
            .clamp(4, 12);
        let mut lines = vec![
            Line::from(Span::styled("categories", pal.dim())),
            Line::from(""),
        ];
        for (index, category) in self.order.iter().enumerate() {
            let group = report.group(*category);
            let selected = index == self.cat;
            let cursor = if selected { "› " } else { "  " };
            let mut label_style = Style::default();
            if selected {
                label_style = label_style.add_modifier(Modifier::BOLD);
                if focused && !pal.mono {
                    label_style = label_style.fg(pal.color(category_rgb(*category)));
                }
            }
            let filled = ((group.size as f64 / largest as f64) * bar_width as f64).ceil() as usize;
            let filled = filled.min(bar_width);
            lines.push(Line::from(vec![
                Span::styled(cursor, pal.fg(ACCENT)),
                Span::styled("■ ", pal.fg(category_rgb(*category))),
                Span::styled(
                    format!("{:<16}", format::fit(self.label(*category), 16)),
                    label_style,
                ),
                Span::styled(
                    format!("{:>9}", format::size(group.size)),
                    if selected {
                        Style::default().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
                Span::raw("  "),
                Span::styled("•".repeat(filled), pal.fg(category_rgb(*category))),
                Span::styled("·".repeat(bar_width - filled), pal.fg(FAINT)),
            ]));
        }
        if let Some(scan) = &self.scan {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!(
                    "  {} files in {:.1}s",
                    format::count(scan.files()),
                    scan.elapsed.as_secs_f64()
                ),
                pal.dim(),
            )));
        }
        if !self.marks.is_empty() {
            let targets = self.marked_targets();
            let size: u64 = targets.iter().map(|target| target.size).sum();
            lines.push(Line::from(vec![
                Span::styled("  ◉ ", pal.fg(ACCENT)),
                Span::styled(
                    format!("{} marked · {}", targets.len(), format::size(size)),
                    pal.fg(ACCENT),
                ),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn draw_items(&mut self, frame: &mut Frame, area: Rect) {
        let pal = self.pal;
        let Some(category) = self.category() else {
            frame.render_widget(
                Paragraph::new("Nothing found to show.").style(pal.dim()),
                area,
            );
            return;
        };
        let Some(report) = &self.report else {
            return;
        };
        let home = self.rules.home.as_deref();
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let color = category_rgb(category);
        let group = report.group(category);
        let disk_total = self.volume.as_ref().map_or(0, |volume| volume.total);
        let width = area.width;
        let mut lines: Vec<Line> = Vec::new();

        let parent_size;
        match self.browse.last() {
            None => {
                parent_size = group.size;
                let mut right = vec![
                    Span::styled(format::size(group.size), bold),
                    Span::styled(
                        format!("  {} of disk", format::percent(group.size, disk_total)),
                        pal.dim(),
                    ),
                ];
                if category != Category::System {
                    let dominant = group
                        .items
                        .iter()
                        .filter(|item| !item.loose)
                        .map(|item| item.safety)
                        .min()
                        .unwrap_or(Safety::Keep);
                    right.push(Span::styled(
                        format!("  {}", dominant.label()),
                        pal.fg(safety_rgb(dominant)),
                    ));
                }
                lines.push(self.spread(
                    vec![Span::styled(
                        self.label(category).to_string(),
                        pal.fg(color).add_modifier(Modifier::BOLD),
                    )],
                    right,
                    width,
                ));
                let about = self
                    .scan
                    .as_ref()
                    .map_or(category.about(), |scan| scan.about(category));
                lines.push(Line::from(Span::styled(about, pal.dim())));
                if category == Category::System && !self.custom {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "This is the space the disk reports as used minus everything lla found.",
                        pal.dim(),
                    )));
                    lines.push(Line::from(Span::styled(
                        "Snapshots and swap shrink on their own; restarting often gives some back.",
                        pal.dim(),
                    )));
                }
                if category == Category::System {
                    let unreadable = self.scan.as_ref().map_or(0, Scan::unreadable);
                    if unreadable > 0 {
                        lines.push(Line::from(""));
                        lines.push(Line::from(vec![
                            Span::styled("▲ ", pal.fg(AMBER)),
                            Span::styled(
                                format!(
                                    "{} folders could not be read, so their size lands here. To include them, {}.",
                                    format::count(unreadable),
                                    unreadable_hint()
                                ),
                                pal.fg(AMBER),
                            ),
                        ]));
                    }
                    frame.render_widget(
                        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
                        area,
                    );
                    return;
                }
                let mut summary = Vec::new();
                if group.safe > 0 {
                    summary.push(Span::styled(
                        format::size(group.safe),
                        pal.fg(GREEN).add_modifier(Modifier::BOLD),
                    ));
                    summary.push(Span::styled(" safe to clean", pal.dim()));
                }
                let stale_only = group.stale.saturating_sub(group.safe.min(group.stale));
                if group.stale > 0 {
                    if !summary.is_empty() {
                        summary.push(Span::styled("  ·  ", pal.dim()));
                    }
                    summary.push(Span::styled(
                        format::size(group.stale.max(stale_only)),
                        pal.fg(AMBER).add_modifier(Modifier::BOLD),
                    ));
                    summary.push(Span::styled(" untouched for 3+ months", pal.dim()));
                }
                let filter_hint = if self.filter {
                    vec![
                        Span::styled("f ", pal.fg(ACCENT).add_modifier(Modifier::BOLD)),
                        Span::styled("showing cleanable only", pal.fg(ACCENT)),
                    ]
                } else {
                    vec![
                        Span::styled("f ", bold),
                        Span::styled("only cleanable & unused", pal.dim()),
                    ]
                };
                lines.push(self.spread(summary, filter_hint, width));
            }
            Some(browse) => {
                let node_size = self
                    .scan
                    .as_ref()
                    .and_then(|scan| scan.locate(&browse.path))
                    .map_or(0, |node| node.size);
                parent_size = node_size;
                let mut crumbs = vec![Span::styled(
                    category.label().to_string(),
                    pal.fg(color).add_modifier(Modifier::BOLD),
                )];
                let base_name = browse
                    .base
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| browse.base.display().to_string());
                let mut parts = vec![base_name];
                if let Ok(rel) = browse.path.strip_prefix(&browse.base) {
                    parts.extend(rel.iter().map(|part| part.to_string_lossy().into_owned()));
                }
                let crumb_text = parts.join(" › ");
                let room = (width as usize).saturating_sub(category.label().len() + 24);
                crumbs.push(Span::styled(" › ", pal.dim()));
                crumbs.push(Span::styled(format::fit_start(&crumb_text, room), bold));
                lines.push(self.spread(
                    crumbs,
                    vec![
                        Span::styled(format::size(node_size), bold),
                        Span::styled(
                            format!("  {} of disk", format::percent(node_size, disk_total)),
                            pal.dim(),
                        ),
                    ],
                    width,
                ));
                let path_text = format::tilde(&browse.path, home);
                lines.push(Line::from(Span::styled(
                    format::fit_start(&path_text, width as usize),
                    pal.dim(),
                )));
                lines.push(self.spread(
                    vec![
                        Span::styled(browse.safety.label(), pal.fg(safety_rgb(browse.safety))),
                        Span::styled(format!(" · {}", browse.reason), pal.dim()),
                    ],
                    vec![Span::styled("esc ", bold), Span::styled("back", pal.dim())],
                    width,
                ));
            }
        }

        lines.push(Line::from(""));
        let count_label = match (self.rows.len(), self.browse.is_empty()) {
            (1, _) => "1 item".to_string(),
            (n, _) => format!("{} items", format::count(n as u64)),
        };
        let wide = width >= 72;
        let bar_width = if wide { 12 } else { 0 };
        let tail = 9 + 1 + bar_width + if wide { 1 } else { 0 } + 4 + 2 + 6;
        lines.push(self.spread(
            vec![Span::styled(
                format!("  {count_label} · by {}", self.sort.label()),
                pal.dim(),
            )],
            vec![Span::styled(
                format!(
                    "{:>width$}",
                    "size        last change",
                    width = tail.min(width as usize)
                ),
                pal.dim(),
            )],
            width,
        ));

        let header_lines = lines.len();
        let list_height = (area.height as usize).saturating_sub(header_lines);
        let focused = self.focus == Focus::Items || !self.browse.is_empty();
        let detail_lines = if focused { 2 } else { 0 };
        let capacity = list_height.saturating_sub(detail_lines).max(1);
        self.page = capacity;

        let sel = self.selected();
        let offset = {
            let current = self
                .browse
                .last()
                .map_or(self.offset, |browse| browse.offset);
            let mut offset = current.min(sel);
            if sel >= offset + capacity {
                offset = sel + 1 - capacity;
            }
            offset
        };
        match self.browse.last_mut() {
            Some(browse) => browse.offset = offset,
            None => self.offset = offset,
        }

        if self.rows.is_empty() {
            let message = if self.filter {
                "Nothing here is safe to clean or unused. Press f to show everything."
            } else {
                "Empty."
            };
            lines.push(Line::from(Span::styled(format!("  {message}"), pal.dim())));
        }

        let largest = self
            .rows
            .iter()
            .map(|row| row.size)
            .max()
            .unwrap_or(1)
            .max(1);
        let now = report.now;
        for (index, row) in self.rows.iter().enumerate().skip(offset).take(capacity) {
            let is_selected = index == sel;
            let marked = row
                .path
                .as_ref()
                .is_some_and(|path| self.marks.contains_key(path));
            let stale = row.newest > 0 && now - row.newest > model::STALE_AFTER;

            let cursor = if is_selected && focused { "› " } else { "  " };
            let (bullet, bullet_style) = if marked {
                ("◉ ", pal.fg(ACCENT))
            } else if row.dir {
                ("▸ ", pal.fg(shade(color, 0.8)))
            } else {
                ("· ", pal.dim())
            };
            let mut name_style = if row.protected || row.loose {
                pal.dim()
            } else {
                Style::default()
            };
            if is_selected && focused {
                name_style = name_style.add_modifier(Modifier::BOLD);
                if !row.loose {
                    name_style = name_style.fg(pal.color(color));
                }
            }
            if marked {
                name_style = name_style.fg(pal.color(ACCENT));
            }

            let name_room = (width as usize).saturating_sub(4 + tail + 2);
            let name = format::fit(&row.name, name_room);
            let location_room = name_room.saturating_sub(format::width(&name) + 3);
            let location = if row.location.is_empty() || location_room < 6 {
                String::new()
            } else {
                format!("  {}", format::fit_start(&row.location, location_room))
            };

            let mut right = vec![Span::styled(
                format!("{:>9}", format::size(row.size)),
                if is_selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            )];
            if wide {
                let filled = (((row.size as f64 / largest as f64) * bar_width as f64).ceil()
                    as usize)
                    .min(bar_width);
                right.push(Span::raw(" "));
                right.push(Span::styled("•".repeat(filled), pal.fg(shade(color, 0.9))));
                right.push(Span::styled("·".repeat(bar_width - filled), pal.fg(FAINT)));
            }
            right.push(Span::styled(
                format!(" {:>4}", format::percent(row.size, parent_size)),
                pal.dim(),
            ));
            right.push(Span::styled(
                format!("  {:>6}", format::age(now, row.newest)),
                if stale { pal.fg(AMBER) } else { pal.dim() },
            ));

            lines.push(self.spread(
                vec![
                    Span::styled(cursor, pal.fg(ACCENT)),
                    Span::styled(bullet, bullet_style),
                    Span::styled(name, name_style),
                    Span::styled(location, pal.dim()),
                ],
                right,
                width,
            ));

            if is_selected && focused {
                lines.push(self.detail_line(row, now, width));
                lines.push(self.action_line(row));
            }
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn detail_line(&self, row: &Row, now: i64, width: u16) -> Line<'static> {
        let pal = self.pal;
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let mut spans = vec![Span::styled("     └ ", pal.fg(FAINT))];
        if row.protected && !row.loose {
            spans.push(Span::styled("protected", pal.fg(RED)));
            spans.push(Span::styled(
                " · lla will not remove this folder",
                pal.dim(),
            ));
            return Line::from(spans);
        }
        spans.push(Span::styled(
            row.safety.label(),
            pal.fg(safety_rgb(row.safety)),
        ));
        spans.push(Span::styled(format!(" · {}", row.reason), pal.dim()));
        if row.newest > 0 {
            spans.push(Span::styled(
                format!(" · changed {}", format::long_age(now, row.newest)),
                pal.dim(),
            ));
        }
        if row.files > 1 {
            spans.push(Span::styled(
                format!(" · {} files", format::count(row.files)),
                pal.dim(),
            ));
        }
        if let (true, Some(path)) = (row.dir, &row.path) {
            if let Some(node) = self.scan.as_ref().and_then(|scan| scan.locate(path)) {
                if let Some((rel, size)) = culprit(node) {
                    let used: usize = spans.iter().map(|span| format::width(&span.content)).sum();
                    let room = (width as usize).saturating_sub(used + 30);
                    if room > 8 {
                        spans.push(Span::styled(" · mostly in ", pal.dim()));
                        let suffix = if node_is_dir(node, &rel) { "/" } else { "" };
                        spans.push(Span::styled(
                            format::fit_start(&format!("{}{suffix}", rel.display()), room),
                            bold,
                        ));
                        spans.push(Span::styled(
                            format!(
                                " {} · {}",
                                format::size(size),
                                format::percent(size, node.size)
                            ),
                            pal.dim(),
                        ));
                    }
                }
            }
        }
        Line::from(spans)
    }

    fn action_line(&self, row: &Row) -> Line<'static> {
        let pal = self.pal;
        let key =
            |text: &'static str| Span::styled(text, Style::default().add_modifier(Modifier::BOLD));
        let label = |text: &'static str| Span::styled(text, pal.dim());
        let mut spans = vec![Span::raw("       ")];
        if row.dir && row.path.is_some() {
            spans.extend([
                key("⏎ "),
                label("open   "),
                key("g "),
                label("biggest part   "),
            ]);
        }
        if row.path.is_some() {
            spans.extend([key("o "), label("reveal   ")]);
        }
        if row.markable() {
            spans.extend([key("␣ "), label("mark   ")]);
            if row.permanent {
                spans.push(Span::styled("d ", pal.fg(RED).add_modifier(Modifier::BOLD)));
                spans.push(label("delete for good"));
            } else {
                spans.push(Span::styled("d ", pal.fg(RED).add_modifier(Modifier::BOLD)));
                spans.push(label("move to trash"));
            }
        }
        Line::from(spans)
    }

    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let pal = self.pal;
        let bold = Style::default().add_modifier(Modifier::BOLD);
        if let Some(toast) = &self.toast {
            let color = match toast.tone {
                Tone::Good => GREEN,
                Tone::Warn => AMBER,
                Tone::Info => ACCENT,
            };
            let line = Line::from(vec![
                Span::styled("● ", pal.fg(color)),
                Span::styled(
                    format::fit(&toast.text, area.width.saturating_sub(2) as usize),
                    pal.fg(color),
                ),
            ]);
            frame.render_widget(Paragraph::new(line), area);
            return;
        }
        let hints: &[(&str, &str)] = if self.report.is_none() {
            &[("q", "cancel")]
        } else if self.focus == Focus::Categories && self.browse.is_empty() {
            &[
                ("↑↓", "category"),
                ("⏎", "browse"),
                ("tab", "next"),
                ("f", "filter"),
                ("r", "rescan"),
                ("?", "help"),
                ("q", "quit"),
            ]
        } else {
            &[
                ("↑↓", "move"),
                ("⏎", "open"),
                ("esc", "back"),
                ("␣", "mark"),
                ("a", "mark cleanable"),
                ("d", "trash"),
                ("s", "sort"),
                ("?", "help"),
            ]
        };
        let mut left = Vec::new();
        for (key, label) in hints {
            left.push(Span::styled(*key, bold));
            left.push(Span::styled(format!(" {label}   "), pal.dim()));
        }
        let mut right = Vec::new();
        if !self.marks.is_empty() {
            let targets = self.marked_targets();
            let size: u64 = targets.iter().map(|target| target.size).sum();
            right.extend([
                Span::styled("◉ ", pal.fg(ACCENT)),
                Span::styled(
                    format!("{} marked · {}", targets.len(), format::size(size)),
                    pal.fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  d", bold),
                Span::styled(" clean", pal.dim()),
            ]);
        }
        // Drop hints that do not fit rather than wrapping.
        let right_width: usize = right.iter().map(|span| format::width(&span.content)).sum();
        let mut used = 0;
        let mut kept = Vec::new();
        for pair in left.chunks(2) {
            let width: usize = pair.iter().map(|span| format::width(&span.content)).sum();
            if used + width + right_width + 2 > area.width as usize {
                break;
            }
            used += width;
            kept.extend(pair.iter().cloned());
        }
        frame.render_widget(Paragraph::new(self.spread(kept, right, area.width)), area);
    }

    fn draw_confirm(&self, frame: &mut Frame, area: Rect, method: Method, targets: &[Target]) {
        let pal = self.pal;
        let home = self.rules.home.as_deref();
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let permanent = method == Method::Delete || targets.iter().all(|target| target.permanent);
        let some_permanent = targets.iter().any(|target| target.permanent);
        let total: u64 = targets.iter().map(|target| target.size).sum();
        let shown = targets.len().min(8);
        let width = area.width.saturating_sub(4).min(76);
        let height = (shown as u16 + 9).min(area.height.saturating_sub(2));
        let rect = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        let title = if permanent {
            format!(" Delete {} for good? ", plural(targets.len(), "item"))
        } else {
            let place = if cfg!(target_os = "macos") {
                "the Trash"
            } else {
                "the trash"
            };
            format!(" Move {} to {place}? ", plural(targets.len(), "item"))
        };
        let border = if permanent { RED } else { ACCENT };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(pal.fg(border))
            .title(Span::styled(title, bold));
        let inner_width = width.saturating_sub(4) as usize;
        let mut lines = vec![Line::from("")];
        for target in targets.iter().take(shown) {
            let path = format::tilde(&target.path, home);
            lines.push(Line::from(vec![
                Span::styled(format!(" {:>9}  ", format::size(target.size)), bold),
                Span::raw(format::fit_start(&path, inner_width.saturating_sub(12))),
            ]));
        }
        if targets.len() > shown {
            lines.push(Line::from(Span::styled(
                format!("            and {} more", targets.len() - shown),
                pal.dim(),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {:>9}  ", format::size(total)),
                pal.fg(border).add_modifier(Modifier::BOLD),
            ),
            Span::styled("in total", pal.dim()),
        ]));
        let note = if permanent {
            Span::styled(" This cannot be undone.", pal.fg(RED))
        } else if some_permanent {
            Span::styled(
                " Items already in the trash are deleted for good; the rest can be restored.",
                pal.fg(AMBER),
            )
        } else {
            Span::styled(
                " You can put them back from the trash until you empty it.",
                pal.dim(),
            )
        };
        lines.push(Line::from(note));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(" y ", pal.fg(border).add_modifier(Modifier::BOLD)),
            Span::styled(
                if permanent {
                    "delete    "
                } else {
                    "move to trash    "
                },
                pal.dim(),
            ),
            Span::styled("n ", bold),
            Span::styled("cancel", pal.dim()),
        ]));
        frame.render_widget(Clear, rect);
        frame.render_widget(Paragraph::new(lines).block(block), rect);
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let pal = self.pal;
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let keys: &[(&str, &str)] = &[
            ("↑ ↓  j k", "move"),
            ("⏎  →  l", "open a category or folder"),
            ("esc  ←  h", "go back"),
            ("tab", "next category"),
            ("space", "mark or unmark"),
            ("a", "mark everything cleanable here"),
            ("u", "clear marks"),
            ("d", "move marked items to the trash"),
            ("D", "delete marked items for good"),
            ("g", "jump to the biggest part of a folder"),
            ("o", "reveal in the file manager"),
            ("f", "show only cleanable and unused items"),
            ("s", "sort by size, age or name"),
            ("r", "scan again"),
            ("q", "quit"),
        ];
        let width = area.width.saturating_sub(4).min(64);
        let height = (keys.len() as u16 + 9).min(area.height.saturating_sub(2));
        let rect = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };
        let mut lines = vec![Line::from("")];
        for (key, label) in keys {
            lines.push(Line::from(vec![
                Span::styled(format!("  {key:<12}"), bold),
                Span::styled(*label, pal.dim()),
            ]));
        }
        lines.push(Line::from(""));
        for safety in [Safety::Safe, Safety::Review, Safety::Keep] {
            let about = match safety {
                Safety::Safe => "rebuilt or downloaded again on its own",
                Safety::Review => "probably fine to remove, take a look first",
                Safety::Keep => "personal files, never suggested",
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<14}", safety.label()),
                    pal.fg(safety_rgb(safety)),
                ),
                Span::styled(about, pal.dim()),
            ]));
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(pal.fg(ACCENT))
            .title(Span::styled(" keys ", bold));
        frame.render_widget(Clear, rect);
        frame.render_widget(Paragraph::new(lines).block(block), rect);
    }
}

fn node_is_dir(node: &Node, rel: &Path) -> bool {
    node.find(rel).is_some_and(|found| found.dir)
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("1 {word}")
    } else {
        format!("{count} {word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::fs;

    fn screen(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn press(app: &mut App, code: KeyCode) {
        app.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn fixture_app() -> (tempfile::TempDir, App) {
        let root = tempfile::TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let app_dir = path.join("site");
        fs::create_dir_all(app_dir.join("node_modules/react/dist")).unwrap();
        fs::write(app_dir.join("package.json"), "{}").unwrap();
        fs::write(
            app_dir.join("node_modules/react/dist/react.js"),
            vec![0u8; 3 << 20],
        )
        .unwrap();
        fs::write(path.join("movie.mp4"), vec![0u8; 2 << 20]).unwrap();
        let rules = Rules::new(None, Vec::new());
        let job = Job::start(vec![path.clone()], true);
        let scan = loop {
            if let Some(scan) = job.try_finish() {
                break scan;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut app = App::new(
            rules,
            vec![path],
            true,
            None,
            Palette {
                truecolor: true,
                mono: false,
            },
        );
        app.finish(scan);
        (root, app)
    }

    #[test]
    fn every_screen_draws_at_common_sizes() {
        let (_root, mut app) = fixture_app();
        for (width, height) in [(150, 44), (100, 30), (80, 24), (60, 16), (30, 8)] {
            screen(&mut app, width, height);
        }
        let text = screen(&mut app, 120, 36);
        assert!(text.contains("Developer"), "{text}");
        assert!(
            text.contains("Rest of the disk") || app.volume.is_none(),
            "{text}"
        );

        // Into the developer list, open node_modules, and back out.
        let developer = app
            .order
            .iter()
            .position(|category| *category == Category::Developer)
            .unwrap();
        app.cat = developer;
        app.refresh_rows();
        press(&mut app, KeyCode::Enter);
        let text = screen(&mut app, 120, 36);
        assert!(text.contains("node_modules/"), "{text}");
        assert!(
            text.contains("reinstalled by your package manager"),
            "{text}"
        );
        press(&mut app, KeyCode::Char('g'));
        let text = screen(&mut app, 120, 36);
        assert!(text.contains("react.js"), "{text}");
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Esc);
        assert!(app.browse.is_empty());

        // Marking opens a confirmation that names the folder.
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('D'));
        let text = screen(&mut app, 120, 36);
        assert!(text.contains("for good"), "{text}");
        assert!(text.contains("node_modules"), "{text}");
        press(&mut app, KeyCode::Char('n'));
        assert!(app.modal.is_none());

        press(&mut app, KeyCode::Char('?'));
        let text = screen(&mut app, 120, 36);
        assert!(text.contains("keys"), "{text}");
        if std::env::var_os("LLA_SCAN_SHOW").is_some() {
            press(&mut app, KeyCode::Esc);
            println!("{}", screen(&mut app, 150, 44));
        }
    }

    /// `LLA_SCAN_PATH=~ cargo test real_scan_preview -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_scan_preview() {
        let path = PathBuf::from(std::env::var("LLA_SCAN_PATH").unwrap());
        let rules = Rules::detect();
        let mut targets = vec![path.clone()];
        let custom = rules.home.as_deref() != Some(path.as_path());
        if !custom {
            targets.extend(rules.system_roots());
        }
        let job = Job::start(targets.clone(), custom);
        let scan = loop {
            if let Some(scan) = job.try_finish() {
                break scan;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let mut app = App::new(
            rules,
            targets,
            custom,
            None,
            Palette {
                truecolor: true,
                mono: false,
            },
        );
        app.finish(scan);
        app.tick = 100;
        println!("{}", screen(&mut app, 150, 44));
        app.cat = 1;
        app.refresh_rows();
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        println!("{}", screen(&mut app, 150, 44));
        press(&mut app, KeyCode::Char('g'));
        println!("{}", screen(&mut app, 150, 44));
    }

    #[test]
    fn deleting_updates_the_tree_and_totals() {
        let (root, mut app) = fixture_app();
        let target = root
            .path()
            .canonicalize()
            .unwrap()
            .join("site/node_modules");
        let before = app.scan.as_ref().unwrap().scanned_bytes();
        app.clean(
            Method::Delete,
            vec![Target {
                path: target.clone(),
                size: 3 << 20,
                permanent: false,
            }],
        );
        assert!(!target.exists());
        let after = app.scan.as_ref().unwrap().scanned_bytes();
        assert!(after + (3 << 20) <= before, "{before} -> {after}");
        let report = app.report.as_ref().unwrap();
        assert!(report.group(Category::Developer).items.is_empty());
    }

    #[test]
    fn rgb_falls_back_to_256_colours() {
        assert_eq!(ansi256((0, 0, 0)), 16);
        assert_eq!(ansi256((255, 255, 255)), 231);
        assert_eq!(ansi256((255, 0, 0)), 196);
        assert!((232..=255).contains(&ansi256((128, 128, 128))));
    }
}
