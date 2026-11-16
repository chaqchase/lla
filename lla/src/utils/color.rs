use crate::commands::args::Args;
use crate::theme::{color_value_to_color, get_file_color_as, is_no_color, ColorValue, Theme};
use colored::*;
use lla_plugin_interface::proto::EntryMetadata;
use std::path::Path;
use std::sync::OnceLock;

static CURRENT_THEME: OnceLock<Theme> = OnceLock::new();

pub fn set_theme(theme: Theme) {
    let _ = CURRENT_THEME.set(theme);
}

pub fn get_theme() -> &'static Theme {
    CURRENT_THEME.get_or_init(Theme::default)
}

fn get_color(color_value: &ColorValue) -> Color {
    color_value_to_color(color_value)
}

/// What kind of entry a name is rendered for. Formatters that already hold
/// entry metadata pass it in so rendering a name never touches the filesystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Directory,
    Symlink,
    Executable,
    File,
}

impl FileKind {
    /// Classifies a path with a single `lstat`. Symlinks are reported as
    /// symlinks even when they point at a directory.
    pub fn of_path(path: &Path) -> Self {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => {
                let file_type = metadata.file_type();
                if file_type.is_symlink() {
                    FileKind::Symlink
                } else if file_type.is_dir() {
                    FileKind::Directory
                } else if is_executable_mode(
                    path,
                    crate::utils::fs_metadata::permission_mode(&metadata),
                ) {
                    FileKind::Executable
                } else {
                    FileKind::File
                }
            }
            Err(_) => FileKind::File,
        }
    }

    pub fn of_entry(path: &Path, metadata: Option<&EntryMetadata>) -> Self {
        match metadata {
            Some(metadata) if metadata.is_symlink => FileKind::Symlink,
            Some(metadata) if metadata.is_dir => FileKind::Directory,
            Some(metadata) if is_executable_mode(path, metadata.permissions) => {
                FileKind::Executable
            }
            Some(_) => FileKind::File,
            None => FileKind::of_path(path),
        }
    }
}

fn display_name(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| path.to_string_lossy())
}

/// Applies the theme style for `kind` to arbitrary text (a name or an icon).
fn style_as(text: String, path: &Path, kind: FileKind) -> ColoredString {
    if is_no_color() {
        return text.normal();
    }
    let theme = get_theme();
    match kind {
        FileKind::Directory => text.color(get_file_color_as(path, true)).bold(),
        FileKind::Symlink => text
            .color(get_color(&theme.colors.symlink))
            .italic()
            .underline(),
        FileKind::Executable => text.color(get_color(&theme.colors.executable)).bold(),
        FileKind::File => text.color(get_file_color_as(path, false)),
    }
}

pub fn colorize_file_name(path: &Path) -> ColoredString {
    colorize_file_name_as(path, FileKind::of_path(path))
}

pub fn colorize_file_name_as(path: &Path, kind: FileKind) -> ColoredString {
    let name = display_name(path);
    // "." and "/" have no file name; don't decorate them as "./" or "//".
    let text = if kind == FileKind::Directory && path.file_name().is_some() {
        format!("{}/", name)
    } else {
        name.into_owned()
    };
    style_as(text, path, kind)
}

/// Colors an icon to match the entry it decorates.
pub fn colorize_icon_as(icon: &str, path: &Path, kind: FileKind) -> ColoredString {
    if is_no_color() {
        return icon.normal();
    }
    match kind {
        // Underlining the icon of a symlink looks like a rendering glitch.
        FileKind::Symlink => icon.color(get_color(&get_theme().colors.symlink)),
        _ => style_as(icon.to_string(), path, kind).bold(),
    }
}

pub fn colorize_size(size: u64) -> ColoredString {
    let formatted = if size < 1024 {
        format!("{}B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1}K", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1}M", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}G", size as f64 / (1024.0 * 1024.0 * 1024.0))
    };

    if is_no_color() {
        formatted.normal()
    } else {
        let theme = get_theme();
        formatted.color(get_color(&theme.colors.size))
    }
}

pub fn colorize_group(group: &str) -> ColoredString {
    if is_no_color() {
        group.normal()
    } else {
        group.color(get_color(&get_theme().colors.group))
    }
}

pub fn colorize_user(user: &str) -> ColoredString {
    if is_no_color() {
        user.normal()
    } else {
        user.color(get_color(&get_theme().colors.user))
    }
}

