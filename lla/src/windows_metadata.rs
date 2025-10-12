#[cfg(windows)]
use std::fs::Metadata;
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use windows::core::PWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, PSID};
#[cfg(windows)]
use windows::Win32::Security::{
    GetFileSecurityW, GetSecurityDescriptorOwner, LookupAccountSidW, DACL_SECURITY_INFORMATION,
    GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
};
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{
    GetFileAttributesW, FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_ENCRYPTED, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM,
};

#[cfg(windows)]
pub fn get_windows_permissions(path: &Path) -> u32 {
    use std::os::windows::ffi::OsStrExt;

    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let attrs = GetFileAttributesW(PWSTR(wide_path.as_ptr() as *mut u16));

        // Convert Windows file attributes to Unix-style permissions
        let mut mode = 0o000;

        // Owner permissions
        mode |= 0o400; // Always readable by owner
        if attrs & FILE_ATTRIBUTE_READONLY.0 == 0 {
            mode |= 0o200; // Writable if not read-only
        }
        if attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
            mode |= 0o100; // Executable if directory
        } else if path.extension().and_then(|e| e.to_str()).map_or(false, |ext| {
            matches!(ext.to_lowercase().as_str(), "exe" | "bat" | "cmd" | "com" | "ps1")
        }) {
            mode |= 0o100; // Executable if it has executable extension
        }

        // Group permissions (same as owner on Windows)
        if mode & 0o400 != 0 {
            mode |= 0o040;
        }
        if mode & 0o200 != 0 {
            mode |= 0o020;
        }
        if mode & 0o100 != 0 {
            mode |= 0o010;
        }

        // Other permissions (same as owner on Windows)
        if mode & 0o400 != 0 {
            mode |= 0o004;
        }
        if mode & 0o200 != 0 {
            mode |= 0o002;
        }
        if mode & 0o100 != 0 {
            mode |= 0o001;
        }

        mode
    }
}

#[cfg(windows)]
pub fn get_windows_owner(path: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;

    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut sd_size = 0u32;

        // Get required buffer size
        let _ = GetFileSecurityW(
            PWSTR(wide_path.as_ptr() as *mut u16),
            OWNER_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR::default(),
            0,
            &mut sd_size,
        );

        if sd_size == 0 {
            return None;
        }

        let mut sd_buffer = vec![0u8; sd_size as usize];

        if GetFileSecurityW(
            PWSTR(wide_path.as_ptr() as *mut u16),
            OWNER_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR(sd_buffer.as_mut_ptr() as *mut _),
            sd_size,
            &mut sd_size,
        )
        .is_err()
        {
            return None;
        }

        let mut owner_sid: PSID = PSID::default();
        let mut owner_defaulted = 0i32;

        if GetSecurityDescriptorOwner(
            PSECURITY_DESCRIPTOR(sd_buffer.as_ptr() as *const _),
            &mut owner_sid,
            &mut owner_defaulted,
        )
        .is_err()
        {
            return None;
        }

        // Lookup account name
        let mut name_size = 0u32;
        let mut domain_size = 0u32;
        let mut sid_type = 0u32;

        let _ = LookupAccountSidW(
            None,
            owner_sid,
            PWSTR::null(),
            &mut name_size,
            PWSTR::null(),
            &mut domain_size,
            &mut sid_type,
        );

        if name_size == 0 {
            return None;
        }

        let mut name_buffer = vec![0u16; name_size as usize];
        let mut domain_buffer = vec![0u16; domain_size as usize];

        if LookupAccountSidW(
            None,
            owner_sid,
            PWSTR(name_buffer.as_mut_ptr()),
            &mut name_size,
            PWSTR(domain_buffer.as_mut_ptr()),
            &mut domain_size,
            &mut sid_type,
        )
        .is_ok()
        {
            let name = String::from_utf16_lossy(&name_buffer[..name_size as usize - 1]);
            Some(name)
        } else {
            None
        }
    }
}

#[cfg(windows)]
pub fn get_windows_group(_path: &Path) -> Option<String> {
    // Windows doesn't have a direct equivalent to Unix groups
    // Return None for now
    None
}

#[cfg(windows)]
pub fn get_file_attributes_string(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;

    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let attrs = GetFileAttributesW(PWSTR(wide_path.as_ptr() as *mut u16));

        let mut result = String::new();

        if attrs & FILE_ATTRIBUTE_READONLY.0 != 0 {
            result.push('R');
        }
        if attrs & FILE_ATTRIBUTE_HIDDEN.0 != 0 {
            result.push('H');
        }
        if attrs & FILE_ATTRIBUTE_SYSTEM.0 != 0 {
            result.push('S');
        }
        if attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
            result.push('D');
        }
        if attrs & FILE_ATTRIBUTE_ARCHIVE.0 != 0 {
            result.push('A');
        }
        if attrs & FILE_ATTRIBUTE_COMPRESSED.0 != 0 {
            result.push('C');
        }
        if attrs & FILE_ATTRIBUTE_ENCRYPTED.0 != 0 {
            result.push('E');
        }
        if attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            result.push('L');
        }

        if result.is_empty() {
            result.push('N'); // Normal
        }

        result
    }
}
