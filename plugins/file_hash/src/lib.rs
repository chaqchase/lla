use lla_plugin_sdk::{
    interface::proto, manifest_action_infos, response, value, ActionArguments, DecoratedEntryExt,
    Plugin,
};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CONFIG_FILE: &str = "config.toml";
const CACHE_FILE: &str = "cache.toml";
const USAGE_REFRESH_INTERVAL_NS: u64 = 60 * 60 * 1_000_000_000;
const DEFAULT_CONFIG: &str = r#"[colors]
sha1 = "bright_green"
sha256 = "bright_yellow"
success = "bright_green"
info = "bright_blue"
name = "bright_yellow"

[cache]
enabled = true
max_entries = 10000
"#;

pub struct FileHashPlugin {
    colors: HashMap<String, String>,
    cache: HashCache,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct PluginConfig {
    colors: HashMap<String, String>,
    cache: CacheSettings,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
            cache: CacheSettings::default(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct CacheSettings {
    enabled: bool,
    max_entries: usize,
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 10_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CacheEntry {
    size: u64,
    modified_ns: String,
    sha1: String,
    sha256: String,
    last_used_ns: u64,
}

#[derive(Default, Deserialize, Serialize)]
struct CacheFile {
    entries: HashMap<String, CacheEntry>,
}

#[derive(Serialize)]
struct CacheFileRef<'a> {
    entries: &'a HashMap<String, CacheEntry>,
}

struct HashCache {
    enabled: bool,
    max_entries: usize,
    path: PathBuf,
    entries: HashMap<String, CacheEntry>,
    dirty: bool,
}

impl FileHashPlugin {
    fn calculate_hashes(path: &Path) -> Option<(String, String)> {
        let file = File::open(path).ok()?;
        Self::hash_reader(BufReader::new(file)).ok()
    }

    fn hash_reader(mut reader: impl Read) -> std::io::Result<(String, String)> {
        let mut sha1 = Sha1::new();
        let mut sha256 = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            sha1.update(&buffer[..read]);
            sha256.update(&buffer[..read]);
        }
        Ok((
            format!("{:x}", sha1.finalize()),
            format!("{:x}", sha256.finalize()),
        ))
    }

    fn decorate(&mut self, mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        if entry
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.is_file)
        {
            if let Some(((sha1, sha256), _cache_hit)) =
                self.cache.hashes_for(Path::new(&entry.path))
            {
                entry.insert_field("sha1", value::string(&sha1), sha1);
                entry.insert_field("sha256", value::string(&sha256), sha256);
            }
        }
        entry
    }

    fn paint(&self, key: &str, text: &str) -> String {
        let color = self
            .colors
            .get(key)
            .map(String::as_str)
            .and_then(ansi_color)
            .unwrap_or("\x1b[97m");
        format!("{color}{text}\x1b[0m")
    }

    fn format_hash_info(&self, entry: &proto::DecoratedEntry, format: &str) -> Option<String> {
        if !entry
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.is_file)
        {
            return None;
        }
        let sha1 = entry.custom_fields.get("sha1")?;
        let sha256 = entry.custom_fields.get("sha256")?;
        let (sha1, sha256) = match format {
            "default" => (sha1.get(..8)?, sha256.get(..8)?),
            "long" => (sha1.as_str(), sha256.as_str()),
            _ => return None,
        };
        Some(format!(
            "\n┌─\n│ {} {}\n│ {} {}\n└─\n",
            self.paint("sha1", "SHA1"),
            self.paint("sha1", sha1),
            self.paint("sha256", "SHA256"),
            self.paint("sha256", sha256),
        ))
    }

    fn help(&self) -> String {
        format!(
            "{}\n\n{}\n\n  {}\n    Calculates SHA-1 and SHA-256 hashes for files.\n\n{}\n\n  {}\n    Show this help information.\n\n    Examples:\n      • lla plugin run file_hash help\n\n{}\n\n  {}\n    Show the first 8 characters of each hash.\n\n  {}\n    Show complete hash values.\n",
            self.paint("success", "File Hash Plugin"),
            self.paint("info", "Description"),
            self.paint("name", "File hashing"),
            self.paint("info", "Actions"),
            self.paint("name", "help"),
            self.paint("info", "Formats"),
            self.paint("name", "default"),
            self.paint("name", "long"),
        )
    }
}

impl Plugin for FileHashPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        let entry = self.decorate(entry);
        self.cache.persist();
        entry
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        let entries = entries
            .into_iter()
            .map(|entry| self.decorate(entry))
            .collect();
        self.cache.persist();
        entries
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        self.format_hash_info(&entry, &format)
    }

    fn format_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        format: &str,
    ) -> Vec<Option<String>> {
        entries
            .iter()
            .map(|entry| self.format_hash_info(entry, format))
            .collect()
    }

    fn run_action(&mut self, action: String, _arguments: ActionArguments) -> proto::ActionResponse {
        match action.as_str() {
            "help" => response::text(self.help()),
            _ => response::error(lla_plugin_sdk::ActionError::new(
                "unknown-action",
                format!("unknown action '{action}'"),
            )),
        }
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        manifest_action_infos(include_str!("../plugin.toml"))
    }
}

impl Default for FileHashPlugin {
    fn default() -> Self {
        let data_dir = plugin_data_dir();
        let config_path = data_dir.join(CONFIG_FILE);
        let cache_path = data_dir.join(CACHE_FILE);
        let _ = std::fs::create_dir_all(&data_dir);
        if !config_path.exists() {
            let _ = std::fs::write(&config_path, DEFAULT_CONFIG);
        }
        let config = std::fs::read_to_string(config_path)
            .ok()
            .and_then(|source| toml::from_str::<PluginConfig>(&source).ok())
            .unwrap_or_default();
        Self {
            colors: config.colors,
            cache: HashCache::load(cache_path, config.cache.enabled, config.cache.max_entries),
        }
    }
}

