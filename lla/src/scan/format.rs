use std::path::Path;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Decimal units, matching what Finder, `df -H` and most file managers show.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 999.95 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

const DAY: i64 = 86_400;

/// Compact age for table columns: `today`, `3d`, `5mo`, `2y`.
pub fn age(now: i64, then: i64) -> String {
    if then <= 0 {
        return "-".to_string();
    }
    let days = (now - then).max(0) / DAY;
    match days {
        0 => "today".to_string(),
        1..=59 => format!("{days}d"),
        60..=729 => format!("{}mo", days / 30),
        _ => format!("{}y", days / 365),
    }
}

/// Sentence form: `today`, `yesterday`, `3 weeks ago`.
pub fn long_age(now: i64, then: i64) -> String {
    if then <= 0 {
        return "at an unknown time".to_string();
    }
    let days = (now - then).max(0) / DAY;
    let plural = |n: i64, unit: &str| {
        if n == 1 {
            format!("1 {unit} ago")
        } else {
            format!("{n} {unit}s ago")
        }
    };
    match days {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        2..=13 => plural(days, "day"),
        14..=59 => plural(days / 7, "week"),
        60..=729 => plural(days / 30, "month"),
        _ => plural(days / 365, "year"),
    }
}

pub fn percent(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0%".to_string();
    }
    let value = part as f64 * 100.0 / whole as f64;
    if value > 0.0 && value < 1.0 {
        "<1%".to_string()
    } else {
        format!("{value:.0}%")
    }
}

/// Shows paths under the home folder as `~/...`.
pub fn tilde(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home {
        if path == home {
            return "~".to_string();
        }
        if let Ok(rest) = path.strip_prefix(home) {
            let sep = std::path::MAIN_SEPARATOR;
            return format!("~{sep}{}", rest.display());
        }
    }
    path.display().to_string()
}

pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Cuts `text` to `max` columns, ending with an ellipsis when it had to cut.
pub fn fit(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

/// Like [`fit`] but keeps the end of the text, which is the useful part of a path.
pub fn fit_start(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut kept: Vec<char> = Vec::new();
    let mut used = 0;
    for ch in text.chars().rev() {
        let w = ch.width().unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        kept.push(ch);
        used += w;
    }
    let mut out = String::from("…");
    out.extend(kept.into_iter().rev());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(999), "999 B");
        assert_eq!(size(1_500), "1.5 kB");
        assert_eq!(size(401_300_000_000), "401 GB");
        assert_eq!(size(93_100_000_000), "93.1 GB");
        assert_eq!(size(999_990_000), "1.0 GB");
    }

    #[test]
    fn ages_are_compact() {
        let now = 1_000 * DAY;
        assert_eq!(age(now, now - 10), "today");
        assert_eq!(age(now, now - 3 * DAY), "3d");
        assert_eq!(age(now, now - 90 * DAY), "3mo");
        assert_eq!(age(now, now - 800 * DAY), "2y");
        assert_eq!(long_age(now, now - 21 * DAY), "3 weeks ago");
    }

    #[test]
    fn fitting_respects_width() {
        assert_eq!(fit("node_modules", 6), "node_…");
        assert_eq!(fit_start("/a/b/target", 7), "…target");
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(percent(1, 1000), "<1%");
    }
}
