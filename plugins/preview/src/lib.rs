use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, response, ActionArguments, Plugin};
use lla_plugin_utils::{
    action_arguments_as_strings, action_infos, decode_decorated_entry, map_decorated_entry,
    ActionRegistry, DecoratedEntry,
};
use parking_lot::RwLock;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::{Command, Stdio},
    sync::OnceLock,
};

static BAT_AVAILABLE: OnceLock<bool> = OnceLock::new();
static CHAFA_AVAILABLE: OnceLock<bool> = OnceLock::new();
static TAR_AVAILABLE: OnceLock<bool> = OnceLock::new();
static UNZIP_AVAILABLE: OnceLock<bool> = OnceLock::new();

lazy_static! {
    static ref ACTIONS: RwLock<ActionRegistry> = RwLock::new({
        let mut actions = ActionRegistry::new();
        lla_plugin_utils::define_action!(
            actions,
            "show",
            "show <path> [--lines <count>]",
            "Render a terminal preview using bat, chafa, archive tools, or built-in fallbacks",
            [
                "lla plugin preview show README.md",
                "lla plugin preview show src/main.rs -- --lines 80"
            ],
            PreviewPlugin::show_action
        );
        lla_plugin_utils::define_action!(
            actions,
            "backends",
            "backends",
            "Report optional preview backends available on PATH",
            ["lla plugin preview backends"],
            |_| PreviewPlugin::backends_action()
        );
        lla_plugin_utils::define_action!(
            actions,
            "help",
            "help",
            "Show preview usage",
            ["lla plugin preview help"],
            |_| PreviewPlugin::help_action()
        );
        actions
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewKind {
    Text,
    Markdown,
    Image,
    Archive,
    Directory,
    Unsupported,
}

impl PreviewKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Image => "image",
            Self::Archive => "archive",
            Self::Directory => "directory",
            Self::Unsupported => "unsupported",
        }
    }
}

pub struct PreviewPlugin;

impl PreviewPlugin {
    fn decorate(mut entry: DecoratedEntry) -> DecoratedEntry {
        let kind = classify(&entry.path);
        entry
            .custom_fields
            .insert("preview_kind".to_string(), kind.as_str().to_string());
        entry.custom_fields.insert(
            "preview_backend".to_string(),
            preferred_backend(kind).to_string(),
        );
        entry
    }

    fn format(entry: &DecoratedEntry, format: &str) -> Option<String> {
        let kind = entry.custom_fields.get("preview_kind")?;
        let backend = entry.custom_fields.get("preview_backend")?;
        match format {
            "default" => Some(format!("[preview: {kind} via {backend}]")),
            "long" => Some(format!("Preview kind: {kind}\nBackend: {backend}")),
            _ => None,
        }
    }

    fn show_action(args: &[String]) -> Result<(), String> {
        let path = args
            .first()
            .map(Path::new)
            .ok_or_else(|| "Usage: show <path> [--lines <count>]".to_string())?;
        if !path.exists() {
            return Err(format!("Path does not exist: {}", path.display()));
        }
        let lines = args
            .windows(2)
            .find(|pair| pair[0] == "--lines")
            .and_then(|pair| pair[1].parse::<usize>().ok())
            .unwrap_or(120)
            .clamp(1, 5_000);
        match classify(path) {
            PreviewKind::Text => preview_text(path, lines, None),
            PreviewKind::Markdown => preview_text(path, lines, Some("markdown")),
            PreviewKind::Image => preview_image(path),
            PreviewKind::Archive => preview_archive(path),
            PreviewKind::Directory => Err("Preview expects a file, not a directory".to_string()),
            PreviewKind::Unsupported => Err(format!(
                "No safe preview backend is available for '{}'",
                path.display()
            )),
        }
    }

    fn backends_action() -> Result<(), String> {
        for backend in ["bat", "chafa", "tar", "unzip"] {
            println!(
                "{backend}: {}",
                if command_available(backend) {
                    "available"
                } else {
                    "not found (built-in fallback will be used where possible)"
                }
            );
        }
        Ok(())
    }

    fn help_action() -> Result<(), String> {
        println!(
            "preview\n\n  show <path> [--lines N]  Render text, Markdown, image, or archive previews\n  backends                 Report bat/chafa/archive tool availability\n  help                     Show this help\n\nText is bounded to avoid loading huge files. Archives use listing-only commands and are never extracted."
        );
        Ok(())
    }
}