impl HashCache {
    fn load(path: impl Into<PathBuf>, enabled: bool, max_entries: usize) -> Self {
        let path = path.into();
        let entries = enabled
            .then(|| std::fs::read_to_string(&path).ok())
            .flatten()
            .and_then(|source| toml::from_str::<CacheFile>(&source).ok())
            .map(|cache| cache.entries)
            .unwrap_or_default();
        Self {
            enabled,
            max_entries: max_entries.max(1),
            path,
            entries,
            dirty: false,
        }
    }

    fn hashes_for(&mut self, path: &Path) -> Option<((String, String), bool)> {
        if !self.enabled {
            return FileHashPlugin::calculate_hashes(path).map(|hashes| (hashes, false));
        }

        let metadata = path.metadata().ok()?;
        let Some(modified_ns) = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|modified| modified.as_nanos().to_string())
        else {
            return FileHashPlugin::calculate_hashes(path).map(|hashes| (hashes, false));
        };
        let key = path.to_string_lossy().into_owned();
        let last_used_ns = now_ns();
        if let Some(entry) = self.entries.get_mut(&key) {
            if entry.size == metadata.len() && entry.modified_ns == modified_ns {
                if last_used_ns.saturating_sub(entry.last_used_ns) >= USAGE_REFRESH_INTERVAL_NS {
                    entry.last_used_ns = last_used_ns;
                    self.dirty = true;
                }
                return Some(((entry.sha1.clone(), entry.sha256.clone()), true));
            }
        }

        let (sha1, sha256) = FileHashPlugin::calculate_hashes(path)?;
        self.entries.insert(
            key,
            CacheEntry {
                size: metadata.len(),
                modified_ns,
                sha1: sha1.clone(),
                sha256: sha256.clone(),
                last_used_ns,
            },
        );
        self.dirty = true;
        Some(((sha1, sha256), false))
    }

    fn persist(&mut self) {
        if !self.enabled || !self.dirty {
            return;
        }
        self.prune();
        let Some(parent) = self.path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let Ok(source) = toml::to_string(&CacheFileRef {
            entries: &self.entries,
        }) else {
            return;
        };
        let temporary = self.path.with_extension("toml.tmp");
        if std::fs::write(&temporary, source).is_ok()
            && std::fs::rename(&temporary, &self.path).is_ok()
        {
            self.dirty = false;
        } else {
            let _ = std::fs::remove_file(temporary);
        }
    }

    fn prune(&mut self) {
        if self.entries.len() <= self.max_entries {
            return;
        }
        let mut entries = self
            .entries
            .iter()
            .map(|(path, entry)| (path.clone(), entry.last_used_ns))
            .collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(_, last_used)| std::cmp::Reverse(*last_used));
        entries.truncate(self.max_entries);
        let keep = entries
            .into_iter()
            .map(|(path, _)| path)
            .collect::<std::collections::HashSet<_>>();
        self.entries.retain(|path, _| keep.contains(path));
    }
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

fn default_colors() -> HashMap<String, String> {
    [
        ("sha1", "bright_green"),
        ("sha256", "bright_yellow"),
        ("success", "bright_green"),
        ("info", "bright_blue"),
        ("name", "bright_yellow"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect()
}

fn ansi_color(color: &str) -> Option<&'static str> {
    match color {
        "black" => Some("\x1b[30m"),
        "red" => Some("\x1b[31m"),
        "green" => Some("\x1b[32m"),
        "yellow" => Some("\x1b[33m"),
        "blue" => Some("\x1b[34m"),
        "magenta" => Some("\x1b[35m"),
        "cyan" => Some("\x1b[36m"),
        "white" => Some("\x1b[37m"),
        "bright_red" => Some("\x1b[91m"),
        "bright_green" => Some("\x1b[92m"),
        "bright_yellow" => Some("\x1b[93m"),
        "bright_blue" => Some("\x1b[94m"),
        "bright_magenta" => Some("\x1b[95m"),
        "bright_cyan" => Some("\x1b[96m"),
        "bright_white" => Some("\x1b[97m"),
        _ => None,
    }
}

fn plugin_data_dir() -> PathBuf {
    std::env::var_os("LLA_PLUGIN_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
                .join("lla")
                .join("plugins")
        })
        .join("file_hash")
}

lla_plugin_sdk::export_plugin!(FileHashPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn hashes_are_stable_and_streamed() {
        let (sha1, sha256) = FileHashPlugin::hash_reader(Cursor::new(b"abc")).unwrap();
        assert_eq!(sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn cache_hits_and_invalidates_when_file_changes() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.txt");
        let cache_path = directory.path().join("cache.toml");
        std::fs::write(&source, "first").unwrap();
        let mut cache = HashCache::load(&cache_path, true, 10);

        let (first, first_hit) = cache.hashes_for(&source).unwrap();
        assert!(!first_hit);
        cache.persist();

        let mut reloaded = HashCache::load(&cache_path, true, 10);
        let (second, second_hit) = reloaded.hashes_for(&source).unwrap();
        assert!(second_hit);
        assert_eq!(first, second);

        std::fs::write(&source, "different-length").unwrap();
        let (third, third_hit) = reloaded.hashes_for(&source).unwrap();
        assert!(!third_hit);
        assert_ne!(second, third);
    }
}
