use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, response, ActionArguments, Plugin};
use lla_plugin_utils::{
    action_arguments_as_strings, action_infos, decode_decorated_entry, map_decorated_entry,
    ActionRegistry, DecoratedEntry,
};
use parking_lot::RwLock;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs::File,
    io::{BufReader, Read},
    path::Path,
    process::{Command, Stdio},
};

lazy_static! {
    static ref ACTIONS: RwLock<ActionRegistry> = RwLock::new({
        let mut actions = ActionRegistry::new();
        lla_plugin_utils::define_action!(
            actions,
            "inspect",
            "inspect <path>",
            "Print complete media metadata for a file",
            ["lla plugin media_inspector inspect ./photo.jpg"],
            MediaInspectorPlugin::inspect_action
        );
        lla_plugin_utils::define_action!(
            actions,
            "tools",
            "tools",
            "Report optional media metadata tools available on PATH",
            ["lla plugin media_inspector tools"],
            |_| MediaInspectorPlugin::tools_action()
        );
        lla_plugin_utils::define_action!(
            actions,
            "help",
            "help",
            "Show media inspector usage",
            ["lla plugin media_inspector help"],
            |_| MediaInspectorPlugin::help_action()
        );
        actions
    });
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MediaInfo {
    mime_type: String,
    kind: String,
    width: Option<u32>,
    height: Option<u32>,
    duration_ms: Option<u64>,
    codecs: Vec<String>,
    bitrate_bps: Option<u64>,
    exif: Option<String>,
}

pub struct MediaInspectorPlugin;

impl MediaInspectorPlugin {
    fn inspect_path(path: &Path) -> MediaInfo {
        if path.is_dir() {
            return MediaInfo {
                mime_type: "inode/directory".to_string(),
                kind: "directory".to_string(),
                ..MediaInfo::default()
            };
        }
        let mime_type = detect_mime(path);
        let kind = media_kind(&mime_type).to_string();
        let mut info = MediaInfo {
            mime_type,
            kind,
            ..MediaInfo::default()
        };

        if info.kind == "image" {
            let dimensions = builtin_dimensions(path).or_else(|| external_dimensions(path));
            if let Some((width, height)) = dimensions {
                info.width = Some(width);
                info.height = Some(height);
            }
            info.exif = exif_summary(path);
        }
        if matches!(info.kind.as_str(), "audio" | "video") {
            apply_ffprobe(path, &mut info);
        }
        info
    }

    fn decorate(mut entry: DecoratedEntry) -> DecoratedEntry {
        let info = Self::inspect_path(&entry.path);
        entry
            .custom_fields
            .insert("mime_type".to_string(), info.mime_type);
        entry
            .custom_fields
            .insert("media_kind".to_string(), info.kind);
        if let Some(width) = info.width {
            entry
                .custom_fields
                .insert("media_width".to_string(), width.to_string());
        }
        if let Some(height) = info.height {
            entry
                .custom_fields
                .insert("media_height".to_string(), height.to_string());
        }
        if let Some(duration) = info.duration_ms {
            entry
                .custom_fields
                .insert("duration_ms".to_string(), duration.to_string());
        }
        if !info.codecs.is_empty() {
            entry
                .custom_fields
                .insert("codecs".to_string(), info.codecs.join(", "));
        }
        if let Some(bitrate) = info.bitrate_bps {
            entry
                .custom_fields
                .insert("bitrate_bps".to_string(), bitrate.to_string());
        }
        if let Some(exif) = info.exif {
            entry.custom_fields.insert("exif".to_string(), exif);
        }
        entry
    }

    fn format(entry: &DecoratedEntry, format: &str) -> Option<String> {
        let mime = entry.custom_fields.get("mime_type")?;
        let kind = entry.custom_fields.get("media_kind")?;
        let dimensions = entry
            .custom_fields
            .get("media_width")
            .zip(entry.custom_fields.get("media_height"))
            .map(|(width, height)| format!("{width}x{height}"));
        match format {
            "default" => Some(match dimensions {
                Some(dimensions) => format!("[{mime}; {dimensions}]"),
                None => format!("[{mime}]"),
            }),
            "long" => {
                let mut rows = vec![format!("Type: {kind}"), format!("MIME: {mime}")];
                if let Some(dimensions) = dimensions {
                    rows.push(format!("Dimensions: {dimensions}"));
                }
                if let Some(duration) = entry.custom_fields.get("duration_ms") {
                    rows.push(format!("Duration: {} ms", duration));
                }
                if let Some(codecs) = entry.custom_fields.get("codecs") {
                    rows.push(format!("Codecs: {codecs}"));
                }
                if let Some(bitrate) = entry.custom_fields.get("bitrate_bps") {
                    rows.push(format!("Bitrate: {bitrate} bps"));
                }
                if let Some(exif) = entry.custom_fields.get("exif") {
                    rows.push(format!("EXIF: {exif}"));
                }
                Some(rows.join("\n"))
            }
            _ => None,
        }
    }

    fn inspect_action(args: &[String]) -> Result<(), String> {
        let path = args
            .first()
            .map(Path::new)
            .ok_or_else(|| "Usage: inspect <path>".to_string())?;
        if !path.exists() {
            return Err(format!("Path does not exist: {}", path.display()));
        }
        let info = Self::inspect_path(path);
        println!("Path: {}", path.display());
        println!("Kind: {}", info.kind);
        println!("MIME: {}", info.mime_type);
        if let (Some(width), Some(height)) = (info.width, info.height) {
            println!("Dimensions: {width}x{height}");
        }
        if let Some(duration) = info.duration_ms {
            println!("Duration: {duration} ms");
        }
        if !info.codecs.is_empty() {
            println!("Codecs: {}", info.codecs.join(", "));
        }
        if let Some(bitrate) = info.bitrate_bps {
            println!("Bitrate: {bitrate} bps");
        }
        if let Some(exif) = info.exif {
            println!("EXIF: {exif}");
        }
        Ok(())
    }

    fn tools_action() -> Result<(), String> {
        for tool in ["file", "ffprobe", "exiftool", "sips", "identify"] {
            println!(
                "{tool}: {}",
                if tool_available(tool) {
                    "available"
                } else {
                    "not found"
                }
            );
        }
        Ok(())
    }

    fn help_action() -> Result<(), String> {
        println!(
            "media_inspector\n\n  inspect <path>  Print MIME, dimensions, EXIF, duration, codecs, and bitrate\n  tools           Show optional backends\n  help            Show this help\n\nBuilt-in image parsing works without external tools. ffprobe and exiftool add richer audio/video and EXIF data."
        );
        Ok(())
    }
}

fn detect_mime(path: &Path) -> String {
    let builtin = builtin_mime(path);
    if builtin != "application/octet-stream" {
        return builtin.to_string();
    }
    command_output("file", &["--brief", "--mime-type", &path.to_string_lossy()])
        .filter(|value| value.contains('/'))
        .unwrap_or_else(|| builtin.to_string())
}

fn builtin_mime(path: &Path) -> &'static str {
    let mut header = [0u8; 16];
    let read = File::open(path)
        .and_then(|mut file| file.read(&mut header))
        .unwrap_or(0);
    let header = &header[..read];
    if header.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if header.starts_with(b"\xff\xd8\xff") {
        return "image/jpeg";
    }
    if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
        return "image/gif";
    }
    if header.starts_with(b"RIFF") && header.get(8..12) == Some(b"WEBP") {
        return "image/webp";
    }
    if header.starts_with(b"%PDF-") {
        return "application/pdf";
    }
    if header.starts_with(b"PK\x03\x04") {
        return "application/zip";
    }
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "svg" => "image/svg+xml",
        "md" | "markdown" => "text/markdown",
        "txt" | "rs" | "toml" | "json" | "yaml" | "yml" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn media_kind(mime: &str) -> &'static str {
    if mime.starts_with("image/") {
        "image"
    } else if mime.starts_with("audio/") {
        "audio"
    } else if mime.starts_with("video/") {
        "video"
    } else if mime.starts_with("text/") {
        "text"
    } else {
        "other"
    }
}

