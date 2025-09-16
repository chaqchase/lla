use crate::commands::args::{Args, OutputMode};
use crate::config::Config;
use crate::error::{LlaError, Result};
use crate::theme::is_no_color;
use crate::utils::color::colorize_file_name;
use colored::*;
use ignore::WalkBuilder;
use lla_plugin_utils::syntax::CodeHighlighter;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use unicode_width::UnicodeWidthChar;

fn build_exclude_args(config: &Config, root: &Path) -> Vec<String> {
    let mut args = Vec::new();
    for ex in &config.exclude_paths {
        // Convert absolute excludes to relative to root if possible; ripgrep --glob works with patterns
        // We prefer --glob '!path/**' to ensure prefix exclusion.
        let pattern = if let Ok(rel) = ex.strip_prefix(root) {
            format!("!{}/**", rel.to_string_lossy())
        } else {
            // If not under root, use absolute-like pattern which ripgrep will still try to match
            format!("!{}/**", ex.to_string_lossy())
        };
        args.push("--glob".to_string());
        args.push(pattern);
    }
    args
}

fn build_name_filter_args(args_cfg: &Args) -> Vec<String> {
    // Reuse existing name/path filter string as ripgrep file filtering via --glob when possible.
    // We only map simple extension (.ext) and glob: patterns to ripgrep globs.
    let mut args = Vec::new();
    if let Some(filter_str) = &args_cfg.filter {
        if filter_str.starts_with("glob:") {
            let g = &filter_str[5..];
            args.push("--glob".to_string());
            args.push(g.to_string());
        } else if filter_str.starts_with('.') {
            let ext = &filter_str[1..];
            args.push("--glob".to_string());
            args.push(format!("**/*.{}", ext));
        }
    }
    args
}

