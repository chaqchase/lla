//! Image thumbnails for the finder preview.
//!
//! Terminals that speak the Kitty graphics protocol (Kitty, Ghostty, WezTerm)
//! get the real image. Everywhere else, including Windows and tmux, the image
//! is drawn with half-block characters, two pixels per cell.

use base64::Engine;
use image::{DynamicImage, GenericImageView};
use std::io::Cursor;
use std::path::Path;
use std::sync::OnceLock;

/// Larger files are not decoded for a preview.
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
/// The id lla's preview image uses, so it can be replaced and removed.
pub const KITTY_IMAGE_ID: u32 = 7_301;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Kitty,
    Blocks,
    Off,
}

/// How images are shown in this terminal. `LLA_IMAGES=kitty|blocks|off`
/// overrides the detection.
pub fn protocol() -> Protocol {
    static PROTOCOL: OnceLock<Protocol> = OnceLock::new();
    *PROTOCOL.get_or_init(|| {
        let var = |name: &str| std::env::var(name).unwrap_or_default();
        match var("LLA_IMAGES").to_lowercase().as_str() {
            "kitty" => return Protocol::Kitty,
            "blocks" => return Protocol::Blocks,
            "off" | "none" | "0" => return Protocol::Off,
            _ => {}
        }
        if crate::theme::is_no_color() {
            return Protocol::Off;
        }
        detect(&var("TERM"), &var("TERM_PROGRAM"), |name| {
            std::env::var_os(name).is_some()
        })
    })
}

fn detect(term: &str, program: &str, is_set: impl Fn(&str) -> bool) -> Protocol {
    // Multiplexers need passthrough for graphics, which is often off.
    if is_set("TMUX") || term.starts_with("screen") || term.starts_with("tmux") || is_set("ZELLIJ")
    {
        return Protocol::Blocks;
    }
    let kitty = is_set("KITTY_WINDOW_ID")
        || is_set("GHOSTTY_RESOURCES_DIR")
        || term == "xterm-kitty"
        || term == "xterm-ghostty"
        || matches!(program, "ghostty" | "WezTerm");
    if kitty {
        Protocol::Kitty
    } else {
        Protocol::Blocks
    }
}

fn truecolor() -> bool {
    static TRUECOLOR: OnceLock<bool> = OnceLock::new();
    *TRUECOLOR.get_or_init(|| {
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        colorterm.contains("truecolor")
            || colorterm.contains("24bit")
            || std::env::var_os("WT_SESSION").is_some()
            || matches!(
                program.as_str(),
                "iTerm.app" | "WezTerm" | "ghostty" | "vscode" | "Hyper" | "rio" | "Tabby"
            )
    })
}

/// Width and height of one terminal cell in pixels, when the terminal says.
fn cell_pixels() -> (f64, f64) {
    if let Ok(size) = crossterm::terminal::window_size() {
        if size.width > 0 && size.height > 0 && size.columns > 0 && size.rows > 0 {
            return (
                f64::from(size.width) / f64::from(size.columns),
                f64::from(size.height) / f64::from(size.rows),
            );
        }
    }
    (8.0, 16.0)
}

pub fn decode(path: &Path) -> Option<DynamicImage> {
    if path.metadata().ok()?.len() > MAX_IMAGE_BYTES {
        return None;
    }
    image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()
}

/// An image ready to send with the Kitty protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graphic {
    /// PNG data, base64 encoded.
    pub data: String,
    pub cols: u16,
    pub rows: u16,
}

/// Scales `image` to fit `cols` × `rows` cells, keeping its shape.
pub fn kitty_graphic(image: &DynamicImage, cols: usize, rows: usize) -> Option<Graphic> {
    let (cell_w, cell_h) = cell_pixels();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || cols == 0 || rows == 0 {
        return None;
    }
    let box_w = cols as f64 * cell_w;
    let box_h = rows as f64 * cell_h;
    let scale = (box_w / f64::from(width))
        .min(box_h / f64::from(height))
        .min(1.0);
    let target_w = (f64::from(width) * scale).round().max(1.0) as u32;
    let target_h = (f64::from(height) * scale).round().max(1.0) as u32;
    let resized = if scale < 1.0 {
        image.thumbnail(target_w, target_h)
    } else {
        image.clone()
    };
    let mut png = Vec::new();
    resized
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(Graphic {
        data: base64::engine::general_purpose::STANDARD.encode(png),
        cols: ((f64::from(target_w) / cell_w).ceil() as u16).clamp(1, cols as u16),
        rows: ((f64::from(target_h) / cell_h).ceil() as u16).clamp(1, rows as u16),
    })
}

