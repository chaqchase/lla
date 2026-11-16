//! Searching inside files for the finder's text mode.

use regex::bytes::{Regex, RegexBuilder};
use std::io::Read;
use std::path::Path;

/// Files bigger than this are skipped; they are rarely what you are looking
/// for interactively and would stall the search.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const BINARY_SNIFF_BYTES: usize = 8 * 1024;
/// Shown characters per matching line.
const MAX_LINE_CHARS: usize = 300;
pub const MAX_HITS_PER_FILE: usize = 20;

/// A matching line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 1-based line number.
    pub line: u32,
    /// The line with leading whitespace trimmed, safe to print.
    pub text: String,
    /// Char ranges of the matches within `text`.
    pub ranges: Vec<(usize, usize)>,
}

/// The query as a literal, case-insensitive unless it contains uppercase.
pub fn pattern(query: &str) -> Option<Regex> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    RegexBuilder::new(&regex::escape(query))
        .case_insensitive(!query.chars().any(char::is_uppercase))
        .build()
        .ok()
}

/// The matching lines of one file, at most `max_hits`. Binary, unreadable and
/// very large files give nothing.
pub fn search_file(path: &Path, pattern: &Regex, max_hits: usize) -> Vec<Hit> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    if file
        .metadata()
        .map_or(true, |meta| meta.len() > MAX_FILE_BYTES)
    {
        return Vec::new();
    }
    let mut buffer = Vec::new();
    if file.take(MAX_FILE_BYTES).read_to_end(&mut buffer).is_err() {
        return Vec::new();
    }
    search_bytes(&buffer, pattern, max_hits)
}

pub fn search_bytes(buffer: &[u8], pattern: &Regex, max_hits: usize) -> Vec<Hit> {
    if buffer[..buffer.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
        return Vec::new();
    }
    let mut hits: Vec<Hit> = Vec::new();
    // Line number of `counted_to`, so newlines are only counted once.
    let mut counted_to = 0;
    let mut line_number = 1u32;
    let mut last_line_start = usize::MAX;
    for found in pattern.find_iter(buffer) {
        let start = memchr::memrchr(b'\n', &buffer[..found.start()]).map_or(0, |at| at + 1);
        if start == last_line_start {
            // Another match on a line already reported.
            continue;
        }
        if hits.len() == max_hits {
            break;
        }
        last_line_start = start;
        line_number += memchr::memchr_iter(b'\n', &buffer[counted_to..start]).count() as u32;
        counted_to = start;
        let end = memchr::memchr(b'\n', &buffer[start..]).map_or(buffer.len(), |at| start + at);
        hits.push(hit(&buffer[start..end], line_number, pattern));
    }
    hits
}

fn hit(line: &[u8], number: u32, pattern: &Regex) -> Hit {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let indent = line
        .iter()
        .take_while(|b| **b == b' ' || **b == b'\t')
        .count();
    let line = &line[indent..];
    let matches: Vec<(usize, usize)> = pattern
        .find_iter(line)
        .map(|found| (found.start(), found.end()))
        .collect();

    // Build the shown text char by char, mapping byte offsets to char offsets
    // as we go and dropping control characters.
    let text = String::from_utf8_lossy(line);
    let mut out = String::new();
    let mut ranges = Vec::new();
    let mut chars = 0;
    let mut open: Option<usize> = None;
    let mut byte = 0;
    for c in text.chars() {
        if chars == MAX_LINE_CHARS {
            break;
        }
        if open.is_none() && matches.iter().any(|(start, _)| *start == byte) {
            open = Some(chars);
        }
        if let Some(from) = open {
            if matches.iter().any(|(_, end)| *end == byte) {
                ranges.push((from, chars));
                open = matches
                    .iter()
                    .any(|(start, _)| *start == byte)
                    .then_some(chars);
            }
        }
        byte += c.len_utf8();
        match c {
            '\t' => {
                out.push_str("    ");
                chars += 4;
            }
            c if c.is_control() => {}
            c => {
                out.push(c);
                chars += 1;
            }
        }
    }
    if let Some(from) = open {
        ranges.push((from, chars));
    }
    Hit {
        line: number,
        text: out,
        ranges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_lines_with_numbers_and_ranges() {
        let text = b"fn main() {\n    let main_value = 1;\n}\n\nmain();\n";
        let hits = search_bytes(text, &pattern("main").unwrap(), 10);
        assert_eq!(
            hits.iter().map(|hit| hit.line).collect::<Vec<_>>(),
            vec![1, 2, 5]
        );
        assert_eq!(hits[1].text, "let main_value = 1;");
        assert_eq!(hits[1].ranges, vec![(4, 8)]);
        assert_eq!(hits[0].ranges, vec![(3, 7)]);
    }

    #[test]
    fn smart_case_and_limits() {
        let text = b"Main\nmain\nMAIN\n";
        assert_eq!(search_bytes(text, &pattern("main").unwrap(), 10).len(), 3);
        assert_eq!(search_bytes(text, &pattern("Main").unwrap(), 10).len(), 1);
        assert_eq!(search_bytes(text, &pattern("main").unwrap(), 2).len(), 2);
        assert!(search_bytes(b"main\0", &pattern("main").unwrap(), 10).is_empty());
        assert!(pattern("   ").is_none());
    }

    #[test]
    fn several_matches_on_a_line_and_windows_endings() {
        let hits = search_bytes(b"a.b a.b\r\nx\r\n", &pattern("a.b").unwrap(), 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "a.b a.b");
        assert_eq!(hits[0].ranges, vec![(0, 3), (4, 7)]);
        // The dot is literal.
        assert!(search_bytes(b"axb", &pattern("a.b").unwrap(), 10).is_empty());
    }
}