pub fn colorize_permissions(mode: u32, format: Option<&str>) -> String {
    if is_no_color() {
        return format_permissions_no_color(mode, format);
    }

    let theme = get_theme();

    match format.unwrap_or("symbolic") {
        "octal" => format_octal_permissions(mode, theme),
        "binary" => format_binary_permissions(mode, theme),
        "verbose" => format_verbose_permissions(mode, theme),
        "compact" => format_compact_permissions(mode, theme),
        _ => format_symbolic_permissions(mode, theme),
    }
}

fn format_symbolic_permissions(mode: u32, theme: &Theme) -> String {
    let file_type = if mode & 0o170000 == 0o120000 {
        "l".color(get_color(&theme.colors.permission_dir))
    } else if mode & 0o170000 == 0o040000 {
        "d".color(get_color(&theme.colors.permission_dir))
    } else {
        "-".color(get_color(&theme.colors.permission_none))
    };
    let user = triplet(mode, 6);
    let group = triplet(mode, 3);
    let other = triplet(mode, 0);
    format!("{}{}{}{}", file_type, user, group, other)
}

fn format_octal_permissions(mode: u32, theme: &Theme) -> String {
    let file_type = if mode & 0o170000 == 0o120000 {
        "l".color(get_color(&theme.colors.permission_dir))
    } else if mode & 0o170000 == 0o040000 {
        "d".color(get_color(&theme.colors.permission_dir))
    } else {
        "-".color(get_color(&theme.colors.permission_none))
    };

    let perms = mode & 0o777;
    let user = ((perms >> 6) & 0o7)
        .to_string()
        .color(get_color(&theme.colors.permission_read));
    let group = ((perms >> 3) & 0o7)
        .to_string()
        .color(get_color(&theme.colors.permission_write));
    let other = (perms & 0o7)
        .to_string()
        .color(get_color(&theme.colors.permission_exec));

    format!("{}{}{}{}", file_type, user, group, other)
}

fn format_binary_permissions(mode: u32, theme: &Theme) -> String {
    let file_type = if mode & 0o170000 == 0o120000 {
        "l".color(get_color(&theme.colors.permission_dir))
    } else if mode & 0o170000 == 0o040000 {
        "d".color(get_color(&theme.colors.permission_dir))
    } else {
        "-".color(get_color(&theme.colors.permission_none))
    };

    let perms = mode & 0o777;
    let binary = format!("{:09b}", perms);

    let mut colored_binary = Vec::new();

    for c in binary[0..3].chars() {
        colored_binary.push(if c == '1' {
            "1".color(get_color(&theme.colors.permission_read))
                .to_string()
        } else {
            "0".color(get_color(&theme.colors.permission_none))
                .to_string()
        });
    }

    for c in binary[3..6].chars() {
        colored_binary.push(if c == '1' {
            "1".color(get_color(&theme.colors.permission_write))
                .to_string()
        } else {
            "0".color(get_color(&theme.colors.permission_none))
                .to_string()
        });
    }

    for c in binary[6..9].chars() {
        colored_binary.push(if c == '1' {
            "1".color(get_color(&theme.colors.permission_exec))
                .to_string()
        } else {
            "0".color(get_color(&theme.colors.permission_none))
                .to_string()
        });
    }

    format!("{}{}", file_type, colored_binary.join(""))
}

fn format_verbose_permissions(mode: u32, theme: &Theme) -> String {
    let file_type = if mode & 0o170000 == 0o120000 {
        "type:link".color(get_color(&theme.colors.permission_dir))
    } else if mode & 0o170000 == 0o040000 {
        "type:dir ".color(get_color(&theme.colors.permission_dir))
    } else {
        "type:file".color(get_color(&theme.colors.permission_none))
    };

    let user = format!(
        "owner:{}{}{}",
        if mode & 0o400 != 0 { "r" } else { "-" },
        if mode & 0o200 != 0 { "w" } else { "-" },
        if mode & 0o100 != 0 { "x" } else { "-" }
    )
    .color(get_color(&theme.colors.permission_read));

    let group = format!(
        "group:{}{}{}",
        if mode & 0o40 != 0 { "r" } else { "-" },
        if mode & 0o20 != 0 { "w" } else { "-" },
        if mode & 0o10 != 0 { "x" } else { "-" }
    )
    .color(get_color(&theme.colors.permission_write));

    let other = format!(
        "others:{}{}{}",
        if mode & 0o4 != 0 { "r" } else { "-" },
        if mode & 0o2 != 0 { "w" } else { "-" },
        if mode & 0o1 != 0 { "x" } else { "-" }
    )
    .color(get_color(&theme.colors.permission_exec));

    format!("{} {} {} {}", file_type, user, group, other)
}

