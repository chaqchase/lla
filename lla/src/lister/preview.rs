//! File previews for the fuzzy finder: highlighted source with line numbers,
//! tables for CSV, listings for archives, image sizes and hex dumps, all in
//! the colors of the current theme.

use super::graphics::{self, Graphic, Protocol};
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
use syntect::parsing::{SyntaxDefinition, SyntaxReference, SyntaxSet, SyntaxSetBuilder};

const MAX_BYTES: u64 = 64 * 1024;
/// When opening at a matched line, read this far to reach it.
const MAX_BYTES_AT_LINE: u64 = 8 * 1024 * 1024;
/// Lines highlighted before the shown window so the colors are right there.
const MAX_LEAD_IN_LINES: usize = 2_000;
const MAX_FOLDER_ENTRIES: usize = 10_000;
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
    /// Open at this line and mark it (text search results).
    pub line: Option<u32>,
}

#[derive(Clone)]
pub struct Preview {
    pub request: Request,
    pub lines: Vec<String>,
    /// An image to draw over the preview with the Kitty protocol, and the
    /// preview line its top sits on.
    pub graphic: Option<(Graphic, usize)>,
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
            let preview = match cache.iter().find(|cached| cached.request == request) {
                Some(cached) => cached.clone(),
                None => {
                    let preview = render(&request);
                    if cache.len() == CACHE_SIZE {
                        cache.pop_front();
                    }
                    cache.push_back(preview.clone());
                    preview
                }
            };
            if previews.send(preview).is_err() {
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

#[cfg(test)]
pub fn build(path: &Path, height: usize, width: usize) -> Vec<String> {
    render(&Request {
        path: path.to_path_buf(),
        height,
        width,
        line: None,
    })
    .lines
}

/// A header, a rule, then the content.
pub fn render(request: &Request) -> Preview {
    let Request {
        path,
        height,
        width,
        line,
    } = request;
    let (height, width) = (*height, *width);
    let done = |lines: Vec<String>, graphic: Option<(Graphic, usize)>| Preview {
        request: request.clone(),
        lines,
        graphic,
    };
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(e) => {
            return done(
                vec![
                    name.bold().to_string(),
                    muted(&format!("cannot preview: {e}")),
                ],
                None,
            )
        }
    };
    let room = height.saturating_sub(3);

    let body = if metadata.is_dir() {
        folder(path, room)
    } else {
        let limit = if line.is_some() {
            MAX_BYTES_AT_LINE
        } else {
            MAX_BYTES
        };
        let mut head = Vec::new();
        let read = File::open(path).and_then(|file| file.take(limit).read_to_end(&mut head));
        match read {
            Err(e) => Body::message(format!("cannot preview: {e}")),
            Ok(_) if head.is_empty() => Body::message("empty file"),
            Ok(_) => content(path, &head, metadata.len(), room, width, *line),
        }
    };

    let modified = metadata
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let mut details = Vec::new();
    if let Some(kind) = &body.kind {
        details.push(paint(kind, palette::current().accent).to_string());
    }
    if !metadata.is_dir() {
        details.push(colorize_size(metadata.len()).to_string());
    }
    details.push(colorize_date_relative(&modified).to_string());
    let mut lines = vec![
        name.bold().to_string(),
        details.join(&faint("  ·  ")),
        faint(&"─".repeat(width)),
    ];
    let graphic = body.graphic.map(|graphic| (graphic, lines.len()));
    lines.extend(body.lines);
    lines.truncate(height);
    done(lines, graphic)
}

/// The entries of a folder, folders first.
fn folder(path: &Path, height: usize) -> Body {
    let ui = palette::current();
    let Ok(read) = std::fs::read_dir(path) else {
        return Body::message("cannot read this folder");
    };
    let mut entries: Vec<(bool, String, u64)> = Vec::new();
    let mut total = 0;
    for entry in read.flatten() {
        total += 1;
        if entries.len() < MAX_FOLDER_ENTRIES {
            let meta = entry.metadata().ok();
            let dir = meta.as_ref().is_some_and(|meta| meta.is_dir());
            let size = meta.map_or(0, |meta| meta.len());
            entries.push((dir, entry.file_name().to_string_lossy().into_owned(), size));
        }
    }
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });
    let shown = height.saturating_sub(1);
    let mut lines: Vec<String> = entries
        .iter()
        .take(shown)
        .map(|(dir, name, size)| {
            if *dir {
                format!(
                    "{}  {}{}",
                    faint(&format!("{:>9}", "-")),
                    paint(name, ui.accent),
                    muted(std::path::MAIN_SEPARATOR_STR)
                )
            } else {
                format!(
                    "{}  {}",
                    muted(&format!("{:>9}", crate::scan::format::size(*size))),
                    paint(name, ui.text)
                )
            }
        })
        .collect();
    if total > shown {
        lines.push(faint(&format!("… {} more", total - shown.min(total))));
    }
    if total == 0 {
        lines.push(muted("empty folder"));
    }
    let count = if total > MAX_FOLDER_ENTRIES {
        format!("{}+", crate::scan::format::count(MAX_FOLDER_ENTRIES as u64))
    } else {
        crate::scan::format::count(total as u64)
    };
    let items = if total == 1 { "item" } else { "items" };
    Body {
        kind: Some(format!("folder · {count} {items}")),
        lines,
        graphic: None,
    }
}

