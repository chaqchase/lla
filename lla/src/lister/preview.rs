//! File previews for the fuzzy finder: highlighted source with line numbers,
//! tables for CSV, listings for archives, image sizes and hex dumps, all in
//! the colors of the current theme.

use crate::theme::palette::{self, Tone};
use crate::utils::color::{colorize_date_relative, colorize_size};
use colored::{ColoredString, Colorize};
use crossbeam_channel::{Receiver, Sender};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SynColor, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{SyntaxDefinition, SyntaxReference, SyntaxSet};

const MAX_BYTES: u64 = 64 * 1024;
const BINARY_SNIFF_BYTES: usize = 8 * 1024;
/// Lines are cut before highlighting; nothing past this fits on screen anyway,
/// and minified files would otherwise take a long time.
const MAX_LINE_CHARS: usize = 400;
const CACHE_SIZE: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub path: PathBuf,
    pub height: usize,
    pub width: usize,
}

pub struct Preview {
    pub request: Request,
    pub lines: Vec<String>,
}

/// Builds previews on their own thread so moving through results never waits
/// for a file to be read or highlighted. Only the newest request is served.
pub fn spawn_worker(requests: Receiver<Request>, previews: Sender<Preview>) {
    std::thread::spawn(move || {
        let mut cache: VecDeque<Preview> = VecDeque::with_capacity(CACHE_SIZE);
        while let Ok(mut request) = requests.recv() {
            while let Ok(newer) = requests.try_recv() {
                request = newer;
            }
            let lines = match cache.iter().position(|cached| cached.request == request) {
                Some(index) => cache[index].lines.clone(),
                None => {
                    let lines = build(&request.path, request.height, request.width);
                    if cache.len() == CACHE_SIZE {
                        cache.pop_front();
                    }
                    cache.push_back(Preview {
                        request: request.clone(),
                        lines: lines.clone(),
                    });
                    lines
                }
            };
            if previews.send(Preview { request, lines }).is_err() {
                return;
            }
        }
    });
}

fn paint(text: &str, tone: Tone) -> ColoredString {
    text.color(tone.colored())
}

fn muted(text: &str) -> String {
    paint(text, palette::current().muted).to_string()
}

fn faint(text: &str) -> String {
    paint(text, palette::current().faint).to_string()
}

/// The lines shown for `path`: a header, a rule, then the content.
pub fn build(path: &Path, height: usize, width: usize) -> Vec<String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(e) => {
            return vec![
                name.bold().to_string(),
                muted(&format!("cannot preview: {e}")),
            ]
        }
    };

    let mut head = Vec::new();
    let read = File::open(path).and_then(|file| file.take(MAX_BYTES).read_to_end(&mut head));
    let body = match read {
        Err(e) => Body::message(format!("cannot preview: {e}")),
        Ok(_) if head.is_empty() => Body::message("empty file"),
        Ok(_) => content(path, &head, metadata.len(), height.saturating_sub(3), width),
    };

    let modified = metadata
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let mut details = vec![
        colorize_size(metadata.len()).to_string(),
        colorize_date_relative(&modified).to_string(),
    ];
    if let Some(kind) = &body.kind {
        details.insert(0, paint(kind, palette::current().accent).to_string());
    }
    let mut lines = vec![
        name.bold().to_string(),
        details.join(&faint("  ·  ")),
        faint(&"─".repeat(width)),
    ];
    lines.extend(body.lines);
    lines.truncate(height);
    lines
}

struct Body {
    kind: Option<String>,
    lines: Vec<String>,
}

