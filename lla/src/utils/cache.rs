use crate::error::{LlaError, Result};
use chrono::Utc;
use lla_plugin_interface::proto::{DecoratedEntry, EntryMetadata};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub struct ListingCache {
    base_dir: PathBuf,
}

impl ListingCache {
    pub fn new() -> Result<Self> {
        let mut root = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("."));
        root = root.join("lla").join("listings");
        fs::create_dir_all(&root)?;
        Ok(Self { base_dir: root })
    }

    pub fn load(&self, key: &str) -> Result<Option<Vec<DecoratedEntry>>> {
        let path = self.cache_path(key);
        if !path.exists() {
            return Ok(None);
        }

        let bytes = fs::read(&path)?;
        let listing: CachedListing = serde_json::from_slice(&bytes).map_err(|err| {
            LlaError::Other(format!(
                "Failed to read cached listing {}: {}",
                path.display(),
                err
            ))
        })?;

        Ok(Some(
            listing
                .entries
                .into_iter()
                .map(DecoratedEntry::from)
                .collect(),
        ))
    }

    pub fn save(&self, key: &str, summary: &str, entries: &[DecoratedEntry]) -> Result<()> {
        let path = self.cache_path(key);
        let listing = CachedListing {
            context_summary: summary.to_string(),
            generated_at: Utc::now().to_rfc3339(),
            entry_count: entries.len(),
            entries: entries.iter().map(SerializableEntry::from).collect(),
        };

        let data = serde_json::to_vec_pretty(&listing)?;
        fs::write(&path, data)?;
        Ok(())
    }

    fn cache_path(&self, key: &str) -> PathBuf {
        self.base_dir.join(format!("{}.json", key))
    }
}

#[derive(Serialize, Deserialize)]
struct CachedListing {
    context_summary: String,
    generated_at: String,
    entry_count: usize,
    entries: Vec<SerializableEntry>,
}

#[derive(Serialize, Deserialize)]
struct SerializableEntry {
    path: String,
    metadata: Option<SerializableMetadata>,
    custom_fields: HashMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct SerializableMetadata {
    size: u64,
    modified: u64,
    accessed: u64,
    created: u64,
    is_dir: bool,
    is_file: bool,
    is_symlink: bool,
    permissions: u32,
    uid: u32,
    gid: u32,
}

impl From<&DecoratedEntry> for SerializableEntry {
    fn from(entry: &DecoratedEntry) -> Self {
        SerializableEntry {
            path: entry.path.clone(),
            metadata: entry.metadata.as_ref().map(SerializableMetadata::from),
            custom_fields: entry.custom_fields.clone(),
        }
    }
}

impl From<SerializableEntry> for DecoratedEntry {
    fn from(entry: SerializableEntry) -> Self {
        DecoratedEntry {
            path: entry.path,
            metadata: entry.metadata.map(EntryMetadata::from),
            custom_fields: entry.custom_fields,
        }
    }
}

impl From<&EntryMetadata> for SerializableMetadata {
    fn from(meta: &EntryMetadata) -> Self {
        SerializableMetadata {
            size: meta.size,
            modified: meta.modified,
            accessed: meta.accessed,
            created: meta.created,
            is_dir: meta.is_dir,
            is_file: meta.is_file,
            is_symlink: meta.is_symlink,
            permissions: meta.permissions,
            uid: meta.uid,
            gid: meta.gid,
        }
    }
}

impl From<SerializableMetadata> for EntryMetadata {
    fn from(meta: SerializableMetadata) -> Self {
        EntryMetadata {
            size: meta.size,
            modified: meta.modified,
            accessed: meta.accessed,
            created: meta.created,
            is_dir: meta.is_dir,
            is_file: meta.is_file,
            is_symlink: meta.is_symlink,
            permissions: meta.permissions,
            uid: meta.uid,
            gid: meta.gid,
        }
    }
}