pub fn run_search(args: &Args, config: &Config) -> Result<()> {
    // Record visit of the search root for jump history
    crate::commands::jump::record_visit(&args.directory, config);
    let pattern = match &args.search {
        Some(p) => p,
        None => return Err(LlaError::Other("--search requires a pattern".into())),
    };

    // Validate directory and honor dotfile filtering using the ignore crate (same semantics as listing)
    let root = Path::new(&args.directory);
    if !root.exists() {
        return Err(LlaError::Other(format!(
            "Path not found: {}",
            root.display()
        )));
    }

    // Build ripgrep command
    let mut cmd = Command::new("rg");
    cmd.current_dir(root);
    cmd.arg("--line-number");
    cmd.arg("--column");
    cmd.arg("--color");
    cmd.arg("never");
    cmd.arg("--context");
    cmd.arg(args.search_context.to_string());

    // Use fixed-string mode by default for literal matching (safer for user input)
    // Users can override with regex: prefix if they want regex behavior
    let (search_pattern, use_fixed_string) = if pattern.starts_with("regex:") {
        (&pattern[6..], false)
    } else {
        (pattern.as_str(), true)
    };

    if use_fixed_string {
        cmd.arg("--fixed-strings");
    }

    // Case sensitivity
    if !args.case_sensitive {
        cmd.arg("-i");
    }

    // Dotfiles handling
    if args.no_dotfiles && !args.almost_all && !args.dotfiles_only {
        cmd.arg("--hidden"); // allow ripgrep to see hidden
                             // but exclude dotfiles with glob
        cmd.arg("--glob");
        cmd.arg("!**/.*");
    } else {
        cmd.arg("--hidden");
    }

    // File type inclusions/exclusions based on flags
    if args.dirs_only {
        // ripgrep searches files; if dirs_only, nothing to search
        println!("No results (dirs-only with content search)");
        return Ok(());
    }
    if args.no_files {
        println!("No results (--no-files with content search)");
        return Ok(());
    }

    // Respect exclude_paths
    for g in build_exclude_args(config, root) {
        cmd.arg(g);
    }

    // Map simple name filters to ripgrep globs
    for g in build_name_filter_args(args) {
        cmd.arg(g);
    }

    // Always request ripgrep JSON; we'll pretty-print in human mode
    let _machine_output = matches!(
        args.output_mode,
        OutputMode::Json { .. } | OutputMode::Ndjson | OutputMode::Csv
    );
    cmd.arg("--json");

    cmd.arg(search_pattern);

    // Scope search paths: walk honoring .gitignore and config.filter.no_dotfiles with ignore crate
    // Collect eligible paths to pass to ripgrep to avoid traversing excluded prefixes
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut walker = WalkBuilder::new(root);
    walker.hidden(!args.almost_all && (args.no_dotfiles || config.filter.no_dotfiles));
    walker.git_ignore(true).git_exclude(true).parents(true);
    let walker = walker.build();
    for dent in walker {
        if let Ok(d) = dent {
            let p = d.path();
            // Skip directories under excluded prefixes
            if !config.exclude_paths.is_empty() {
                let abs = p;
                if config.exclude_paths.iter().any(|ex| abs.starts_with(ex)) {
                    continue;
                }
            }
            if p.is_file() {
                if args.files_only || (!args.no_files) {
                    paths.push(p.to_path_buf());
                }
            }
        }
    }

    if paths.is_empty() {
        // Still run rg on root to allow it to handle includes if any
        cmd.arg(".");
    } else {
        for p in paths {
            cmd.arg(p);
        }
    }

    let output = cmd
        .output()
        .map_err(|e| LlaError::Other(format!("Failed to run ripgrep: {}", e)))?;

    if !output.status.success() && output.stdout.is_empty() {
        // Show stderr if rg failed without output (e.g., binary not found)
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(LlaError::Other(format!("ripgrep error: {}", err.trim())));
    }

    match args.output_mode {
        OutputMode::Human => render_pretty(&output.stdout, args),
        OutputMode::Json { .. } | OutputMode::Ndjson => {
            // Pass-through ripgrep JSON
            let s = String::from_utf8_lossy(&output.stdout);
            print!("{}", s);
            Ok(())
        }
        OutputMode::Csv => render_csv(&output.stdout),
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum RipgrepEvent {
    Match {
        data: RgMatch,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize, Clone)]
struct RgMatch {
    path: RgText,
    lines: RgLines,
    line_number: usize,
    submatches: Vec<RgSubMatch>,
}

#[derive(Debug, Deserialize, Clone)]
struct RgText {
    text: String,
}

#[derive(Debug, Deserialize, Clone)]
struct RgLines {
    text: String,
}

#[derive(Debug, Deserialize, Clone)]
struct RgSubMatch {
    #[serde(rename = "match")]
    m: RgText,
    start: usize,
    end: usize,
}

fn render_pretty(stdout: &[u8], args: &Args) -> Result<()> {
    use std::collections::BTreeMap;
    let mut matches_by_file: BTreeMap<String, Vec<RgMatch>> = BTreeMap::new();

    for line in String::from_utf8_lossy(stdout).lines() {
        if let Ok(ev) = serde_json::from_str::<RipgrepEvent>(line) {
            if let RipgrepEvent::Match { data } = ev {
                matches_by_file
                    .entry(data.path.text.clone())
                    .or_default()
                    .push(data);
            }
        }
    }

    const TABSTOP: usize = 4;

    for (file, list) in matches_by_file.iter_mut() {
        list.sort_by_key(|m| m.line_number);
        let path = std::path::Path::new(file);
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let full_path = path.display();

        if is_no_color() {
            println!("\nFile: {} [{}]", file_name, full_path);
        } else {
            println!(
                "\n{} {} {}",
                "File:".bright_black().bold(),
                colorize_file_name(path).bold(),
                format!("[{}]", full_path).bright_black()
            );
        }

        for m in list.iter() {
            // Compute snippet region centered around the match line
            let start_line = m.line_number.saturating_sub(args.search_context);
            let total_lines = args.search_context * 2 + 1;
            let snippet = read_snippet(path, start_line, total_lines)?;
            if snippet.is_empty() {
                continue;
            }

            // Expand tabs in snippet for consistent alignment with markers
            let expanded: Vec<String> = snippet.iter().map(|s| expand_tabs(s, TABSTOP)).collect();

            // Highlight and print each line with our own prefix so we can inject markers right after the target line
            let language = path.extension().and_then(|e| e.to_str()).unwrap_or("txt");
            for (idx, line) in expanded.iter().enumerate() {
                let ln = start_line + idx;
                let prefix = format!("{:4} │ ", ln);
                let mut to_highlight = line.clone();
                to_highlight.push('\n');
                let highlighted = CodeHighlighter::highlight(&to_highlight, language);
                print!("{}{}", prefix, highlighted);

                if ln == m.line_number {
                    // Underline match on this exact line using display columns
                    let center_text = expand_tabs(&m.lines.text.replace('\n', ""), TABSTOP);
                    if !center_text.is_empty() && !m.submatches.is_empty() {
                        let width = display_col_at(&center_text, center_text.len(), TABSTOP);
                        let mut markers = vec![' '; width.max(1)];
                        for sm in &m.submatches {
                            let s = display_col_at(&center_text, sm.start, TABSTOP);
                            let e = display_col_at(&center_text, sm.end, TABSTOP);
                            let end = e.min(markers.len());
                            let start = s.min(end);
                            for i in start..end {
                                markers[i] = '^';
                            }
                        }
                        let marker_line: String = markers.into_iter().collect();
                        if is_no_color() {
                            println!("{}{}", prefix, marker_line);
                        } else {
                            let colored: String = marker_line
                                .chars()
                                .map(|c| {
                                    if c == '^' {
                                        "^".bright_yellow().bold().to_string()
                                    } else {
                                        " ".to_string()
                                    }
                                })
                                .collect();
                            println!("{}{}", prefix, colored);
                        }
                    }
                }
            }
            println!();
        }
    }

    Ok(())
}

fn render_csv(stdout: &[u8]) -> Result<()> {
    println!("file,line,column,kind,text");
    for line in String::from_utf8_lossy(stdout).lines() {
        if let Ok(ev) = serde_json::from_str::<RipgrepEvent>(line) {
            if let RipgrepEvent::Match { data } = ev {
                let file = data.path.text;
                let line_no = data.line_number;
                let col = data.submatches.get(0).map(|sm| sm.start + 1).unwrap_or(1);
                let text = data.lines.text.replace('\n', "");
                let text_escaped = text.replace('"', "\"\"");
                println!(
                    "{} ,{} ,{} ,match ,\"{}\"",
                    file, line_no, col, text_escaped
                );
            }
        }
    }
    Ok(())
}

fn read_snippet(path: &std::path::Path, start_line: usize, lines: usize) -> Result<Vec<String>> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    let file = File::open(path)
        .map_err(|e| LlaError::Other(format!("Failed to read {}: {}", path.display(), e)))?;
    let reader = BufReader::new(file);
    let mut result = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let ln = idx + 1;
        if ln < start_line {
            continue;
        }
        if ln >= start_line + lines {
            break;
        }
        result.push(line.unwrap_or_default());
    }
    Ok(result)
}

fn utf8_col(s: &str, byte_offset: usize) -> usize {
    let mut count = 0;
    for (i, _) in s.char_indices() {
        if i >= byte_offset {
            break;
        }
        count += 1;
    }
    count
}

fn expand_tabs(s: &str, tabstop: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = tabstop - (col % tabstop);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            out.push(ch);
            col += UnicodeWidthChar::width(ch).unwrap_or(1);
        }
    }
    out
}

fn display_col_at(s: &str, byte_offset: usize, tabstop: usize) -> usize {
    let mut col = 0usize;
    for (i, ch) in s.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\t' {
            let spaces = tabstop - (col % tabstop);
            col += spaces;
        } else {
            col += UnicodeWidthChar::width(ch).unwrap_or(1);
        }
    }
    col
}
