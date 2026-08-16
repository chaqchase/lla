use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CacheRecord<T> {
    fingerprint: String,
    stored_at_ns: u64,
    last_used_ns: u64,
    value: T,
}

#[derive(Debug, Deserialize, Serialize)]
struct CacheDocument<T> {
    schema_version: u32,
    entries: HashMap<String, CacheRecord<T>>,
}

#[derive(Serialize)]
struct CacheDocumentRef<'a, T> {
    schema_version: u32,
    entries: &'a HashMap<String, CacheRecord<T>>,
}

/// A small, bounded cache for plugin results that survives across CLI runs.
///
/// Callers choose the cache key and invalidation fingerprint, which keeps this
/// primitive useful for files, repositories, API responses, and aggregate
/// directory results without hiding workload-specific correctness decisions.
pub struct PersistentCache<T> {
    path: PathBuf,
    schema_version: u32,
    max_entries: usize,
    entries: HashMap<String, CacheRecord<T>>,
    dirty: bool,
}

impl<T> PersistentCache<T>
where
    T: Clone + DeserializeOwned + Serialize,
{
    pub fn for_plugin(
        plugin_name: &str,
        file_name: &str,
        schema_version: u32,
        max_entries: usize,
    ) -> Self {
        Self::load(
            plugin_data_dir(plugin_name).join(file_name),
            schema_version,
            max_entries,
        )
    }

    pub fn load(path: impl Into<PathBuf>, schema_version: u32, max_entries: usize) -> Self {
        let path = path.into();
        let entries = fs::read_to_string(&path)
            .ok()
            .and_then(|source| toml::from_str::<CacheDocument<T>>(&source).ok())
            .filter(|document| document.schema_version == schema_version)
            .map(|document| document.entries)
            .unwrap_or_default();
        Self {
            path,
            schema_version,
            max_entries: max_entries.max(1),
            entries,
            dirty: false,
        }
    }

    pub fn get(&mut self, key: &str, fingerprint: &str) -> Option<T> {
        let now = now_ns();
        let record = self.entries.get_mut(key)?;
        if record.fingerprint != fingerprint {
            return None;
        }
        if now.saturating_sub(record.last_used_ns) >= duration_ns(USAGE_REFRESH_INTERVAL) {
            record.last_used_ns = now;
            self.dirty = true;
        }
        Some(record.value.clone())
    }

    pub fn get_fresh(&mut self, key: &str, max_age: Duration) -> Option<T> {
        self.get_fresh_matching(key, None, max_age)
    }

    pub fn get_fresh_matching(
        &mut self,
        key: &str,
        fingerprint: Option<&str>,
        max_age: Duration,
    ) -> Option<T> {
        let now = now_ns();
        let record = self.entries.get_mut(key)?;
        if fingerprint.is_some_and(|fingerprint| record.fingerprint != fingerprint) {
            return None;
        }
        if now.saturating_sub(record.stored_at_ns) > duration_ns(max_age) {
            return None;
        }
        if now.saturating_sub(record.last_used_ns) >= duration_ns(USAGE_REFRESH_INTERVAL) {
            record.last_used_ns = now;
            self.dirty = true;
        }
        Some(record.value.clone())
    }

    pub fn insert(&mut self, key: impl Into<String>, fingerprint: impl Into<String>, value: T) {
        let now = now_ns();
        self.entries.insert(
            key.into(),
            CacheRecord {
                fingerprint: fingerprint.into(),
                stored_at_ns: now,
                last_used_ns: now,
                value,
            },
        );
        self.dirty = true;
    }

    pub fn remove(&mut self, key: &str) -> Option<T> {
        let value = self.entries.remove(key).map(|record| record.value);
        self.dirty |= value.is_some();
        value
    }

    pub fn clear(&mut self) {
        if !self.entries.is_empty() {
            self.entries.clear();
            self.dirty = true;
        }
    }

    pub fn persist(&mut self) -> std::io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        self.prune();
        let parent = self.path.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "cache path has no parent")
        })?;
        fs::create_dir_all(parent)?;
        let source = toml::to_string(&CacheDocumentRef {
            schema_version: self.schema_version,
            entries: &self.entries,
        })
        .map_err(std::io::Error::other)?;
        let temporary = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&temporary, source)?;
        let rename = fs::rename(&temporary, &self.path).or_else(|first_error| {
            if self.path.is_file() {
                fs::remove_file(&self.path)?;
                fs::rename(&temporary, &self.path)
            } else {
                Err(first_error)
            }
        });
        match rename {
            Ok(()) => {
                self.dirty = false;
                Ok(())
            }
            Err(error) => {
                let _ = fs::remove_file(temporary);
                Err(error)
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn prune(&mut self) {
        if self.entries.len() <= self.max_entries {
            return;
        }
        let mut entries = self
            .entries
            .iter()
            .map(|(key, record)| (key.clone(), record.last_used_ns))
            .collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(_, used)| std::cmp::Reverse(*used));
        entries.truncate(self.max_entries);
        let keep = entries
            .into_iter()
            .map(|(key, _)| key)
            .collect::<HashSet<_>>();
        self.entries.retain(|key, _| keep.contains(key));
    }
}

pub fn plugin_data_dir(plugin_name: &str) -> PathBuf {
    std::env::var_os("LLA_PLUGIN_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
                .join("lla")
                .join("plugins")
        })
        .join(plugin_name)
}

pub fn canonical_cache_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub fn file_fingerprint(path: &Path) -> std::io::Result<String> {
    let metadata = path.metadata()?;
    let modified_ns = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(format!("{}:{modified_ns}", metadata.len()))
}

pub fn text_fingerprint(value: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_survive_reload_and_fingerprints_invalidate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.toml");
        let mut cache = PersistentCache::<String>::load(&path, 2, 10);
        cache.insert("entry", "first", "value".to_string());
        cache.persist().unwrap();

        let mut reloaded = PersistentCache::<String>::load(&path, 2, 10);
        assert_eq!(reloaded.get("entry", "first").as_deref(), Some("value"));
        assert_eq!(reloaded.get("entry", "different"), None);
        assert!(PersistentCache::<String>::load(&path, 3, 10).is_empty());
    }

    #[test]
    fn pruning_retains_the_requested_bound() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.toml");
        let mut cache = PersistentCache::<u64>::load(&path, 1, 2);
        for value in 0..4 {
            cache.insert(value.to_string(), "same", value);
        }
        cache.persist().unwrap();

        assert_eq!(PersistentCache::<u64>::load(path, 1, 2).len(), 2);
    }
}
