use lla_plugin_interface::proto::EntryMetadata;
use once_cell::sync::Lazy;
use std::fs::Metadata;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
struct MountInfo {
    source: String,
    point: PathBuf,
    filesystem: String,
}

static MOUNTS: Lazy<Vec<MountInfo>> = Lazy::new(load_mounts);

pub fn from_metadata(metadata: &Metadata) -> EntryMetadata {
    EntryMetadata {
        size: metadata.len(),
        modified: timestamp(metadata.modified()),
        accessed: timestamp(metadata.accessed()),
        created: timestamp(metadata.created()),
        is_dir: metadata.is_dir(),
        is_file: metadata.is_file(),
        is_symlink: metadata.is_symlink(),
        permissions: metadata.mode(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        inode: metadata.ino(),
        hard_links: metadata.nlink(),
        allocated_size: metadata.blocks().saturating_mul(512),
        ..EntryMetadata::default()
    }
}

fn timestamp(value: std::io::Result<std::time::SystemTime>) -> u64 {
    value
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Populate filesystem details that need calls beyond `stat(2)`.
pub fn enrich(path: &Path, metadata: &mut EntryMetadata, extended: bool, mounts: bool) {
    if extended {
        read_extended_metadata(path, metadata);
    }

    if mounts {
        if let Some(info) = mount_for(path) {
            metadata.mount_point = info.point.to_string_lossy().into_owned();
            metadata.mount_source = info.source.clone();
            metadata.filesystem = info.filesystem.clone();
        }
    }
}

fn read_extended_metadata(path: &Path, metadata: &mut EntryMetadata) {
    metadata.has_acl = platform_has_acl(path);

    let Ok(names) = xattr::list(path) else {
        return;
    };

    for name in names {
        let name_display = name.to_string_lossy().into_owned();
        let value = xattr::get(path, &name).ok().flatten();
        let size = value.as_ref().map_or(0, Vec::len) as u64;

        if is_acl_attribute(&name_display) {
            metadata.has_acl = true;
        }
        if name_display == "security.selinux" {
            if let Some(value) = value.as_deref() {
                metadata.security_context = String::from_utf8_lossy(value)
                    .trim_end_matches('\0')
                    .to_string();
            }
        }

        metadata.xattrs.insert(name_display, size);
    }
}

#[cfg(target_os = "macos")]
fn platform_has_acl(path: &Path) -> bool {
    use std::ffi::{c_char, c_int, c_void, CString};

    type Acl = *mut c_void;
    const ACL_TYPE_EXTENDED: c_int = 0x0000_0100;

    extern "C" {
        fn acl_get_file(path: *const c_char, acl_type: c_int) -> Acl;
        fn acl_get_link_np(path: *const c_char, acl_type: c_int) -> Acl;
        fn acl_free(object: *mut c_void) -> c_int;
    }

    let is_symlink = std::fs::symlink_metadata(path)
        .map(|metadata| metadata.is_symlink())
        .unwrap_or(false);
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let acl = unsafe {
        if is_symlink {
            acl_get_link_np(c_path.as_ptr(), ACL_TYPE_EXTENDED)
        } else {
            acl_get_file(c_path.as_ptr(), ACL_TYPE_EXTENDED)
        }
    };
    if acl.is_null() {
        return false;
    }

    unsafe {
        acl_free(acl);
    }
    true
}

#[cfg(not(target_os = "macos"))]
fn platform_has_acl(_path: &Path) -> bool {
    false
}

fn is_acl_attribute(name: &str) -> bool {
    matches!(
        name,
        "system.posix_acl_access"
            | "system.posix_acl_default"
            | "com.apple.acl.text"
            | "com.apple.system.Security"
    )
}

pub fn format_xattrs(metadata: &EntryMetadata) -> String {
    if metadata.xattrs.is_empty() {
        return "-".to_string();
    }

    let mut attributes: Vec<_> = metadata.xattrs.iter().collect();
    attributes.sort_unstable_by(|left, right| left.0.cmp(right.0));
    attributes
        .into_iter()
        .map(|(name, size)| format!("{}={}B", name, size))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn format_context(metadata: &EntryMetadata) -> String {
    if !metadata.security_context.is_empty() {
        metadata.security_context.clone()
    } else if metadata.has_acl {
        "acl".to_string()
    } else {
        "-".to_string()
    }
}

pub fn format_mount(metadata: &EntryMetadata) -> String {
    if metadata.mount_point.is_empty() {
        return "-".to_string();
    }

    match (
        metadata.mount_source.is_empty(),
        metadata.filesystem.is_empty(),
    ) {
        (false, false) => format!(
            "{} on {} ({})",
            metadata.mount_source, metadata.mount_point, metadata.filesystem
        ),
        (false, true) => format!("{} on {}", metadata.mount_source, metadata.mount_point),
        (true, false) => format!("{} ({})", metadata.mount_point, metadata.filesystem),
        (true, true) => metadata.mount_point.clone(),
    }
}

fn mount_for(path: &Path) -> Option<&'static MountInfo> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };

    MOUNTS
        .iter()
        .filter(|mount| absolute.starts_with(&mount.point))
        .max_by_key(|mount| mount.point.as_os_str().as_bytes().len())
}

fn load_mounts() -> Vec<MountInfo> {
    #[cfg(target_os = "linux")]
    if let Ok(contents) = std::fs::read_to_string("/proc/self/mountinfo") {
        let mounts = parse_linux_mountinfo(&contents);
        if !mounts.is_empty() {
            return mounts;
        }
    }

    Command::new("mount")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_mount_output(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_mountinfo(contents: &str) -> Vec<MountInfo> {
    contents
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let left: Vec<_> = left.split_whitespace().collect();
            let right: Vec<_> = right.split_whitespace().collect();
            Some(MountInfo {
                source: unescape_mount_field(right.get(1)?),
                point: PathBuf::from(unescape_mount_field(left.get(4)?)),
                filesystem: (*right.first()?).to_string(),
            })
        })
        .collect()
}

fn parse_mount_output(contents: &str) -> Vec<MountInfo> {
    contents
        .lines()
        .filter_map(|line| {
            let (source, rest) = line.split_once(" on ")?;
            let (point, details) = if let Some((point, details)) = rest.rsplit_once(" (") {
                (point, details.trim_end_matches(')'))
            } else if let Some((point, details)) = rest.split_once(" type ") {
                (point, details)
            } else {
                return None;
            };
            let filesystem = details
                .split([',', ' '])
                .find(|part| !part.is_empty())
                .unwrap_or("");
            Some(MountInfo {
                source: unescape_mount_field(source),
                point: PathBuf::from(unescape_mount_field(point)),
                filesystem: filesystem.to_string(),
            })
        })
        .collect()
}

fn unescape_mount_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_linux_mountinfo_and_escapes() {
        let mounts = parse_linux_mountinfo(
            "36 25 0:31 / / rw,relatime - overlay overlay rw\n42 36 0:9 / /media/My\\040Drive rw - vfat /dev/sda1 rw\n",
        );
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[1].point, PathBuf::from("/media/My Drive"));
        assert_eq!(mounts[1].source, "/dev/sda1");
        assert_eq!(mounts[1].filesystem, "vfat");
    }

    #[test]
    fn parses_bsd_mount_output() {
        let mounts = parse_mount_output("/dev/disk3s1s1 on / (apfs, sealed, local)\n");
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].point, PathBuf::from("/"));
        assert_eq!(mounts[0].filesystem, "apfs");
    }

    #[test]
    fn captures_inode_links_allocated_size_and_xattrs() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("file");
        let link = directory.path().join("hard-link");
        fs::write(&file, vec![b'x'; 1024]).unwrap();
        fs::hard_link(&file, &link).unwrap();
        let attribute = if cfg!(target_os = "macos") {
            "com.lla.test"
        } else {
            "user.lla.test"
        };
        xattr::set(&file, attribute, b"value").unwrap();

        let mut metadata = from_metadata(&fs::metadata(&file).unwrap());
        enrich(&file, &mut metadata, true, false);

        assert_ne!(metadata.inode, 0);
        assert!(metadata.hard_links >= 2);
        assert_eq!(metadata.xattrs.get(attribute), Some(&5));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_macos_extended_acl() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("acl-file");
        fs::write(&file, []).unwrap();
        let status = Command::new("chmod")
            .args(["+a", "everyone deny delete"])
            .arg(&file)
            .status()
            .unwrap();
        assert!(status.success());

        let mut metadata = from_metadata(&fs::metadata(&file).unwrap());
        enrich(&file, &mut metadata, true, false);
        assert!(metadata.has_acl);
        assert_eq!(format_context(&metadata), "acl");
    }
}
