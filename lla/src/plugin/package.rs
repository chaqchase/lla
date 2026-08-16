use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Component, Path};
use toml::Value;

/// Verifies a package's optional checksum inventory. A present inventory must
/// cover the runtime entrypoint and, for v3 packages, the manifest.
pub(crate) fn verify_package_checksums(entrypoint: &Path) -> Result<bool, String> {
    let package_dir = entrypoint
        .parent()
        .ok_or_else(|| "plugin entrypoint has no package directory".to_string())?;
    let checksums_path = package_dir.join("checksums.toml");
    if !checksums_path.is_file() {
        return Ok(false);
    }

    let contents = fs::read_to_string(&checksums_path)
        .map_err(|error| format!("failed to read {}: {error}", checksums_path.display()))?;
    let document: Value = toml::from_str(&contents)
        .map_err(|error| format!("failed to parse {}: {error}", checksums_path.display()))?;
    let files = document
        .get("files")
        .and_then(Value::as_table)
        .ok_or_else(|| format!("{} is missing a [files] table", checksums_path.display()))?;

    let entrypoint_name = entrypoint
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "plugin entrypoint has an invalid file name".to_string())?;
    if !files.contains_key(entrypoint_name) {
        return Err(format!(
            "{} does not cover runtime entrypoint '{}'",
            checksums_path.display(),
            entrypoint_name
        ));
    }
    if package_dir.join("plugin.toml").is_file() && !files.contains_key("plugin.toml") {
        return Err(format!(
            "{} does not cover plugin.toml",
            checksums_path.display()
        ));
    }

    for (relative, expected) in files {
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(format!(
                "unsafe checksum path '{}' in {}",
                relative,
                checksums_path.display()
            ));
        }
        let expected = expected
            .as_str()
            .filter(|value| {
                value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
            })
            .ok_or_else(|| format!("invalid SHA-256 checksum for '{relative}'"))?;
        let file = package_dir.join(relative_path);
        let metadata = fs::symlink_metadata(&file)
            .map_err(|error| format!("failed to inspect {}: {error}", file.display()))?;
        if !metadata.file_type().is_file() {
            return Err(format!(
                "plugin package checksum target is not a regular file: {}",
                file.display()
            ));
        }
        let actual = calculate_sha256(&file)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "plugin package checksum mismatch for {}",
                file.display()
            ));
        }
    }

    Ok(true)
}

fn calculate_sha256(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn checksum_inventory_rejects_symlinked_entrypoints() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("actual.so");
        let entrypoint = root.path().join("plugin.so");
        let manifest = root.path().join("plugin.toml");
        fs::write(&target, b"plugin").unwrap();
        fs::write(&manifest, b"manifest").unwrap();
        symlink(&target, &entrypoint).unwrap();
        let entrypoint_hash = calculate_sha256(&target).unwrap();
        let manifest_hash = calculate_sha256(&manifest).unwrap();
        fs::write(
            root.path().join("checksums.toml"),
            format!(
                "[files]\n\"plugin.so\" = \"{entrypoint_hash}\"\n\"plugin.toml\" = \"{manifest_hash}\"\n"
            ),
        )
        .unwrap();

        assert!(verify_package_checksums(&entrypoint).is_err());
    }
}