struct Body {
    kind: Option<String>,
    lines: Vec<String>,
    /// Drawn with the Kitty protocol over the first lines of the body.
    graphic: Option<Graphic>,
}

impl Body {
    fn message(text: impl Into<String>) -> Self {
        Self {
            kind: None,
            lines: vec![muted(&text.into())],
            graphic: None,
        }
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn content(
    path: &Path,
    head: &[u8],
    size: u64,
    height: usize,
    width: usize,
    line: Option<u32>,
) -> Body {
    let ext = extension(path);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if let Some((format, w, h)) = image_size(head) {
        return image(path, format, w, h, height, width);
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
                return code(path, &pretty, "JSON", false, height, None);
            }
        }
        _ => {}
    }
    code(path, &text, "", truncated, height, line)
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

/// The image itself where the terminal can show it, then its size.
fn image(path: &Path, format: &str, width: u32, height: u32, rows: usize, cols: usize) -> Body {
    let ui = palette::current();
    let mut info = vec![format!(
        "{}  {}",
        muted("dimensions"),
        paint(&format!("{width} × {height}"), ui.text).bold()
    )];
    let megapixels = f64::from(width) * f64::from(height) / 1e6;
    if megapixels >= 0.1 {
        info.push(format!(
            "{}  {}",
            muted("megapixels"),
            paint(&format!("{megapixels:.1}"), ui.text)
        ));
    }

    // Leave room for a blank line and the size underneath.
    let picture_rows = rows.saturating_sub(info.len() + 1);
    let protocol = graphics::protocol();
    let decoded = (protocol != Protocol::Off && picture_rows >= 4)
        .then(|| graphics::decode(path))
        .flatten();
    let mut lines = Vec::new();
    let mut graphic = None;
    if let Some(decoded) = decoded {
        match protocol {
            Protocol::Kitty => {
                if let Some(picture) = graphics::kitty_graphic(&decoded, cols, picture_rows) {
                    lines.extend(std::iter::repeat_n(String::new(), picture.rows as usize));
                    graphic = Some(picture);
                }
            }
            Protocol::Blocks => lines.extend(graphics::blocks(&decoded, cols, picture_rows)),
            Protocol::Off => {}
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
    }
    lines.extend(info);
    Body {
        kind: Some(format!("{format} image")),
        lines,
        graphic,
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
        graphic: None,
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
        graphic: None,
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
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Grammars syntect does not ship. They live in their own small set so the
/// bundled one never has to be rebuilt, which is slow.
fn extra_syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        let mut builder = SyntaxSetBuilder::new();
        let toml = include_str!("syntaxes/toml.sublime-syntax");
        if let Ok(definition) = SyntaxDefinition::load_from_str(toml, true, None) {
            builder.add(definition);
        }
        builder.build()
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

/// The grammar for a file, and the set it belongs to.
fn find_syntax(path: &Path, first_line: &str) -> (&'static SyntaxSet, &'static SyntaxReference) {
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
    let tokens = [Some(ext.as_str()), Some(name.as_str()), alias.or(by_name)];
    for token in tokens.into_iter().flatten() {
        for set in [syntaxes(), extra_syntaxes()] {
            if let Some(syntax) = set.find_syntax_by_extension(token) {
                return (set, syntax);
            }
        }
    }
    let set = syntaxes();
    let syntax = set
        .find_syntax_by_first_line(first_line)
        .unwrap_or_else(|| set.find_syntax_plain_text());
    (set, syntax)
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
fn code(
    path: &Path,
    text: &str,
    kind: &str,
    truncated: bool,
    height: usize,
    focus: Option<u32>,
) -> Body {
    let first_line = text.lines().next().unwrap_or("");
    let (set, syntax) = find_syntax(path, first_line);
    let plain = syntax.name == "Plain Text";
    let mut highlighter = HighlightLines::new(syntax, highlight_theme());

    // With a focus line, show it a third of the way down.
    let first = focus.map_or(0, |line| (line as usize).saturating_sub(1 + height / 3));
    let lead_in = first.min(MAX_LEAD_IN_LINES);
    let mut all = text.lines().skip(first - lead_in);
    if !plain {
        // Run the lines above the window through the highlighter so strings
        // and comments that started earlier are colored right.
        for line in all.by_ref().take(lead_in) {
            let _ = highlighter.highlight_line(&format!("{}\n", sanitize(line)), set);
        }
    } else {
        all.by_ref().take(lead_in).for_each(drop);
    }
    let lines: Vec<&str> = all.take(height).collect();
    let gutter = (first + lines.len()).max(1).to_string().len();
    let ui = palette::current();
    let number_tone = ui.muted.mix(ui.faint, 0.5);
    let rendered = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let number = first + index + 1;
            let focused = focus == Some(number as u32);
            let number = if focused {
                paint(&format!("{number:>gutter$}▌"), ui.warning)
                    .bold()
                    .to_string()
            } else {
                paint(&format!("{number:>gutter$} "), number_tone).to_string()
            };
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
        graphic: None,
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
        graphic: None,
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
    fn toml_lines_folders_and_images() {
        let dir = tempfile::TempDir::new().unwrap();
        let toml = dir.path().join("Cargo.toml");
        std::fs::write(&toml, "[package]\nname = \"lla\"\n").unwrap();
        assert!(strip(&build(&toml, 20, 60))[1].starts_with("TOML · 2 lines"));
        let lock = dir.path().join("Cargo.lock");
        std::fs::write(&lock, "version = 4\n").unwrap();
        assert!(strip(&build(&lock, 20, 60))[1].starts_with("TOML"));

        // Opening at a line puts it a third of the way down and marks it.
        let long = dir.path().join("long.txt");
        let text: String = (1..=100).map(|n| format!("line {n}\n")).collect();
        std::fs::write(&long, text).unwrap();
        let preview = render(&Request {
            path: long.clone(),
            height: 13,
            width: 60,
            line: Some(50),
        });
        let lines = strip(&preview.lines);
        let marked: Vec<_> = lines.iter().filter(|line| line.contains('▌')).collect();
        assert_eq!(marked, vec!["50▌line 50"]);
        assert_eq!(lines[3], "47 line 47");

        let folder_path = dir.path().join("folder");
        std::fs::create_dir_all(folder_path.join("sub")).unwrap();
        std::fs::write(folder_path.join("a.txt"), "a").unwrap();
        let lines = strip(&build(&folder_path, 20, 60));
        assert!(lines[1].starts_with("folder · 2 items"), "{lines:?}");
        assert!(lines[3].trim_start().starts_with("-  sub"), "{lines:?}");
        assert!(lines[4].ends_with("a.txt"));

        // Blocks are the fallback everywhere; the size is always listed.
        let png = dir.path().join("dot.png");
        image::RgbaImage::from_pixel(64, 32, image::Rgba([0, 128, 255, 255]))
            .save(&png)
            .unwrap();
        let preview = render(&Request {
            path: png,
            height: 20,
            width: 60,
            line: None,
        });
        let lines = strip(&preview.lines);
        assert!(lines[1].starts_with("PNG image"));
        assert!(lines.iter().any(|line| line.contains("64 × 32")));
        match graphics::protocol() {
            Protocol::Kitty => assert!(preview.graphic.is_some()),
            Protocol::Blocks => assert!(lines.iter().any(|line| line.contains('▀'))),
            Protocol::Off => {}
        }
    }

    #[test]
    fn file_content_cannot_reach_the_terminal() {
        assert_eq!(sanitize("a\x1b[31mb\tc"), "a[31mb    c");
    }
}