fn classify(path: &Path) -> PreviewKind {
    if path.is_dir() {
        return PreviewKind::Directory;
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "md" | "markdown" | "mdown" | "mkd") {
        return PreviewKind::Markdown;
    }
    if matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "avif" | "heic"
    ) {
        return PreviewKind::Image;
    }
    if matches!(
        extension.as_str(),
        "zip" | "tar" | "tgz" | "gz" | "bz2" | "xz" | "7z"
    ) || path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.ends_with(".tar.gz") || name.ends_with(".tar.xz"))
    {
        return PreviewKind::Archive;
    }
    if matches!(
        extension.as_str(),
        "txt"
            | "rs"
            | "toml"
            | "json"
            | "yaml"
            | "yml"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "py"
            | "go"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "java"
            | "sh"
            | "zsh"
            | "fish"
            | "css"
            | "html"
            | "xml"
            | "sql"
            | "log"
            | "csv"
    ) || extension.is_empty()
    {
        PreviewKind::Text
    } else {
        PreviewKind::Unsupported
    }
}

fn preferred_backend(kind: PreviewKind) -> &'static str {
    match kind {
        PreviewKind::Text | PreviewKind::Markdown if command_available("bat") => "bat",
        PreviewKind::Text | PreviewKind::Markdown => "builtin",
        PreviewKind::Image if command_available("chafa") => "chafa",
        PreviewKind::Image => "metadata",
        PreviewKind::Archive => "listing",
        PreviewKind::Directory => "none",
        PreviewKind::Unsupported => "none",
    }
}

fn preview_text(path: &Path, lines: usize, language: Option<&str>) -> Result<(), String> {
    if command_available("bat") {
        let mut command = Command::new("bat");
        command
            .arg("--color=always")
            .arg("--style=plain")
            .arg("--paging=never")
            .arg("--line-range")
            .arg(format!(":{lines}"));
        if let Some(language) = language {
            command.arg("--language").arg(language);
        }
        let status = command
            .arg(path)
            .stdin(Stdio::null())
            .status()
            .map_err(|error| format!("Failed to run bat: {error}"))?;
        return status
            .success()
            .then_some(())
            .ok_or_else(|| "bat could not preview the file".to_string());
    }
    let preview = builtin_text_preview(path, lines)?;
    print!("{preview}");
    Ok(())
}

fn builtin_text_preview(path: &Path, lines: usize) -> Result<String, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| format!("Failed to open '{}': {error}", path.display()))?
        .take(512 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read '{}': {error}", path.display()))?;
    if bytes.contains(&0) {
        return Err("File appears to be binary".to_string());
    }
    let source = String::from_utf8_lossy(&bytes);
    let mut output = source.lines().take(lines).collect::<Vec<_>>().join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    Ok(output)
}

fn preview_image(path: &Path) -> Result<(), String> {
    if command_available("chafa") {
        let status = Command::new("chafa")
            .args(["--format", "symbols", "--size", "80x40"])
            .arg(path)
            .stdin(Stdio::null())
            .status()
            .map_err(|error| format!("Failed to run chafa: {error}"))?;
        return status
            .success()
            .then_some(())
            .ok_or_else(|| "chafa could not render the image".to_string());
    }
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("Failed to inspect '{}': {error}", path.display()))?;
    println!(
        "Image: {} ({} bytes)\nInstall chafa for an inline terminal rendering.",
        path.display(),
        metadata.len()
    );
    Ok(())
}

fn preview_archive(path: &Path) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let command = if extension.eq_ignore_ascii_case("zip") && command_available("unzip") {
        Some(("unzip", vec!["-l"]))
    } else if (name.ends_with(".tar.gz") || name.ends_with(".tgz")) && command_available("tar") {
        Some(("tar", vec!["-tzf"]))
    } else if name.ends_with(".tar.xz") && command_available("tar") {
        Some(("tar", vec!["-tJf"]))
    } else if extension.eq_ignore_ascii_case("tar") && command_available("tar") {
        Some(("tar", vec!["-tf"]))
    } else {
        None
    };
    if let Some((program, args)) = command {
        let status = Command::new(program)
            .args(args)
            .arg(path)
            .stdin(Stdio::null())
            .status()
            .map_err(|error| format!("Failed to run {program}: {error}"))?;
        return status
            .success()
            .then_some(())
            .ok_or_else(|| format!("{program} could not list the archive"));
    }

    let entries = builtin_archive_entries(path)?;
    println!("Archive: {}", path.display());
    for entry in entries.iter().take(200) {
        println!("  {entry}");
    }
    if entries.len() > 200 {
        println!("  … {} more entries", entries.len() - 200);
    }
    Ok(())
}

