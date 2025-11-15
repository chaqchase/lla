use crate::error::{LlaError, Result};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize)]
pub struct NumericBound {
    pub value: u64,
    pub inclusive: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct NumericRange {
    pub min: Option<NumericBound>,
    pub max: Option<NumericBound>,
}

impl NumericRange {
    pub fn matches(&self, value: u64) -> bool {
        if let Some(bound) = &self.min {
            if bound.inclusive {
                if value < bound.value {
                    return false;
                }
            } else if value <= bound.value {
                return false;
            }
        }

        if let Some(bound) = &self.max {
            if bound.inclusive {
                if value > bound.value {
                    return false;
                }
            } else if value >= bound.value {
                return false;
            }
        }

        true
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TimeRange {
    pub earliest: Option<SystemTime>,
    pub latest: Option<SystemTime>,
}

impl TimeRange {
    pub fn matches_epoch_secs(&self, seconds: u64) -> bool {
        let timestamp = UNIX_EPOCH + Duration::from_secs(seconds);
        self.matches_timestamp(timestamp)
    }

    pub fn matches_timestamp(&self, timestamp: SystemTime) -> bool {
        if let Some(start) = self.earliest {
            if timestamp < start {
                return false;
            }
        }

        if let Some(end) = self.latest {
            if timestamp > end {
                return false;
            }
        }

        true
    }
}

pub fn parse_size_range(expr: &str) -> Result<NumericRange> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(LlaError::Parse("Size filter cannot be empty".into()));
    }

    if let Some(rest) = trimmed.strip_prefix(">=") {
        return Ok(NumericRange {
            min: Some(NumericBound {
                value: parse_size_value(rest.trim())?,
                inclusive: true,
            }),
            max: None,
        });
    }

    if let Some(rest) = trimmed.strip_prefix('>') {
        return Ok(NumericRange {
            min: Some(NumericBound {
                value: parse_size_value(rest.trim())?,
                inclusive: false,
            }),
            max: None,
        });
    }

    if let Some(rest) = trimmed.strip_prefix("<=") {
        return Ok(NumericRange {
            min: None,
            max: Some(NumericBound {
                value: parse_size_value(rest.trim())?,
                inclusive: true,
            }),
        });
    }

    if let Some(rest) = trimmed.strip_prefix('<') {
        return Ok(NumericRange {
            min: None,
            max: Some(NumericBound {
                value: parse_size_value(rest.trim())?,
                inclusive: false,
            }),
        });
    }

    if let Some(rest) = trimmed.strip_prefix('=') {
        let value = parse_size_value(rest.trim())?;
        return Ok(NumericRange {
            min: Some(NumericBound {
                value,
                inclusive: true,
            }),
            max: Some(NumericBound {
                value,
                inclusive: true,
            }),
        });
    }

    if let Some(rest) = trimmed.strip_prefix("==") {
        let value = parse_size_value(rest.trim())?;
        return Ok(NumericRange {
            min: Some(NumericBound {
                value,
                inclusive: true,
            }),
            max: Some(NumericBound {
                value,
                inclusive: true,
            }),
        });
    }

    if let Some((start, end)) = trimmed.split_once("..") {
        let start = start.trim();
        let end = end.trim();

        let min = if start.is_empty() {
            None
        } else {
            Some(NumericBound {
                value: parse_size_value(start)?,
                inclusive: true,
            })
        };

        let max = if end.is_empty() {
            None
        } else {
            Some(NumericBound {
                value: parse_size_value(end)?,
                inclusive: true,
            })
        };

        let mut range = NumericRange { min, max };
        normalize_numeric_range(&mut range);
        return Ok(range);
    }

    let value = parse_size_value(trimmed)?;
    Ok(NumericRange {
        min: Some(NumericBound {
            value,
            inclusive: true,
        }),
        max: Some(NumericBound {
            value,
            inclusive: true,
        }),
    })
}

pub fn parse_time_range(expr: &str, now: SystemTime) -> Result<TimeRange> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(LlaError::Parse("Time filter cannot be empty".into()));
    }

    if let Some(rest) = trimmed.strip_prefix(">=") {
        return Ok(TimeRange {
            earliest: Some(parse_time_point(rest.trim(), now)?),
            latest: None,
        });
    }

    if let Some(rest) = trimmed.strip_prefix('>') {
        return Ok(TimeRange {
            earliest: Some(parse_time_point(rest.trim(), now)?),
            latest: None,
        });
    }

    if let Some(rest) = trimmed.strip_prefix("<=") {
        return Ok(TimeRange {
            earliest: None,
            latest: Some(parse_time_point(rest.trim(), now)?),
        });
    }

    if let Some(rest) = trimmed.strip_prefix('<') {
        return Ok(TimeRange {
            earliest: None,
            latest: Some(parse_time_point(rest.trim(), now)?),
        });
    }

    if let Some((start, end)) = trimmed.split_once("..") {
        let start = start.trim();
        let end = end.trim();

        let earliest = if start.is_empty() {
            None
        } else {
            Some(parse_time_point(start, now)?)
        };

        let latest = if end.is_empty() {
            None
        } else {
            Some(parse_time_point(end, now)?)
        };

        let mut range = TimeRange { earliest, latest };
        normalize_time_range(&mut range);
        return Ok(range);
    }

    Ok(TimeRange {
        earliest: Some(parse_time_point(trimmed, now)?),
        latest: None,
    })
}

fn normalize_numeric_range(range: &mut NumericRange) {
    if let (Some(min), Some(max)) = (range.min.clone(), range.max.clone()) {
        if min.value > max.value {
            range.min = Some(max);
            range.max = Some(min);
        }
    }
}

