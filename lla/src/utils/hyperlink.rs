use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn link_path(path: &Path, label: String) -> String {
    if !ENABLED.load(Ordering::Relaxed) {
        return label;
    }
    if !path.exists() && std::fs::symlink_metadata(path).is_err() {
        return label;
    }
    hyperlink(path, label)
}

fn hyperlink(path: &Path, label: String) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let uri_path = percent_encode(absolute.to_string_lossy().as_bytes());
    format!("\x1b]8;;file://{}\x1b\\{}\x1b]8;;\x1b\\", uri_path, label)
}

fn percent_encode(value: &[u8]) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_osc8_file_uri_and_encodes_unsafe_bytes() {
        let linked = hyperlink(Path::new("a file#1"), "label".to_string());
        assert!(linked.starts_with("\x1b]8;;file:///"));
        assert!(linked.contains("a%20file%231"));
        assert!(linked.ends_with("label\x1b]8;;\x1b\\"));
        assert_eq!(strip_ansi_escapes::strip(&linked).unwrap(), b"label");
    }
}