fn builtin_dimensions(path: &Path) -> Option<(u32, u32)> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(2 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        return Some((
            u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        ));
    }
    if (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) && bytes.len() >= 10 {
        return Some((
            u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32,
            u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32,
        ));
    }
    jpeg_dimensions(&bytes)
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(b"\xff\xd8") {
        return None;
    }
    let mut index = 2usize;
    while index + 8 < bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];
        index += 2;
        if marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = u16::from_be_bytes(bytes.get(index..index + 2)?.try_into().ok()?) as usize;
        if length < 2 || index + length > bytes.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            let height = u16::from_be_bytes(bytes.get(index + 3..index + 5)?.try_into().ok()?);
            let width = u16::from_be_bytes(bytes.get(index + 5..index + 7)?.try_into().ok()?);
            return Some((width as u32, height as u32));
        }
        index += length;
    }
    None
}

fn external_dimensions(path: &Path) -> Option<(u32, u32)> {
    let path = path.to_string_lossy();
    if let Some(output) = command_output("sips", &["-g", "pixelWidth", "-g", "pixelHeight", &path])
    {
        let values = output
            .lines()
            .filter_map(|line| line.split(':').nth(1)?.trim().parse::<u32>().ok())
            .collect::<Vec<_>>();
        if values.len() >= 2 {
            return Some((values[0], values[1]));
        }
    }
    command_output("identify", &["-format", "%w %h", &path]).and_then(|output| {
        let mut values = output
            .split_whitespace()
            .filter_map(|value| value.parse().ok());
        Some((values.next()?, values.next()?))
    })
}