fn normalize_time_range(range: &mut TimeRange) {
    if let (Some(start), Some(end)) = (range.earliest.clone(), range.latest.clone()) {
        if start > end {
            range.earliest = Some(end);
            range.latest = Some(start);
        }
    }
}

fn parse_size_value(token: &str) -> Result<u64> {
    let cleaned = token.replace('_', "");
    let mut num_part = String::new();
    let mut suffix_part = String::new();
    for ch in cleaned.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            if !suffix_part.is_empty() {
                suffix_part.push(ch);
            } else {
                num_part.push(ch);
            }
        } else if ch.is_ascii_alphabetic() {
            suffix_part.push(ch);
        } else {
            return Err(LlaError::Parse(format!(
                "Unexpected character '{}' in size literal '{}'",
                ch, token
            )));
        }
    }

    if num_part.is_empty() {
        return Err(LlaError::Parse(format!(
            "Missing numeric value in size literal '{}'",
            token
        )));
    }

    let value: f64 = num_part.parse().map_err(|_| {
        LlaError::Parse(format!(
            "Invalid numeric portion in size literal '{}'",
            token
        ))
    })?;

    let multiplier = size_multiplier(suffix_part.trim())?;
    let bytes = value * multiplier as f64;
    if bytes.is_sign_negative() {
        return Err(LlaError::Parse(format!(
            "Size literal '{}' must be positive",
            token
        )));
    }

    Ok(bytes.round() as u64)
}

fn size_multiplier(unit: &str) -> Result<u64> {
    if unit.is_empty() || unit.eq_ignore_ascii_case("b") {
        return Ok(1);
    }

    let normalized = unit.to_ascii_lowercase();
    let multiplier = match normalized.as_str() {
        "k" | "kb" => 1024u64,
        "m" | "mb" => 1024u64.pow(2),
        "g" | "gb" => 1024u64.pow(3),
        "t" | "tb" => 1024u64.pow(4),
        "p" | "pb" => 1024u64.pow(5),
        "ki" | "kib" => 1024u64,
        "mi" | "mib" => 1024u64.pow(2),
        "gi" | "gib" => 1024u64.pow(3),
        "ti" | "tib" => 1024u64.pow(4),
        "pi" | "pib" => 1024u64.pow(5),
        _ => {
            return Err(LlaError::Parse(format!(
                "Unknown size suffix '{}' (use B, K, M, G, T, P, KiB, MiB, ...)",
                unit
            )))
        }
    };
    Ok(multiplier)
}

fn parse_time_point(input: &str, now: SystemTime) -> Result<SystemTime> {
    let trimmed = input.trim();
    if let Ok(abs) = parse_absolute_datetime(trimmed) {
        return Ok(abs);
    }

    if let Some(duration) = parse_duration(trimmed)? {
        return now.checked_sub(duration).ok_or_else(|| {
            LlaError::Parse(format!("Relative time '{}' exceeds supported range", input))
        });
    }

    Err(LlaError::Parse(format!(
        "Unable to parse date/time '{}'",
        input
    )))
}

fn parse_duration(token: &str) -> Result<Option<Duration>> {
    if token.is_empty() {
        return Ok(None);
    }

    let mut num_part = String::new();
    let mut unit_part = String::new();
    for ch in token.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            if !unit_part.is_empty() {
                return Err(LlaError::Parse(format!(
                    "Invalid duration literal '{}'",
                    token
                )));
            }
            num_part.push(ch);
        } else if ch.is_ascii_alphabetic() {
            unit_part.push(ch);
        } else {
            return Err(LlaError::Parse(format!(
                "Invalid duration literal '{}'",
                token
            )));
        }
    }

    if num_part.is_empty() || unit_part.is_empty() {
        return Ok(None);
    }

    let value: f64 = num_part
        .parse()
        .map_err(|_| LlaError::Parse(format!("Invalid numeric portion in duration '{}'", token)))?;
    let unit = unit_part.to_ascii_lowercase();
    let seconds_per_unit = match unit.as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
        "d" | "day" | "days" => 86400.0,
        "w" | "wk" | "wks" | "week" | "weeks" => 604800.0,
        "mo" | "month" | "months" => 2_592_000.0, // 30 days
        "y" | "yr" | "yrs" | "year" | "years" => 31_536_000.0, // 365 days
        _ => {
            return Err(LlaError::Parse(format!(
                "Unknown duration unit '{}' in '{}'",
                unit, token
            )))
        }
    };

    let seconds = value * seconds_per_unit;
    if seconds.is_sign_negative() {
        return Err(LlaError::Parse(format!(
            "Duration '{}' must be positive",
            token
        )));
    }

    Ok(Some(Duration::from_secs_f64(seconds)))
}

fn parse_absolute_datetime(input: &str) -> Result<SystemTime> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(input) {
        return datetime_to_system_time(dt.with_timezone(&Utc));
    }

    let naive_formats = [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ];

    for fmt in naive_formats {
        if let Ok(naive) = NaiveDateTime::parse_from_str(input, fmt) {
            let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
            return datetime_to_system_time(dt);
        }
    }

    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        if let Some(naive) = date.and_hms_opt(0, 0, 0) {
            let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
            return datetime_to_system_time(dt);
        }
    }

    Err(LlaError::Parse(format!(
        "Unable to parse date/time '{}'. Use ISO8601 or relative durations (e.g., 2023-01-01, 2023-01-01T12:00, 7d, <3h).",
        input
    )))
}

fn datetime_to_system_time(dt: DateTime<Utc>) -> Result<SystemTime> {
    if dt.timestamp() < 0 {
        return Err(LlaError::Parse(
            "Dates before 1970-01-01 are not supported".to_string(),
        ));
    }

    let secs = dt.timestamp() as u64;
    let nanos = dt.timestamp_subsec_nanos() as u64;
    Ok(UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_nanos(nanos))
}