impl Body {
    fn message(text: impl Into<String>) -> Self {
        Self {
            kind: None,
            lines: vec![muted(&text.into())],
        }
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn content(path: &Path, head: &[u8], size: u64, height: usize, width: usize) -> Body {
    let ext = extension(path);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if let Some((format, w, h)) = image_size(head) {
        return image(format, w, h);
    }
    if ext == "zip" || ext == "jar" || ext == "whl" || head.starts_with(b"PK\x03\x04") {
        if let Some(body) = zip_listing(path, height) {
            return body;
        }
    }
    let gzip = head.starts_with(&[0x1f, 0x8b]);
    if name.ends_with(".tar.gz") || ext == "tgz" || (gzip && name.ends_with(".tar")) {
        if let Some(body) = tar_listing(path, true, height) {
            return body;
        }
    }
    if ext == "tar" {
        if let Some(body) = tar_listing(path, false, height) {
            return body;
        }
    }

    let sniff = &head[..head.len().min(BINARY_SNIFF_BYTES)];
    if sniff.contains(&0) || std::str::from_utf8(sniff).is_err() && !mostly_text(sniff) {
        return binary(head, height, width);
    }

    let text = String::from_utf8_lossy(head);
    let truncated = size > head.len() as u64;
    match ext.as_str() {
        "csv" | "tsv" => {
            let delimiter = if ext == "tsv" { b'\t' } else { b',' };
            if let Some(body) = table(&text, delimiter, height) {
                return body;
            }
        }
        "json" if !truncated => {
            if let Some(pretty) = pretty_json(&text) {
                return code(path, &pretty, "JSON", false, height);
            }
        }
        _ => {}
    }
    code(path, &text, "", truncated, height)
}

/// Invalid UTF-8 can still be text in another encoding; only call it binary
/// when there are control bytes too.
fn mostly_text(bytes: &[u8]) -> bool {
    let control = bytes
        .iter()
        .filter(|&&b| b < 0x09 || (0x0e..0x20).contains(&b))
        .count();
    control * 100 <= bytes.len()
}

fn image(format: &str, width: u32, height: u32) -> Body {
    let ui = palette::current();
    let ratio = if height > 0 {
        let gcd = gcd(width, height).max(1);
        format!("{}:{}", width / gcd, height / gcd)
    } else {
        String::new()
    };
    let mut lines = vec![format!(
        "{}  {}",
        muted("dimensions"),
        paint(&format!("{width} × {height}"), ui.text).bold()
    )];
    let megapixels = f64::from(width) * f64::from(height) / 1e6;
    if megapixels >= 0.1 {
        lines.push(format!(
            "{}  {}",
            muted("megapixels"),
            paint(&format!("{megapixels:.1}"), ui.text)
        ));
    }
    if !ratio.is_empty() && ratio.len() <= 9 {
        lines.push(format!(
            "{}  {}",
            muted("aspect    "),
            paint(&ratio, ui.text)
        ));
    }
    Body {
        kind: Some(format!("{format} image")),
        lines,
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Reads the pixel size from the first bytes of common image formats.
fn image_size(head: &[u8]) -> Option<(&'static str, u32, u32)> {
    let be16 = |at: usize| {
        Some(u32::from(u16::from_be_bytes(
            head.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let le16 = |at: usize| {
        Some(u32::from(u16::from_le_bytes(
            head.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let be32 = |at: usize| Some(u32::from_be_bytes(head.get(at..at + 4)?.try_into().ok()?));
    let le32 = |at: usize| Some(u32::from_le_bytes(head.get(at..at + 4)?.try_into().ok()?));
    let le24 = |at: usize| {
        let b = head.get(at..at + 3)?;
        Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
    };

    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(("PNG", be32(16)?, be32(20)?));
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some(("GIF", le16(6)?, le16(8)?));
    }
    if head.starts_with(b"BM") && head.len() > 26 {
        let height = le32(22)? as i32;
        return Some(("BMP", le32(18)?, height.unsigned_abs()));
    }
    if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
        return match head.get(12..16)? {
            b"VP8 " => Some(("WebP", le16(26)? & 0x3fff, le16(28)? & 0x3fff)),
            b"VP8L" => {
                let bits = le32(21)?;
                Some(("WebP", (bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
            }
            b"VP8X" => Some(("WebP", le24(24)? + 1, le24(27)? + 1)),
            _ => None,
        };
    }
    if head.starts_with(&[0xff, 0xd8]) {
        // Walk the JPEG segments to the first start-of-frame marker.
        let mut at = 2;
        while at + 9 < head.len() {
            if head[at] != 0xff {
                at += 1;
                continue;
            }
            let marker = head[at + 1];
            if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                return Some(("JPEG", be16(at + 7)?, be16(at + 5)?));
            }
            if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
                at += 2;
                continue;
            }
            at += 2 + be16(at + 2)? as usize;
        }
        return None;
    }
    None
}

fn listing(kind: String, rows: Vec<(String, u64, bool)>, more: Option<String>) -> Body {
    let ui = palette::current();
    let mut lines: Vec<String> = rows
        .into_iter()
        .map(|(name, size, dir)| {
            let size = if dir {
                faint(&format!("{:>9}", "-"))
            } else {
                muted(&format!("{:>9}", crate::scan::format::size(size)))
            };
            let name = if dir {
                paint(&name, ui.accent).to_string()
            } else {
                paint(&name, ui.text).to_string()
            };
            format!("{size}  {name}")
        })
        .collect();
    if let Some(more) = more {
        lines.push(faint(&more));
    }
    Body {
        kind: Some(kind),
        lines,
    }
}

fn zip_listing(path: &Path, height: usize) -> Option<Body> {
    let file = File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file)).ok()?;
    let total = archive.len();
    let mut unpacked = 0u64;
    let mut rows = Vec::new();
    for index in 0..total {
        let entry = archive.by_index_raw(index).ok()?;
        unpacked += entry.size();
        if rows.len() < height.saturating_sub(1) {
            rows.push((entry.name().to_string(), entry.size(), entry.is_dir()));
        }
    }
    let more = (total > rows.len()).then(|| format!("… {} more", total - rows.len()));
    Some(listing(
        format!(
            "zip · {} entries · {} unpacked",
            total,
            crate::scan::format::size(unpacked)
        ),
        rows,
        more,
    ))
}

fn tar_listing(path: &Path, gzip: bool, height: usize) -> Option<Body> {
    let file = BufReader::new(File::open(path).ok()?);
    let reader: Box<dyn Read> = if gzip {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut archive = tar::Archive::new(reader);
    let room = height.saturating_sub(1);
    let mut rows = Vec::new();
    let mut more = false;
    // Reading further means decompressing everything, so stop at a screenful.
    for entry in archive.entries().ok()? {
        let entry = entry.ok()?;
        if rows.len() == room {
            more = true;
            break;
        }
        let name = entry.path().ok()?.to_string_lossy().into_owned();
        let dir = entry.header().entry_type().is_dir();
        rows.push((name, entry.header().size().unwrap_or(0), dir));
    }
    if rows.is_empty() {
        return None;
    }
    let kind = if gzip {
        "tar.gz archive"
    } else {
        "tar archive"
    };
    Some(listing(
        kind.to_string(),
        rows,
        more.then(|| "…".to_string()),
    ))
}

fn table(text: &str, delimiter: u8, height: usize) -> Option<Body> {
    let ui = palette::current();
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let rows: Vec<Vec<String>> = reader
        .records()
        .take(height)
        .filter_map(|record| record.ok())
        .map(|record| {
            record
                .iter()
                .map(|cell| crate::scan::format::fit(&sanitize(cell), 24))
                .collect()
        })
        .collect();
    let columns = rows.iter().map(Vec::len).max()?;
    if columns < 2 {
        return None;
    }
    let widths: Vec<usize> = (0..columns)
        .map(|col| {
            rows.iter()
                .filter_map(|row| row.get(col))
                .map(|cell| console::measure_text_width(cell))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let numeric = |cell: &str| !cell.is_empty() && cell.parse::<f64>().is_ok();
    let separator = faint(" │ ");
    let lines = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            (0..columns)
                .map(|col| {
                    let cell = row.get(col).map(String::as_str).unwrap_or("");
                    let pad = widths[col].saturating_sub(console::measure_text_width(cell));
                    let padded = if index > 0 && numeric(cell) {
                        format!("{}{cell}", " ".repeat(pad))
                    } else {
                        format!("{cell}{}", " ".repeat(pad))
                    };
                    if index == 0 {
                        paint(&padded, ui.accent).bold().to_string()
                    } else if numeric(cell) {
                        paint(&padded, ui.warning).to_string()
                    } else {
                        paint(&padded, ui.text).to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(&separator)
        })
        .collect();
    let label = if delimiter == b'\t' { "TSV" } else { "CSV" };
    Some(Body {
        kind: Some(format!("{label} · {columns} columns")),
        lines,
    })
}

fn pretty_json(text: &str) -> Option<String> {
    // Only reformat files that are hard to read as they are.
    if text.lines().take(3).all(|line| line.len() < 200) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// Expands tabs and drops control characters so file content cannot send
/// escape sequences to the terminal.
pub fn sanitize(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars().take(MAX_LINE_CHARS) {
        match c {
            '\t' => out.push_str("    "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        let defaults = SyntaxSet::load_defaults_newlines();
        let toml = include_str!("syntaxes/toml.sublime-syntax");
        match SyntaxDefinition::load_from_str(toml, true, None) {
            Ok(definition) => {
                let mut builder = defaults.into_builder();
                builder.add(definition);
                builder.build()
            }
            Err(_) => defaults,
        }
    })
}

/// Starts loading the syntax definitions in the background, since the first
/// load takes a moment.
pub fn warm_up() {
    std::thread::spawn(|| {
        syntaxes();
        highlight_theme();
    });
}

fn find_syntax<'a>(set: &'a SyntaxSet, path: &Path, first_line: &str) -> &'a SyntaxReference {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = extension(path);
    let alias = match ext.as_str() {
        "ts" | "tsx" | "mts" | "cts" => Some("js"),
        "jsx" | "mjs" | "cjs" => Some("js"),
        "ini" | "cfg" | "conf" | "env" => Some("toml"),
        "zsh" | "fish" | "bash" => Some("sh"),
        "kts" | "gradle" => Some("java"),
        "vue" | "svelte" | "astro" => Some("html"),
        "jsonc" | "json5" => Some("json"),
        "lock" if name == "flake.lock" => Some("json"),
        _ => None,
    };
    let by_name = match name.as_str() {
        "Dockerfile" | "Containerfile" | "Makefile" | "Justfile" | "justfile" => Some("sh"),
        "Gemfile" | "Rakefile" => Some("rb"),
        "Cargo.lock" | "poetry.lock" | "uv.lock" | "Pipfile" => Some("toml"),
        _ => None,
    };
    set.find_syntax_by_extension(&ext)
        .or_else(|| set.find_syntax_by_extension(&name))
        .or_else(|| {
            alias
                .or(by_name)
                .and_then(|ext| set.find_syntax_by_extension(ext))
        })
        .or_else(|| set.find_syntax_by_first_line(first_line))
        .unwrap_or_else(|| set.find_syntax_plain_text())
}

/// Syntax colors for the preview, taken from the lla theme.
fn highlight_theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let ui = palette::current();
        let cyan = ui
            .near(185.0, 30.0, &[ui.accent])
            .unwrap_or_else(|| ui.accent.mix(ui.success, 0.5));
        let punctuation = ui.text.mix(ui.muted, 0.6);
        let rules: Vec<(&str, Tone, FontStyle)> = vec![
            ("comment, punctuation.definition.comment", ui.muted, FontStyle::ITALIC),
            ("string, constant.other.symbol, markup.raw, markup.inline.raw", ui.success, FontStyle::empty()),
            ("constant.character.escape, string.regexp", cyan, FontStyle::empty()),
            ("constant.numeric, constant.language, constant.other, support.constant", ui.warning, FontStyle::empty()),
            ("keyword, storage, keyword.operator.word", ui.info, FontStyle::empty()),
            ("keyword.operator, punctuation", punctuation, FontStyle::empty()),
            ("entity.name.function, support.function, meta.function-call variable.function, variable.function", ui.accent, FontStyle::empty()),
            ("entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, support.type, support.class, entity.other.inherited-class, storage.type.rust", cyan, FontStyle::empty()),
            ("entity.name.tag", ui.danger, FontStyle::empty()),
            ("entity.other.attribute-name", ui.warning, FontStyle::empty()),
            ("meta.mapping.key string, support.type.property-name, entity.name.tag.yaml, meta.object-literal.key", ui.accent, FontStyle::empty()),
            ("variable.language, variable.parameter", ui.danger.mix(ui.warning, 0.4), FontStyle::empty()),
            ("entity.name.section, markup.heading, markup.heading punctuation", ui.accent, FontStyle::BOLD),
            ("markup.bold", ui.text, FontStyle::BOLD),
            ("markup.italic, markup.quote", ui.muted, FontStyle::ITALIC),
            ("markup.underline.link, string.other.link", ui.info, FontStyle::UNDERLINE),
            ("markup.list punctuation.definition.list", ui.warning, FontStyle::empty()),
            ("markup.inserted", ui.success, FontStyle::empty()),
            ("markup.deleted", ui.danger, FontStyle::empty()),
            ("meta.diff.range, meta.diff.header", ui.info, FontStyle::empty()),
            ("invalid", ui.danger, FontStyle::empty()),
        ];
        Theme {
            name: Some("lla".to_string()),
            author: None,
            settings: ThemeSettings {
                foreground: Some(encode(ui.text)),
                ..ThemeSettings::default()
            },
            scopes: rules
                .into_iter()
                .filter_map(|(scope, tone, font_style)| {
                    Some(ThemeItem {
                        scope: scope.parse::<ScopeSelectors>().ok()?,
                        style: StyleModifier {
                            foreground: Some(encode(tone)),
                            background: None,
                            font_style: Some(font_style),
                        },
                    })
                })
                .collect(),
        }
    })
}

/// Syntect colors are RGBA; alpha 0 marks a terminal palette index in `r`,
/// so named theme colors keep following the terminal's palette.
fn encode(tone: Tone) -> SynColor {
    match tone.ansi {
        Some(index) => SynColor {
            r: index,
            g: 0,
            b: 0,
            a: 0,
        },
        None => SynColor {
            r: tone.rgb.0,
            g: tone.rgb.1,
            b: tone.rgb.2,
            a: 255,
        },
    }
}

fn decode(color: SynColor) -> Tone {
    if color.a == 0 {
        Tone {
            rgb: (0, 0, 0),
            ansi: Some(color.r),
        }
    } else {
        Tone::rgb(color.r, color.g, color.b)
    }
}

/// `truncated` when only the start of the file was read, so the line count
/// would be wrong.
fn code(path: &Path, text: &str, kind: &str, truncated: bool, height: usize) -> Body {
    let set = syntaxes();
    let first_line = text.lines().next().unwrap_or("");
    let syntax = find_syntax(set, path, first_line);
    let plain = syntax.name == set.find_syntax_plain_text().name;
    let mut highlighter = HighlightLines::new(syntax, highlight_theme());

    let lines: Vec<&str> = text.lines().take(height).collect();
    let gutter = lines.len().max(1).to_string().len();
    let ui = palette::current();
    let number_tone = ui.muted.mix(ui.faint, 0.5);
    let rendered = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let number = paint(&format!("{:>gutter$} ", index + 1), number_tone).to_string();
            let clean = sanitize(line);
            let body = if plain {
                paint(&clean, ui.text).to_string()
            } else {
                let with_newline = format!("{clean}\n");
                match highlighter.highlight_line(&with_newline, set) {
                    Ok(ranges) => ranges
                        .into_iter()
                        .map(|(style, piece)| {
                            let piece = piece.trim_end_matches('\n');
                            let mut out = paint(piece, decode(style.foreground));
                            if style.font_style.contains(FontStyle::BOLD) {
                                out = out.bold();
                            }
                            if style.font_style.contains(FontStyle::ITALIC) {
                                out = out.italic();
                            }
                            if style.font_style.contains(FontStyle::UNDERLINE) {
                                out = out.underline();
                            }
                            out.to_string()
                        })
                        .collect(),
                    Err(_) => clean,
                }
            };
            format!("{number}{body}")
        })
        .collect();

    let label = if !kind.is_empty() {
        kind.to_string()
    } else if plain {
        "text".to_string()
    } else {
        syntax.name.clone()
    };
    let kind = if truncated {
        label
    } else {
        let total = text.lines().count() as u64;
        format!("{label} · {} lines", crate::scan::format::count(total))
    };
    Body {
        kind: Some(kind),
        lines: rendered,
    }
}

/// What a binary file most likely is, from its first bytes.
fn binary_kind(head: &[u8]) -> &'static str {
    let starts = |magic: &[u8]| head.starts_with(magic);
    let at = |offset: usize, magic: &[u8]| head.get(offset..offset + magic.len()) == Some(magic);
    if starts(b"\x7fELF") {
        "ELF executable"
    } else if starts(&[0xcf, 0xfa, 0xed, 0xfe])
        || starts(&[0xce, 0xfa, 0xed, 0xfe])
        || starts(&[0xca, 0xfe, 0xba, 0xbe])
    {
        "Mach-O binary"
    } else if starts(b"MZ") {
        "Windows executable"
    } else if starts(b"%PDF-") {
        "PDF document"
    } else if starts(b"SQLite format 3\0") {
        "SQLite database"
    } else if starts(b"\0asm") {
        "WebAssembly module"
    } else if starts(&[0x1f, 0x8b]) {
        "gzip data"
    } else if starts(&[0x28, 0xb5, 0x2f, 0xfd]) {
        "zstd data"
    } else if starts(b"\xfd7zXZ\0") {
        "xz data"
    } else if starts(b"7z\xbc\xaf\x27\x1c") {
        "7-zip archive"
    } else if starts(b"Rar!") {
        "RAR archive"
    } else if starts(b"ID3") || starts(&[0xff, 0xfb]) {
        "MP3 audio"
    } else if starts(b"fLaC") {
        "FLAC audio"
    } else if starts(b"OggS") {
        "Ogg media"
    } else if starts(b"RIFF") && at(8, b"WAVE") {
        "WAV audio"
    } else if starts(b"RIFF") && at(8, b"AVI ") {
        "AVI video"
    } else if at(4, b"ftyp") {
        "MP4 / QuickTime media"
    } else if starts(&[0x1a, 0x45, 0xdf, 0xa3]) {
        "Matroska / WebM video"
    } else if starts(b"wOFF") || starts(b"wOF2") {
        "web font"
    } else if starts(b"OTTO") || starts(&[0, 1, 0, 0]) {
        "font"
    } else if starts(&[0xca, 0xfe, 0xba, 0xbe]) {
        "Java class"
    } else if starts(b"bplist") {
        "binary property list"
    } else {
        "binary"
    }
}

/// A hex dump sized to the preview, with an ASCII column.
fn binary(head: &[u8], height: usize, width: usize) -> Body {
    let ui = palette::current();
    let per_row = if width >= 78 { 16 } else { 8 };
    let lines = head
        .chunks(per_row)
        .take(height)
        .enumerate()
        .map(|(row, bytes)| {
            let mut hex = String::new();
            for (index, byte) in bytes.iter().enumerate() {
                if index == 8 {
                    hex.push(' ');
                }
                let text = format!("{byte:02x} ");
                hex.push_str(&match byte {
                    0 => faint(&text),
                    b if b.is_ascii_graphic() => paint(&text, ui.text).to_string(),
                    _ => paint(&text, ui.warning).to_string(),
                });
            }
            let missing = per_row - bytes.len();
            hex.push_str(&" ".repeat(missing * 3 + usize::from(per_row > 8 && bytes.len() <= 8)));
            let ascii: String = bytes
                .iter()
                .map(|&b| {
                    if b.is_ascii_graphic() || b == b' ' {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            format!(
                "{}  {} {}",
                faint(&format!("{:08x}", row * per_row)),
                hex,
                muted(&ascii)
            )
        })
        .collect();
    Body {
        kind: Some(binary_kind(head).to_string()),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .map(|line| console::strip_ansi_codes(line).into_owned())
            .collect()
    }

    #[test]
    fn reads_image_sizes() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        assert_eq!(image_size(&png), Some(("PNG", 640, 480)));

        let gif = b"GIF89a\x20\x03\x58\x02";
        assert_eq!(image_size(gif), Some(("GIF", 800, 600)));

        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0];
        jpeg.extend([0xff, 0xc0, 0, 17, 8, 0x01, 0xe0, 0x02, 0x80, 3]);
        assert_eq!(image_size(&jpeg), Some(("JPEG", 640, 480)));
    }

    #[test]
    fn previews_common_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let rust = dir.path().join("main.rs");
        std::fs::write(&rust, "fn main() {\n\tprintln!(\"hi\");\n}\n").unwrap();
        let lines = strip(&build(&rust, 20, 60));
        assert_eq!(lines[0], "main.rs");
        assert!(lines[1].starts_with("Rust · 3 lines"), "{lines:?}");
        assert_eq!(lines[3], "1 fn main() {");
        assert_eq!(lines[4], "2     println!(\"hi\");");

        let csv = dir.path().join("data.csv");
        std::fs::write(&csv, "name,size\nalpha,10\nbeta,200\n").unwrap();
        let lines = strip(&build(&csv, 20, 60));
        assert!(lines[1].starts_with("CSV · 2 columns"));
        assert_eq!(lines[3], "name  │ size");
        assert_eq!(lines[5], "beta  │  200");

        let bin = dir.path().join("blob");
        std::fs::write(&bin, b"\x7fELF\x02\x01\x01\0\0\0").unwrap();
        let lines = strip(&build(&bin, 20, 100));
        assert!(lines[1].starts_with("ELF executable"));
        assert!(lines[3].starts_with("00000000  7f 45 4c 46"), "{lines:?}");

        let zip_path = dir.path().join("a.zip");
        {
            let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
            zip.start_file::<_, ()>("hello.txt", zip::write::FileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, b"hello").unwrap();
            zip.finish().unwrap();
        }
        let lines = strip(&build(&zip_path, 20, 60));
        assert!(lines[1].starts_with("zip · 1 entries"), "{lines:?}");
        assert!(lines[3].ends_with("hello.txt"));

        let empty = dir.path().join("empty");
        std::fs::write(&empty, "").unwrap();
        assert_eq!(strip(&build(&empty, 20, 60))[3], "empty file");
    }

    #[test]
    fn file_content_cannot_reach_the_terminal() {
        assert_eq!(sanitize("a\x1b[31mb\tc"), "a[31mb    c");
    }
}
