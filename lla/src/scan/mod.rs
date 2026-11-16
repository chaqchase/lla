//! `lla scan`: find out what is filling the disk and clean it up.

mod clean;
mod disk;
mod format;
mod model;
mod rules;
mod tui;
mod walk;

use crate::error::{LlaError, Result};
use colored::{Color, Colorize};
use model::{Report, Scan};
use parking_lot::Mutex;
use rules::{Category, Rules, Safety};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use walk::Progress;

#[derive(Clone, Debug, Default)]
pub struct ScanOptions {
    pub path: Option<String>,
    pub json: bool,
    pub report: bool,
    pub top: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootPhase {
    Waiting,
    Scanning,
    Done(u64),
}

/// A scan running on a background thread.
pub(crate) struct Job {
    pub progress: Arc<Progress>,
    pub roots: Arc<Mutex<Vec<(PathBuf, RootPhase)>>>,
    pub started: Instant,
    rx: crossbeam_channel::Receiver<Scan>,
}

impl Job {
    pub fn start(targets: Vec<PathBuf>, custom: bool) -> Self {
        let progress = Arc::new(Progress::default());
        let roots = Arc::new(Mutex::new(
            targets
                .iter()
                .map(|path| (path.clone(), RootPhase::Waiting))
                .collect::<Vec<_>>(),
        ));
        let (tx, rx) = crossbeam_channel::bounded(1);
        let started = Instant::now();
        {
            let progress = Arc::clone(&progress);
            let roots = Arc::clone(&roots);
            std::thread::spawn(move || {
                let mut scanned = Vec::new();
                for (index, path) in targets.iter().enumerate() {
                    roots.lock()[index].1 = RootPhase::Scanning;
                    let node = walk::scan(path, &progress);
                    roots.lock()[index].1 = RootPhase::Done(node.size);
                    scanned.push((path.clone(), node));
                }
                let volume = targets.first().and_then(|path| disk::volume(path));
                let _ = tx.send(Scan {
                    roots: scanned,
                    volume,
                    elapsed: started.elapsed(),
                    custom,
                });
            });
        }
        Self {
            progress,
            roots,
            started,
            rx,
        }
    }

    pub fn try_finish(&self) -> Option<Scan> {
        self.rx.try_recv().ok()
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }
}

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

pub(crate) fn unreadable_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "grant your terminal Full Disk Access"
    } else if cfg!(windows) {
        "run as administrator"
    } else {
        "run with sudo"
    }
}

pub fn run(options: ScanOptions) -> Result<()> {
    let rules = Rules::detect();
    let (targets, custom) = match &options.path {
        Some(path) => {
            let path = Path::new(path);
            let resolved = path
                .canonicalize()
                .map_err(|error| LlaError::Other(format!("{}: {error}", path.display())))?;
            (vec![simplify(resolved)], true)
        }
        None => {
            let home = rules.home.clone().ok_or_else(|| {
                LlaError::Other("could not find your home folder; pass a path to scan".into())
            })?;
            let mut targets = vec![home];
            targets.extend(rules.system_roots());
            (targets, false)
        }
    };

    let interactive = !options.json
        && !options.report
        && std::io::stdout().is_terminal()
        && std::io::stdin().is_terminal();
    if interactive {
        return tui::run(rules, targets, custom);
    }

    let job = Job::start(targets, custom);
    let scan = wait_with_spinner(&job);
    let report = model::build(&scan, &rules, now());
    let top = if options.top == 0 { 10 } else { options.top };
    if options.json {
        print_json(&scan, &report, top)
    } else {
        print_report(&scan, &report, &rules, top);
        Ok(())
    }
}

/// `canonicalize` on Windows returns `\\?\C:\...`, which reads badly and trips up some tools.
fn simplify(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            if rest.as_bytes().get(1) == Some(&b':') {
                return PathBuf::from(rest);
            }
        }
    }
    path
}