/// The escape sequences that draw `graphic` with its top-left corner at the
/// cursor, replacing any earlier preview image.
pub fn kitty_place(graphic: &Graphic) -> String {
    let mut out = kitty_delete();
    let chunks: Vec<&[u8]> = graphic.data.as_bytes().chunks(4096).collect();
    for (index, chunk) in chunks.iter().enumerate() {
        let more = u8::from(index + 1 < chunks.len());
        let chunk = std::str::from_utf8(chunk).unwrap_or_default();
        if index == 0 {
            // q=2: no replies, C=1: leave the cursor where it is.
            out.push_str(&format!(
                "\x1b_Ga=T,f=100,i={},c={},r={},C=1,q=2,m={};{}\x1b\\",
                KITTY_IMAGE_ID, graphic.cols, graphic.rows, more, chunk
            ));
        } else {
            out.push_str(&format!("\x1b_Gm={more};{chunk}\x1b\\"));
        }
    }
    out
}

/// Removes the preview image and frees its data.
pub fn kitty_delete() -> String {
    format!("\x1b_Ga=d,d=I,i={KITTY_IMAGE_ID},q=2\x1b\\")
}

/// Draws `image` with `▀`, two pixels per cell, in at most `cols` × `rows`
/// cells.
pub fn blocks(image: &DynamicImage, cols: usize, rows: usize) -> Vec<String> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || cols == 0 || rows == 0 {
        return Vec::new();
    }
    // Each half cell is roughly square, so the pixel grid is cols × rows*2.
    let (cell_w, cell_h) = cell_pixels();
    let pixel_aspect = (cell_h / 2.0) / cell_w;
    let box_w = cols as f64;
    let box_h = rows as f64 * 2.0;
    let scale = (box_w / f64::from(width)).min(box_h * pixel_aspect / f64::from(height));
    let target_w = (f64::from(width) * scale).round().clamp(1.0, box_w) as u32;
    let target_h = (f64::from(height) * scale / pixel_aspect)
        .round()
        .clamp(1.0, box_h) as u32;
    let small = image
        .resize_exact(target_w, target_h, image::imageops::FilterType::Triangle)
        .to_rgba8();

    let truecolor = truecolor();
    let color = |layer: u8, [r, g, b, _]: [u8; 4]| {
        if truecolor {
            format!("\x1b[{layer}8;2;{r};{g};{b}m")
        } else {
            format!("\x1b[{layer}8;5;{}m", ansi256(r, g, b))
        }
    };
    let opaque = |pixel: &[u8; 4]| pixel[3] >= 128;
    (0..target_h.div_ceil(2))
        .map(|row| {
            let mut line = String::new();
            for x in 0..target_w {
                let top = small.get_pixel(x, row * 2).0;
                let bottom = if row * 2 + 1 < target_h {
                    small.get_pixel(x, row * 2 + 1).0
                } else {
                    [0, 0, 0, 0]
                };
                match (opaque(&top), opaque(&bottom)) {
                    (true, true) => {
                        line.push_str(&color(3, top));
                        line.push_str(&color(4, bottom));
                        line.push('▀');
                    }
                    (true, false) => {
                        line.push_str("\x1b[49m");
                        line.push_str(&color(3, top));
                        line.push('▀');
                    }
                    (false, true) => {
                        line.push_str("\x1b[49m");
                        line.push_str(&color(3, bottom));
                        line.push('▄');
                    }
                    (false, false) => {
                        line.push_str("\x1b[0m ");
                    }
                }
            }
            line.push_str("\x1b[0m");
            line
        })
        .collect()
}

fn ansi256(r: u8, g: u8, b: u8) -> u8 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn detection_prefers_safe_fallbacks() {
        let none = |_: &str| false;
        assert_eq!(detect("xterm-kitty", "", none), Protocol::Kitty);
        assert_eq!(detect("xterm-256color", "ghostty", none), Protocol::Kitty);
        assert_eq!(
            detect("xterm-256color", "Apple_Terminal", none),
            Protocol::Blocks
        );
        assert_eq!(
            detect("xterm-256color", "ghostty", |name| name == "TMUX"),
            Protocol::Blocks
        );
        assert_eq!(detect("screen-256color", "WezTerm", none), Protocol::Blocks);
    }

    #[test]
    fn blocks_keep_the_shape_and_fit_the_box() {
        let image =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(200, 100, Rgba([255, 0, 0, 255])));
        let lines = blocks(&image, 40, 30);
        assert!(!lines.is_empty() && lines.len() <= 30);
        let width = console::measure_text_width(&lines[0]);
        assert!(width <= 40 && width >= 30, "{width}");
        // A 2:1 image drawn 40 cells wide is about 10 rows of two pixels.
        assert!((8..=12).contains(&lines.len()), "{}", lines.len());
        assert!(lines[0].contains('▀'));
    }

    #[test]
    fn kitty_sequences_are_chunked_and_quiet() {
        let graphic = Graphic {
            data: "A".repeat(9000),
            cols: 10,
            rows: 5,
        };
        let out = kitty_place(&graphic);
        assert!(out.starts_with(&kitty_delete()));
        assert!(out.contains("a=T,f=100,i=7301,c=10,r=5,C=1,q=2,m=1;"));
        assert_eq!(out.matches("\x1b_Gm=1;").count(), 1);
        assert_eq!(out.matches("\x1b_Gm=0;").count(), 1);
    }
}