fn exif_summary(path: &Path) -> Option<String> {
    builtin_exif_summary(path).or_else(|| external_exif_summary(path))
}

fn builtin_exif_summary(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    let tags = [
        ("captured", exif::Tag::DateTimeOriginal),
        ("make", exif::Tag::Make),
        ("model", exif::Tag::Model),
        ("orientation", exif::Tag::Orientation),
        ("gps-latitude", exif::Tag::GPSLatitude),
        ("gps-longitude", exif::Tag::GPSLongitude),
    ];
    let values = tags
        .iter()
        .filter_map(|(label, tag)| {
            exif.get_field(*tag, exif::In::PRIMARY)
                .map(|field| format!("{label}={}", field.display_value().with_unit(&exif)))
        })
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join(" | "))
}

fn external_exif_summary(path: &Path) -> Option<String> {
    command_output(
        "exiftool",
        &[
            "-s",
            "-s",
            "-s",
            "-DateTimeOriginal",
            "-Make",
            "-Model",
            "-Orientation",
            "-GPSPosition",
            &path.to_string_lossy(),
        ],
    )
    .map(|output| {
        output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" | ")
    })
    .filter(|output| !output.is_empty())
}

fn apply_ffprobe(path: &Path, info: &mut MediaInfo) {
    let Some(output) = command_output(
        "ffprobe",
        &[
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            &path.to_string_lossy(),
        ],
    ) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(&output) else {
        return;
    };
    apply_ffprobe_value(&value, info);
}

fn apply_ffprobe_value(value: &Value, info: &mut MediaInfo) {
    let format = value.get("format").unwrap_or(&Value::Null);
    info.duration_ms = json_number(format.get("duration"))
        .map(|duration| (duration * 1000.0).round().max(0.0) as u64);
    info.bitrate_bps = json_u64(format.get("bit_rate"));
    let mut codecs = BTreeSet::new();
    if let Some(streams) = value.get("streams").and_then(Value::as_array) {
        for stream in streams {
            if let Some(codec) = stream.get("codec_name").and_then(Value::as_str) {
                codecs.insert(codec.to_string());
            }
            if info.width.is_none() {
                info.width = stream
                    .get("width")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32);
            }
            if info.height.is_none() {
                info.height = stream
                    .get("height")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32);
            }
            if info.bitrate_bps.is_none() {
                info.bitrate_bps = json_u64(stream.get("bit_rate"));
            }
        }
    }
    info.codecs = codecs.into_iter().collect();
}

fn json_number(value: Option<&Value>) -> Option<f64> {
    value.and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn json_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

impl Plugin for MediaInspectorPlugin {
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

lla_plugin_sdk::export_plugin!(MediaInspectorPlugin);

impl Default for MediaInspectorPlugin {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_png_dimensions_without_external_tools() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("pixel.png");
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
        png.extend_from_slice(&320u32.to_be_bytes());
        png.extend_from_slice(&200u32.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]);
        fs::write(&path, png).unwrap();
        assert_eq!(builtin_dimensions(&path), Some((320, 200)));
        assert_eq!(builtin_mime(&path), "image/png");
    }

    #[test]
    fn parses_ffprobe_shape() {
        let value: Value = serde_json::from_str(
            r#"{"format":{"duration":"2.5","bit_rate":"128000"},"streams":[{"codec_name":"h264","width":1920,"height":1080},{"codec_name":"aac"}]}"#,
        )
        .unwrap();
        let mut info = MediaInfo::default();
        apply_ffprobe_value(&value, &mut info);
        assert_eq!(info.duration_ms, Some(2500));
        assert_eq!(info.bitrate_bps, Some(128000));
        assert_eq!(info.width, Some(1920));
        assert_eq!(info.height, Some(1080));
        assert_eq!(info.codecs, vec!["aac", "h264"]);
    }
}