fn format_compact_permissions(mode: u32, theme: &Theme) -> String {
    let perms = mode & 0o777;
    format!("{:03o}", perms)
        .color(get_color(&theme.colors.permission_read))
        .to_string()
}

fn format_permissions_no_color(mode: u32, format: Option<&str>) -> String {
    match format.unwrap_or("symbolic") {
        "octal" => {
            let file_type = if mode & 0o170000 == 0o120000 {
                "l"
            } else if mode & 0o170000 == 0o040000 {
                "d"
            } else {
                "-"
            };
            let perms = mode & 0o777;
            format!("{}{:03o}", file_type, perms)
        }
        "binary" => {
            let file_type = if mode & 0o170000 == 0o120000 {
                "l"
            } else if mode & 0o170000 == 0o040000 {
                "d"
            } else {
                "-"
            };
            let perms = mode & 0o777;
            format!("{}{:09b}", file_type, perms)
        }
        "extended" => {
            let file_type = if mode & 0o170000 == 0o120000 {
                "l"
            } else if mode & 0o170000 == 0o040000 {
                "d"
            } else {
                "-"
            };
            let user_x = if mode & 0o100 != 0 {
                if mode & 0o4000 != 0 {
                    "s"
                } else {
                    "x"
                }
            } else {
                if mode & 0o4000 != 0 {
                    "S"
                } else {
                    "-"
                }
            };
            let group_x = if mode & 0o10 != 0 {
                if mode & 0o2000 != 0 {
                    "s"
                } else {
                    "x"
                }
            } else {
                if mode & 0o2000 != 0 {
                    "S"
                } else {
                    "-"
                }
            };
            let other_x = if mode & 0o1 != 0 {
                if mode & 0o1000 != 0 {
                    "t"
                } else {
                    "x"
                }
            } else {
                if mode & 0o1000 != 0 {
                    "T"
                } else {
                    "-"
                }
            };
            format!(
                "{}{}{}{}{}{}{}{}{}{}",
                file_type,
                if mode & 0o400 != 0 { "r" } else { "-" },
                if mode & 0o200 != 0 { "w" } else { "-" },
                user_x,
                if mode & 0o40 != 0 { "r" } else { "-" },
                if mode & 0o20 != 0 { "w" } else { "-" },
                group_x,
                if mode & 0o4 != 0 { "r" } else { "-" },
                if mode & 0o2 != 0 { "w" } else { "-" },
                other_x
            )
        }
        "verbose" => {
            let file_type = if mode & 0o170000 == 0o120000 {
                "type:link"
            } else if mode & 0o170000 == 0o040000 {
                "type:dir"
            } else {
                "type:file"
            };
            format!(
                "{} owner:{}{}{} group:{}{}{} others:{}{}{}",
                file_type,
                if mode & 0o400 != 0 { "r" } else { "-" },
                if mode & 0o200 != 0 { "w" } else { "-" },
                if mode & 0o100 != 0 { "x" } else { "-" },
                if mode & 0o40 != 0 { "r" } else { "-" },
                if mode & 0o20 != 0 { "w" } else { "-" },
                if mode & 0o10 != 0 { "x" } else { "-" },
                if mode & 0o4 != 0 { "r" } else { "-" },
                if mode & 0o2 != 0 { "w" } else { "-" },
                if mode & 0o1 != 0 { "x" } else { "-" }
            )
        }
        "compact" => {
            let perms = mode & 0o777;
            format!("{:03o}", perms)
        }
        "descriptive" => {
            let file_type = if mode & 0o170000 == 0o120000 {
                "symbolic link"
            } else if mode & 0o170000 == 0o040000 {
                "directory"
            } else {
                "regular file"
            };
            format!(
                "{}\n{}\n{}\n{}",
                file_type,
                if mode & 0o400 != 0 {
                    "read"
                } else {
                    "not read"
                },
                if mode & 0o200 != 0 { " & write" } else { "" },
                if mode & 0o100 != 0 { " & execute" } else { "" }
            )
        }
        _ => {
            let file_type = if mode & 0o170000 == 0o120000 {
                "l"
            } else if mode & 0o170000 == 0o040000 {
                "d"
            } else {
                "-"
            };
            let user = triplet_no_color(mode, 6);
            let group = triplet_no_color(mode, 3);
            let other = triplet_no_color(mode, 0);
            format!("{}{}{}{}", file_type, user, group, other)
        }
    }
}

