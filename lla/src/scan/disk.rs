//! Capacity and free space of the volume holding a path.

use std::path::Path;

#[derive(Clone, Debug)]
pub struct Volume {
    pub total: u64,
    /// Space available to the current user.
    pub free: u64,
    pub label: String,
}

impl Volume {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }
}

#[cfg(unix)]
#[allow(clippy::unnecessary_cast, clippy::useless_conversion)]
pub fn volume(path: &Path) -> Option<Volume> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let unit = u64::from(stat.f_frsize).max(1);
    Some(Volume {
        total: u64::from(stat.f_blocks) * unit,
        free: u64::from(stat.f_bavail) * unit,
        label: mount_label(path),
    })
}

#[cfg(target_os = "macos")]
fn mount_label(path: &Path) -> String {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return String::new();
    };
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c_path.as_ptr(), &mut stat) } != 0 {
        return String::new();
    }
    let mount = unsafe { CStr::from_ptr(stat.f_mntonname.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    match mount.as_str() {
        "/" | "/System/Volumes/Data" => "startup disk".to_string(),
        other => other.to_string(),
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn mount_label(path: &Path) -> String {
    let Ok(path) = path.canonicalize() else {
        return String::new();
    };
    let Ok(mounts) = std::fs::read_to_string("/proc/self/mounts") else {
        return String::new();
    };
    mounts
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        // /proc/mounts escapes spaces as \040.
        .map(|mount| mount.replace("\\040", " "))
        .filter(|mount| path.starts_with(mount))
        .max_by_key(|mount| mount.len())
        .unwrap_or_default()
}

#[cfg(windows)]
pub fn volume(path: &Path) -> Option<Volume> {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free) };
    if ok == 0 {
        return None;
    }
    let label = match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                format!("{}:", char::from(letter))
            }
            _ => prefix.as_os_str().to_string_lossy().into_owned(),
        },
        _ => String::new(),
    };
    Some(Volume { total, free, label })
}

#[cfg(not(any(unix, windows)))]
pub fn volume(_path: &Path) -> Option<Volume> {
    None
}
