//! UI roles (accent, muted text, success, warning, danger, ...) taken from the
//! active theme, for the interactive views.
//!
//! Themes only name file-listing slots, and those slots do not mean the same
//! thing everywhere: `permission_exec` is red in most themes but green in the
//! default one. So each role is picked by hue from everything the theme
//! defines, and falls back to a sensible color only when the theme has nothing
//! close.

use super::{ColorValue, Theme};
use colored::Color;
use std::sync::OnceLock;

pub type Rgb = (u8, u8, u8);

/// A theme color. Named ANSI colors keep their index so terminals render them
/// with their own palette; `rgb` is an approximation used for shading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tone {
    pub rgb: Rgb,
    pub ansi: Option<u8>,
}

impl Tone {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            rgb: (r, g, b),
            ansi: None,
        }
    }

    /// For text printed with the `colored` crate.
    pub fn colored(self) -> Color {
        match self.ansi {
            Some(index) => ANSI_COLORED[index as usize],
            None => Color::TrueColor {
                r: self.rgb.0,
                g: self.rgb.1,
                b: self.rgb.2,
            },
        }
    }

    /// The same hue at a different brightness. Always RGB.
    pub fn shade(self, factor: f32) -> Tone {
        let scale = |v: u8| (f32::from(v) * factor).clamp(0.0, 255.0) as u8;
        Tone::rgb(scale(self.rgb.0), scale(self.rgb.1), scale(self.rgb.2))
    }

    /// Blends toward `other`; `amount` 0 keeps self, 1 gives other.
    pub fn mix(self, other: Tone, amount: f32) -> Tone {
        let blend = |a: u8, b: u8| {
            (f32::from(a) + (f32::from(b) - f32::from(a)) * amount).clamp(0.0, 255.0) as u8
        };
        Tone::rgb(
            blend(self.rgb.0, other.rgb.0),
            blend(self.rgb.1, other.rgb.1),
            blend(self.rgb.2, other.rgb.2),
        )
    }

    fn hsl(self) -> (f32, f32, f32) {
        let (r, g, b) = (
            f32::from(self.rgb.0) / 255.0,
            f32::from(self.rgb.1) / 255.0,
            f32::from(self.rgb.2) / 255.0,
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d < 1e-6 {
            return (0.0, 0.0, l);
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6);
        let h = if max == r {
            60.0 * (((g - b) / d) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        ((h + 360.0) % 360.0, s, l)
    }

    pub fn luminance(self) -> f32 {
        let (r, g, b) = self.rgb;
        (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0
    }
}

const ANSI_COLORED: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::White,
    Color::BrightBlack,
    Color::BrightRed,
    Color::BrightGreen,
    Color::BrightYellow,
    Color::BrightBlue,
    Color::BrightMagenta,
    Color::BrightCyan,
    Color::BrightWhite,
];

/// Close to what most dark terminal palettes use, only for shading and hue.
const ANSI_RGB: [Rgb; 16] = [
    (40, 44, 52),
    (224, 108, 117),
    (152, 195, 121),
    (229, 192, 123),
    (97, 175, 239),
    (198, 120, 221),
    (86, 182, 194),
    (220, 223, 228),
    (92, 99, 112),
    (240, 128, 136),
    (170, 215, 140),
    (240, 210, 140),
    (120, 190, 250),
    (215, 140, 235),
    (110, 200, 212),
    (250, 250, 252),
];

fn named(name: &str) -> Option<u8> {
    let index = match name.to_lowercase().replace([' ', '-'], "_").as_str() {
        "black" => 0,
        "red" => 1,
        "green" => 2,
        "yellow" => 3,
        "blue" => 4,
        "magenta" | "purple" => 5,
        "cyan" => 6,
        "white" => 7,
        "bright_black" | "gray" | "grey" => 8,
        "bright_red" => 9,
        "bright_green" => 10,
        "bright_yellow" => 11,
        "bright_blue" => 12,
        "bright_magenta" => 13,
        "bright_cyan" => 14,
        "bright_white" => 15,
        _ => return None,
    };
    Some(index)
}

pub fn tone(value: &ColorValue) -> Option<Tone> {
    match super::color_value_to_color(value) {
        Color::TrueColor { r, g, b } => Some(Tone::rgb(r, g, b)),
        _ => {
            let ColorValue::Named(name) = value else {
                return None;
            };
            let index = named(name)?;
            Some(Tone {
                rgb: ANSI_RGB[index as usize],
                ansi: Some(index),
            })
        }
    }
}

/// Every color in the theme, for picking by hue.
fn theme_pool(theme: &Theme) -> Vec<Tone> {
    let c = &theme.colors;
    let mut values: Vec<&ColorValue> = vec![
        &c.directory,
        &c.executable,
        &c.symlink,
        &c.user,
        &c.size,
        &c.date,
        &c.permission_read,
        &c.permission_write,
        &c.permission_exec,
        &c.permission_dir,
        &c.file,
        &c.group,
    ];
    let mut extensions: Vec<_> = theme.extensions.colors.iter().collect();
    extensions.sort_by(|a, b| a.0.cmp(b.0));
    values.extend(extensions.into_iter().map(|(_, value)| value));
    let mut special: Vec<_> = theme
        .special_files
        .exact_match
        .iter()
        .chain(&theme.special_files.folders)
        .chain(&theme.special_files.patterns)
        .collect();
    special.sort_by(|a, b| a.0.cmp(b.0));
    values.extend(special.into_iter().map(|(_, value)| value));

    let mut pool: Vec<Tone> = Vec::new();
    for value in values {
        if let Some(tone) = tone(value) {
            if !pool.contains(&tone) {
                pool.push(tone);
            }
        }
    }
    pool
}

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % 360.0;
    d.min(360.0 - d)
}

#[derive(Clone, Debug)]
pub struct Palette {
    /// Normal text.
    pub text: Tone,
    /// Secondary text: paths, hints, counters.
    pub muted: Tone,
    /// Separators and empty bar cells.
    pub faint: Tone,
    /// Selection, the prompt, highlights.
    pub accent: Tone,
    pub success: Tone,
    pub warning: Tone,
    pub danger: Tone,
    /// A second accent for secondary highlights.
    pub info: Tone,
    pool: Vec<Tone>,
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let pool = theme_pool(theme);
        let text = tone(&theme.colors.file).unwrap_or(Tone::rgb(220, 223, 228));
        let light_background = text.luminance() < 0.45;
        let background = if light_background {
            Tone::rgb(250, 250, 250)
        } else {
            Tone::rgb(24, 26, 32)
        };
        let accent = tone(&theme.colors.directory).unwrap_or(Tone::rgb(97, 175, 239));

        // Muted text: the theme's own gray when it is readable, else a blend.
        let muted = [&theme.colors.group, &theme.colors.date, &theme.colors.size]
            .into_iter()
            .filter_map(tone)
            .find(|candidate| {
                let (_, s, _) = candidate.hsl();
                let contrast = (candidate.luminance() - background.luminance()).abs();
                s < 0.35 && contrast > 0.22
            })
            .unwrap_or_else(|| text.mix(background, 0.45));
        let faint = muted.mix(background, 0.55);

        let mut palette = Self {
            text,
            muted,
            faint,
            accent,
            success: Tone::rgb(120, 200, 120),
            warning: Tone::rgb(230, 180, 80),
            danger: Tone::rgb(235, 100, 100),
            info: Tone::rgb(160, 130, 230),
            pool,
        };
        palette.success = palette
            .near(120.0, 45.0, &[])
            .or_else(|| tone(&theme.colors.executable))
            .unwrap_or(palette.success);
        palette.warning = palette.near(40.0, 25.0, &[]).unwrap_or(palette.warning);
        palette.danger = palette.near(355.0, 22.0, &[]).unwrap_or(palette.danger);
        palette.info = palette
            .near(285.0, 45.0, &[accent])
            .or_else(|| tone(&theme.colors.symlink))
            .unwrap_or(palette.info);
        palette
    }

    /// A colorful theme color whose hue is within `spread` degrees of `hue`,
    /// avoiding `taken` when there is another choice.
    pub fn near(&self, hue: f32, spread: f32, taken: &[Tone]) -> Option<Tone> {
        let mut best: Option<(f32, Tone)> = None;
        for tone in &self.pool {
            let (h, s, l) = tone.hsl();
            if s < 0.25 || !(0.2..=0.85).contains(&l) {
                continue;
            }
            let mut distance = hue_distance(h, hue);
            if distance > spread {
                continue;
            }
            if taken.contains(tone) {
                distance += 1000.0;
            }
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, *tone));
            }
        }
        best.map(|(_, tone)| tone)
    }

    /// Picks a theme color for each target hue, keeping them distinct when the
    /// theme allows. Falls back to `fallback` for hues the theme lacks.
    pub fn distinct(&self, wanted: &[(f32, Tone)]) -> Vec<Tone> {
        let mut taken = vec![self.accent];
        let mut out = Vec::with_capacity(wanted.len());
        for (hue, fallback) in wanted {
            let tone = self
                .near(*hue, 35.0, &taken)
                .filter(|tone| !taken[1..].contains(tone))
                .unwrap_or(*fallback);
            taken.push(tone);
            out.push(tone);
        }
        out
    }
}

