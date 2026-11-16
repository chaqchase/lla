//! Remembers the finder's index of large folders between runs, so results
//! show up right away while the folder is read again in the background.
//!
//! A cache file is the sorted relative paths, each stored as the length it
//! shares with the previous one plus the rest, gzip compressed.

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAGIC: &[u8] = b"LLA-FINDER-INDEX-1\n";
/// Walks quicker than this are not worth remembering.
pub const MIN_WALK: Duration = Duration::from_millis(300);
/// Cache files kept; the least recently written go first.
const KEEP: usize = 16;
const BATCH: usize = 4096;

/// The cache file for a folder searched with these settings.
pub fn location(
    root: &Path,
    respect_gitignore: bool,
    ignore_patterns: &[String],
) -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("lla").join("finder");
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(MAGIC);
    hasher.update(root.to_string_lossy().as_bytes());
    hasher.update([u8::from(respect_gitignore)]);
    for pattern in ignore_patterns {
        hasher.update(pattern.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    let name: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
    Some(dir.join(format!("{name}.idx")))
}

fn write_varint(out: &mut Vec<u8>, mut value: usize) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Reads a varint at `*at`, moving past it. `None` when the data ends first.
fn read_varint(data: &[u8], at: &mut usize) -> Option<usize> {
    let mut value = 0usize;
    let mut shift = 0;
    loop {
        let byte = *data.get(*at)?;
        *at += 1;
        value |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 63 {
            // Corrupt; report it as data that never completes.
            return None;
        }
    }
}

/// The shared length, the start and the length of the rest of the record at
/// `at`, if the buffer holds all of it.
fn record(data: &[u8], at: usize) -> Option<(usize, usize, usize)> {
    let mut at = at;
    let shared = read_varint(data, &mut at)?;
    let rest = read_varint(data, &mut at)?;
    (at + rest < data.len()).then_some((shared, at, rest))
}

/// Writes `entries` (relative path, is a folder) to `path`, replacing the old
/// file in one step so a reader never sees half of it.
pub fn save(path: &Path, mut entries: Vec<(String, bool)>) -> io::Result<()> {
    entries.sort_unstable();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    let result = (|| {
        let mut out = BufWriter::new(File::create(&temp)?);
        out.write_all(MAGIC)?;
        let mut records = Vec::new();
        let mut previous: &[u8] = &[];
        for (rel, folder) in &entries {
            let rel = rel.as_bytes();
            let shared = previous.iter().zip(rel).take_while(|(a, b)| a == b).count();
            write_varint(&mut records, shared);
            write_varint(&mut records, rel.len() - shared);
            records.extend_from_slice(&rel[shared..]);
            records.push(u8::from(*folder));
            previous = rel;
        }
        let mut gz = GzEncoder::new(out, Compression::fast());
        gz.write_all(&records)?;
        gz.finish()?.flush()
    })();
    match result {
        Ok(()) => {
            fs::rename(&temp, path)?;
            prune(path.parent());
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&temp);
            Err(e)
        }
    }
}

/// Reads a cache file and hands its entries over in batches. Stops early,
/// without an error, when `send` returns false.
pub fn load(path: &Path, mut send: impl FnMut(Vec<(String, bool)>) -> bool) -> io::Result<()> {
    let mut input = BufReader::new(File::open(path)?);
    let mut magic = vec![0u8; MAGIC.len()];
    input.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(io::ErrorKind::InvalidData.into());
    }
    // Decompress a piece at a time so the first batches go out right away.
    let mut gz = GzDecoder::new(input);
    let mut data: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 256 * 1024];
    let mut at = 0;
    let mut previous: Vec<u8> = Vec::new();
    let mut batch = Vec::with_capacity(BATCH);
    let mut ended = false;
    loop {
        // Parse every whole record in the buffer.
        while let Some((shared, start, rest)) = record(&data, at) {
            if shared > previous.len() {
                return Err(io::ErrorKind::InvalidData.into());
            }
            previous.truncate(shared);
            previous.extend_from_slice(&data[start..start + rest]);
            let folder = data[start + rest] == 1;
            at = start + rest + 1;
            let rel =
                String::from_utf8(previous.clone()).map_err(|_| io::ErrorKind::InvalidData)?;
            batch.push((rel, folder));
            if batch.len() == BATCH
                && !send(std::mem::replace(&mut batch, Vec::with_capacity(BATCH)))
            {
                return Ok(());
            }
        }
        if ended {
            if at != data.len() {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            break;
        }
        data.drain(..at);
        at = 0;
        let read = gz.read(&mut chunk)?;
        if read == 0 {
            ended = true;
        }
        data.extend_from_slice(&chunk[..read]);
    }
    if !batch.is_empty() {
        send(batch);
    }
    Ok(())
}

/// Keeps the newest `KEEP` cache files, and removes temporary files left
/// behind when lla exited while saving.
fn prune(dir: Option<&Path>) {
    let Some(Ok(read)) = dir.map(fs::read_dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        let Some(modified) = entry.metadata().ok().and_then(|meta| meta.modified().ok()) else {
            continue;
        };
        let ext = path
            .extension()
            .map(|ext| ext.to_string_lossy().into_owned())
            .unwrap_or_default();
        if ext == "idx" {
            files.push((modified, path));
        } else if ext.starts_with("tmp")
            && modified
                .elapsed()
                .is_ok_and(|age| age > Duration::from_secs(3600))
        {
            let _ = fs::remove_file(path);
        }
    }
    if files.len() <= KEEP {
        return;
    }
    files.sort();
    for (_, path) in &files[..files.len() - KEEP] {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_shares_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.idx");
        let entries: Vec<(String, bool)> = (0..10_000)
            .map(|n| (format!("src/module_{}/file_{n}.rs", n % 50), false))
            .chain([
                ("src".to_string(), true),
                ("über/ünïcode.txt".to_string(), false),
            ])
            .collect();
        save(&path, entries.clone()).unwrap();
        // Shared prefixes plus gzip keep it far below the raw size.
        let raw: usize = entries.iter().map(|(rel, _)| rel.len()).sum();
        assert!(fs::metadata(&path).unwrap().len() < raw as u64 / 5);

        let mut loaded = Vec::new();
        load(&path, |batch| {
            loaded.extend(batch);
            true
        })
        .unwrap();
        let mut expected = entries;
        expected.sort();
        assert_eq!(loaded, expected);
    }

    #[test]
    fn stops_when_asked_and_rejects_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.idx");
        let entries = (0..20_000).map(|n| (format!("f{n}"), false)).collect();
        save(&path, entries).unwrap();
        let mut batches = 0;
        load(&path, |_| {
            batches += 1;
            false
        })
        .unwrap();
        assert_eq!(batches, 1);

        let bad = dir.path().join("bad.idx");
        fs::write(&bad, b"not an index").unwrap();
        assert!(load(&bad, |_| true).is_err());
    }

    #[test]
    fn settings_change_the_location() {
        let root = Path::new("/some/folder");
        let a = location(root, true, &[]);
        assert_eq!(a, location(root, true, &[]));
        assert_ne!(a, location(root, false, &[]));
        assert_ne!(a, location(root, true, &["target".to_string()]));
    }
}