fn builtin_archive_entries(path: &Path) -> Result<Vec<String>, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("zip") {
        return zip_entries(path);
    }
    if extension.eq_ignore_ascii_case("tar") {
        return tar_entries(path);
    }
    Err("Install tar, unzip, or 7z to list this compressed archive".to_string())
}

fn zip_entries(path: &Path) -> Result<Vec<String>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| format!("Failed to open archive: {error}"))?
        .take(64 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read archive: {error}"))?;
    let mut entries = Vec::new();
    let mut index = 0usize;
    while index + 46 <= bytes.len() {
        if bytes[index..].starts_with(b"PK\x01\x02") {
            let name_len = u16::from_le_bytes([bytes[index + 28], bytes[index + 29]]) as usize;
            let extra_len = u16::from_le_bytes([bytes[index + 30], bytes[index + 31]]) as usize;
            let comment_len = u16::from_le_bytes([bytes[index + 32], bytes[index + 33]]) as usize;
            let name_start = index + 46;
            let name_end = name_start.saturating_add(name_len);
            if name_end > bytes.len() {
                break;
            }
            entries.push(String::from_utf8_lossy(&bytes[name_start..name_end]).to_string());
            index = name_end
                .saturating_add(extra_len)
                .saturating_add(comment_len);
        } else {
            index += 1;
        }
    }
    Ok(entries)
}

fn tar_entries(path: &Path) -> Result<Vec<String>, String> {
    let mut file = File::open(path).map_err(|error| format!("Failed to open archive: {error}"))?;
    let mut entries = Vec::new();
    loop {
        let mut header = [0u8; 512];
        if file.read_exact(&mut header).is_err() || header.iter().all(|byte| *byte == 0) {
            break;
        }
        let name_end = header[..100]
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(100);
        entries.push(String::from_utf8_lossy(&header[..name_end]).to_string());
        let size_end = header[124..136]
            .iter()
            .position(|byte| *byte == 0 || *byte == b' ')
            .unwrap_or(12);
        let size_text = String::from_utf8_lossy(&header[124..124 + size_end]);
        let size = u64::from_str_radix(size_text.trim(), 8).unwrap_or(0);
        let blocks = size.div_ceil(512);
        file.seek(SeekFrom::Current((blocks * 512) as i64))
            .map_err(|error| format!("Failed to scan archive: {error}"))?;
    }
    Ok(entries)
}

fn command_available(program: &str) -> bool {
    let probe = || {
        Command::new(program)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    };
    match program {
        "bat" => *BAT_AVAILABLE.get_or_init(probe),
        "chafa" => *CHAFA_AVAILABLE.get_or_init(probe),
        "tar" => *TAR_AVAILABLE.get_or_init(probe),
        "unzip" => *UNZIP_AVAILABLE.get_or_init(probe),
        _ => probe(),
    }
}

impl Plugin for PreviewPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        map_decorated_entry(entry, Self::decorate)
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        entries
            .into_iter()
            .map(|entry| map_decorated_entry(entry, Self::decorate))
            .collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| Self::format(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        let arguments = action_arguments_as_strings(arguments);
        response::from_result(ACTIONS.read().handle(&action, &arguments))
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        action_infos(ACTIONS.read().list_actions())
    }
}

lla_plugin_sdk::export_plugin!(PreviewPlugin);

impl Default for PreviewPlugin {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn classifies_requested_preview_types() {
        assert_eq!(classify(Path::new("README.md")), PreviewKind::Markdown);
        assert_eq!(classify(Path::new("photo.png")), PreviewKind::Image);
        assert_eq!(classify(Path::new("source.rs")), PreviewKind::Text);
        assert_eq!(classify(Path::new("bundle.tar")), PreviewKind::Archive);
    }

    #[test]
    fn builtin_text_preview_is_bounded() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("notes.txt");
        fs::write(&path, "one\ntwo\nthree\n").unwrap();
        assert_eq!(builtin_text_preview(&path, 2).unwrap(), "one\ntwo\n");
    }

    #[test]
    fn builtin_tar_preview_lists_entries_without_extracting() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sample.tar");
        let mut archive = vec![0u8; 512 * 4];
        archive[..9].copy_from_slice(b"hello.txt");
        archive[124..136].copy_from_slice(b"00000000005\0");
        archive[156] = b'0';
        archive[512..517].copy_from_slice(b"hello");
        fs::write(&path, archive).unwrap();

        assert_eq!(tar_entries(&path).unwrap(), vec!["hello.txt"]);
    }
}