/// The palette for the theme lla is running with.
pub fn current() -> &'static Palette {
    static PALETTE: OnceLock<Palette> = OnceLock::new();
    PALETTE.get_or_init(|| Palette::from_theme(crate::utils::color::get_theme()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Theme {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../themes")
            .join(format!("{name}.toml"));
        toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn roles_follow_hue_not_slot_names() {
        // The default theme's permission_exec is green; danger must still be red.
        let palette = Palette::from_theme(&load("default"));
        let (h, _, _) = palette.danger.hsl();
        assert!(hue_distance(h, 0.0) < 30.0, "{:?}", palette.danger);
        let (h, _, _) = palette.success.hsl();
        assert!(hue_distance(h, 120.0) < 50.0, "{:?}", palette.success);
    }

    #[test]
    fn every_bundled_theme_gives_readable_roles() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../themes");
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let theme: Theme = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let palette = Palette::from_theme(&theme);
            assert_ne!(palette.text, palette.faint, "{}", path.display());
            let picks =
                palette.distinct(&[(220.0, Tone::rgb(0, 0, 255)), (120.0, Tone::rgb(0, 255, 0))]);
            assert_eq!(picks.len(), 2);
        }
    }

    #[test]
    fn muted_text_stays_readable_on_light_themes() {
        // On a light theme muted text has to be darker than the background,
        // not a light gray.
        let palette = Palette::from_theme(&load("light"));
        assert!(palette.muted.luminance() < 0.75, "{:?}", palette.muted);
        let palette = Palette::from_theme(&load("dark"));
        assert!(palette.muted.luminance() > 0.25, "{:?}", palette.muted);
    }
}