fn wait_with_spinner(job: &Job) -> Scan {
    let spinner = std::io::stderr().is_terminal().then(|| {
        let bar = indicatif::ProgressBar::new_spinner();
        bar.set_draw_target(indicatif::ProgressDrawTarget::stderr());
        bar.enable_steady_tick(Duration::from_millis(80));
        bar
    });
    loop {
        if let Some(scan) = job.try_finish() {
            if let Some(bar) = spinner {
                bar.finish_and_clear();
            }
            return scan;
        }
        if let Some(bar) = &spinner {
            let files = job
                .progress
                .files
                .load(std::sync::atomic::Ordering::Relaxed);
            let bytes = job
                .progress
                .bytes
                .load(std::sync::atomic::Ordering::Relaxed);
            bar.set_message(format!(
                "Scanning · {} files · {}",
                format::count(files),
                format::size(bytes)
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn category_color(category: Category) -> Color {
    match category {
        Category::Apps => Color::Blue,
        Category::Developer => Color::Green,
        Category::Caches => Color::Yellow,
        Category::Logs => Color::BrightBlack,
        Category::Downloads => Color::Cyan,
        Category::Documents => Color::BrightCyan,
        Category::Photos => Color::Magenta,
        Category::Videos => Color::BrightMagenta,
        Category::Music => Color::BrightYellow,
        Category::Trash => Color::Red,
        Category::Other => Color::White,
        Category::System => Color::BrightBlack,
    }
}

fn scope_label(scan: &Scan, home: Option<&Path>) -> String {
    scan.roots
        .iter()
        .map(|(path, _)| format::tilde(path, home))
        .collect::<Vec<_>>()
        .join(", ")
}

fn print_report(scan: &Scan, report: &Report, rules: &Rules, top: usize) {
    let home = rules.home.as_deref();
    let mut out = String::new();
    let mut title = format!(
        "{}  {}",
        "lla scan".bold(),
        scope_label(scan, home).dimmed()
    );
    if let Some(volume) = &scan.volume {
        if !volume.label.is_empty() {
            title.push_str(&format!("{}", format!("  ·  {}", volume.label).dimmed()));
        }
    }
    out.push_str(&title);
    out.push('\n');

    let total = scan
        .volume
        .as_ref()
        .map_or(scan.scanned_bytes(), |volume| volume.total);
    if let Some(volume) = &scan.volume {
        let line = [
            format::size(volume.used()).bold().to_string(),
            format!("used of {}", format::size(volume.total))
                .dimmed()
                .to_string(),
            "·".dimmed().to_string(),
            format::size(volume.free).bold().to_string(),
            "free".dimmed().to_string(),
            "·".dimmed().to_string(),
            format::percent(volume.used(), volume.total)
                .bold()
                .to_string(),
            "full".dimmed().to_string(),
        ]
        .join(" ");
        out.push_str(&line);
        out.push('\n');
    }
    out.push('\n');

    // One bar for the whole disk, split by category.
    let order = report.ordered();
    let cells = 64usize;
    let mut bar = String::new();
    let mut filled = 0usize;
    let mut running = 0u64;
    for category in &order {
        running += report.group(*category).size;
        let end = if total == 0 {
            0
        } else {
            ((running as f64 / total as f64) * cells as f64).round() as usize
        }
        .min(cells);
        if end > filled {
            bar.push_str(
                &"█"
                    .repeat(end - filled)
                    .color(category_color(*category))
                    .to_string(),
            );
            filled = end;
        }
    }
    bar.push_str(&"·".repeat(cells - filled).dimmed().to_string());
    out.push_str(&format!("{bar}\n\n"));

    for category in &order {
        let group = report.group(*category);
        let mut line = format!(
            "  {} {:<16} {:>9}  {:>4}",
            "■".color(category_color(*category)),
            scan.label(*category),
            format::size(group.size),
            format::percent(group.size, total),
        );
        if group.safe > 0 {
            line.push_str(&format!(
                "   {} {}",
                format::size(group.safe).green(),
                "safe to clean".dimmed()
            ));
        }
        out.push_str(&line);
        out.push('\n');
    }

    let mut safe: Vec<(&rules::Category, &model::Item)> = Vec::new();
    let mut stale: Vec<(&rules::Category, &model::Item)> = Vec::new();
    for category in &Category::ALL {
        for item in &report.group(*category).items {
            if item.loose || item.protected {
                continue;
            }
            if item.safety == Safety::Safe {
                safe.push((category, item));
            } else if item.safety == Safety::Review && item.stale(report.now) {
                stale.push((category, item));
            }
        }
    }
    for (title, list, color) in [
        ("Safe to clean", &mut safe, Color::Green),
        ("Untouched for 3+ months", &mut stale, Color::Yellow),
    ] {
        if list.is_empty() {
            continue;
        }
        list.sort_by_key(|(_, item)| std::cmp::Reverse(item.size));
        let sum: u64 = list.iter().map(|(_, item)| item.size).sum();
        out.push_str(&format!(
            "\n{}  {}\n",
            title.bold(),
            format::size(sum).color(color)
        ));
        for (category, item) in list.iter().take(top) {
            out.push_str(&format!(
                "  {:>9}  {}  {}\n",
                format::size(item.size),
                format::tilde(&item.path, home),
                format!("{} · {}", scan.label(**category), item.reason).dimmed()
            ));
        }
    }

    out.push('\n');
    let unreadable = scan.unreadable();
    if unreadable > 0 {
        out.push_str(&format!(
            "{}\n",
            format!(
                "{} folders could not be read; {} to include them.",
                format::count(unreadable),
                unreadable_hint()
            )
            .yellow()
        ));
    }
    out.push_str(&format!(
        "{}\n",
        format!(
            "Scanned {} files ({}) in {:.1}s. Run lla scan in a terminal to clean up interactively.",
            format::count(scan.files()),
            format::size(scan.scanned_bytes()),
            scan.elapsed.as_secs_f64()
        )
        .dimmed()
    ));
    print!("{out}");
}

fn print_json(scan: &Scan, report: &Report, top: usize) -> Result<()> {
    use serde_json::json;
    let categories: Vec<_> = report
        .ordered()
        .into_iter()
        .map(|category| {
            let group = report.group(category);
            let items: Vec<_> = group
                .items
                .iter()
                .take(top)
                .map(|item| {
                    json!({
                        "path": item.path,
                        "bytes": item.size,
                        "files": item.files,
                        "modified": item.newest,
                        "safety": item.safety.key(),
                        "reason": item.reason,
                        "stale": item.stale(report.now),
                        "protected": item.protected,
                        "loose_files": item.loose,
                    })
                })
                .collect();
            json!({
                "key": category.key(),
                "label": scan.label(category),
                "bytes": group.size,
                "safe_bytes": group.safe,
                "stale_bytes": group.stale,
                "item_count": group.items.len(),
                "items": items,
            })
        })
        .collect();
    let value = json!({
        "volume": scan.volume.as_ref().map(|volume| json!({
            "label": volume.label,
            "total": volume.total,
            "used": volume.used(),
            "free": volume.free,
        })),
        "roots": scan.roots.iter().map(|(path, _)| path).collect::<Vec<_>>(),
        "scanned_bytes": scan.scanned_bytes(),
        "files": scan.files(),
        "unreadable_folders": scan.unreadable(),
        "elapsed_ms": scan.elapsed.as_millis() as u64,
        "safe_bytes": report.safe_total(),
        "categories": categories,
    });
    let text =
        serde_json::to_string_pretty(&value).map_err(|error| LlaError::Other(error.to_string()))?;
    println!("{text}");
    Ok(())
}