fn triplet(mode: u32, shift: u32) -> String {
    let theme = get_theme();
    let r = if mode >> (shift + 2) & 1u32 != 0 {
        "r".color(get_color(&theme.colors.permission_read))
            .to_string()
    } else {
        "-".color(get_color(&theme.colors.permission_none))
            .to_string()
    };
    let w = if mode >> (shift + 1) & 1u32 != 0 {
        "w".color(get_color(&theme.colors.permission_write))
            .to_string()
    } else {
        "-".color(get_color(&theme.colors.permission_none))
            .to_string()
    };
    let x = if mode >> shift & 1u32 != 0 {
        "x".color(get_color(&theme.colors.permission_exec))
            .to_string()
    } else {
        "-".color(get_color(&theme.colors.permission_none))
            .to_string()
    };
    format!("{}{}{}", r, w, x)
}

fn triplet_no_color(mode: u32, shift: u32) -> String {
    let r = if mode >> (shift + 2) & 1u32 != 0 {
        "r"
    } else {
        "-"
    };
    let w = if mode >> (shift + 1) & 1u32 != 0 {
        "w"
    } else {
        "-"
    };
    let x = if mode >> shift & 1u32 != 0 { "x" } else { "-" };

    format!("{}{}{}", r, w, x)
}

pub fn colorize_date(date: &std::time::SystemTime) -> ColoredString {
    colorize_date_with_format(date, "%b %d %H:%M")
}

pub fn colorize_date_with_format(date: &std::time::SystemTime, format: &str) -> ColoredString {
    let datetime: chrono::DateTime<chrono::Local> = (*date).into();
    let formatted = datetime.format(format).to_string();

    colorize_date_text(formatted)
}

pub fn colorize_date_text(formatted: String) -> ColoredString {
    if is_no_color() {
        formatted.normal()
    } else {
        formatted.color(get_color(&get_theme().colors.date))
    }
}

pub fn colorize_date_relative(date: &std::time::SystemTime) -> ColoredString {
    let dt: chrono::DateTime<chrono::Local> = (*date).into();
    // Use signed difference "dt - now":
    //  - Past times => negative duration => "X ago"
    //  - Future times => positive duration => "in X"
    let delta = dt.signed_duration_since(chrono::Local::now());
    let text = chrono_humanize::HumanTime::from(delta).to_string();

    if is_no_color() {
        text.normal()
    } else {
        text.color(get_color(&get_theme().colors.date))
    }
}

#[cfg(unix)]
fn is_executable_mode(_path: &Path, mode: u32) -> bool {
    mode & 0o111 != 0
}

#[cfg(windows)]
fn is_executable_mode(path: &Path, _mode: u32) -> bool {
    is_executable(path)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "exe" | "com" | "bat" | "cmd" | "ps1"
            )
        })
}

#[cfg(not(any(unix, windows)))]
fn is_executable_mode(_path: &Path, _mode: u32) -> bool {
    false
}

pub struct ColorState {
    pub no_color: bool,
}

impl ColorState {
    pub fn new(args: &Args) -> Self {
        Self {
            no_color: args.no_color,
        }
    }

    pub fn is_enabled(&self) -> bool {
        !self.no_color
    }
}

pub fn colorize_symlink_target(path: &Path) -> ColoredString {
    if is_no_color() {
        return path.to_string_lossy().into_owned().normal();
    }

    let theme = get_theme();
    format!("{}", path.display())
        .color(get_color(&theme.colors.symlink))
        .italic()
        .underline()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_symbolic_permissions_for_directory_are_not_duplicated() {
        assert_eq!(
            format_permissions_no_color(0o040755, Some("symbolic")),
            "drwxr-xr-x"
        );
    }

    #[test]
    fn no_color_symbolic_permissions_for_regular_file_are_not_duplicated() {
        assert_eq!(
            format_permissions_no_color(0o100644, Some("symbolic")),
            "-rw-r--r--"
        );
    }

    #[test]
    fn no_color_non_symbolic_permission_formats_are_unchanged() {
        assert_eq!(format_permissions_no_color(0o100644, Some("octal")), "-644");
        assert_eq!(
            format_permissions_no_color(0o100644, Some("binary")),
            "-110100100"
        );
        assert_eq!(
            format_permissions_no_color(0o100644, Some("compact")),
            "644"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_executable_extensions_are_case_insensitive() {
        assert!(is_executable(Path::new("lla.EXE")));
        assert!(is_executable(Path::new("setup.ps1")));
        assert!(!is_executable(Path::new("notes.txt")));
    }
}
