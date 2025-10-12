use crate::commands::args::Args;
use crate::theme::{color_value_to_color, get_file_color, is_no_color, ColorValue, Theme};
use colored::*;
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

pub fn colorize_file_name(path: &Path) -> ColoredString {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| path.to_str().unwrap_or(""));

    if is_no_color() {
        return if path.is_dir() {
            format!("{}/", name).normal()
        } else {
            name.normal()
        };
    }

    let theme = get_theme();

    if path.is_dir() {
        if let Some(color) = get_file_color(path) {
            format!("{}/", name).color(color).bold()
        } else {
            name.to_string()
                .color(get_color(&theme.colors.directory))
                .bold()
        }
    } else if path.is_symlink() {
        name.color(get_color(&theme.colors.symlink))
            .italic()
            .underline()
    } else if is_executable(path) {
        name.color(get_color(&theme.colors.executable)).bold()
    } else if let Some(color) = get_extension_color(path) {
        name.color(color)
    } else {
        name.color(get_color(&theme.colors.file))
    }
}

pub fn colorize_file_name_with_icon(path: &Path, content: String) -> ColoredString {
    let parts: Vec<&str> = content.split(' ').collect();
    if parts.len() != 2 {
        return if is_no_color() {
            content.normal()
        } else {
            content.color(get_color(&get_theme().colors.file))
        };
    }

    let icon = parts[0];
    let name = parts[1];

    if is_no_color() {
        return if path.is_dir() {
            format!("{} {}", icon, name).normal()
        } else {
            format!("{} {}", icon, name).normal()
        };
    }

    let theme = get_theme();

    if path.is_dir() {
        if let Some(color) = get_file_color(path) {
            format!("{} {}", icon, name).color(color).bold()
        } else {
            format!("{} {}", icon, name)
                .color(get_color(&theme.colors.directory))
                .bold()
        }
    } else if path.is_symlink() {
        format!("{} {}", icon, name)
            .color(get_color(&theme.colors.symlink))
            .italic()
            .underline()
    } else if is_executable(path) {
        format!("{} {}", icon, name)
            .color(get_color(&theme.colors.executable))
            .bold()
    } else if let Some(color) = get_extension_color(path) {
        format!("{} {}", icon, name).color(color)
    } else {
        format!("{} {}", icon, name).color(get_color(&theme.colors.file))
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

use std::fs::Permissions;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Create a Permissions object from mode value (cross-platform)
pub fn permissions_from_mode(mode: u32) -> Permissions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Permissions::from_mode(mode)
    }
    #[cfg(not(unix))]
    {
        // On Windows, create a default permissions object
        // This is a workaround since Windows doesn't have Unix-style permissions
        std::fs::metadata(".").map(|m| m.permissions()).unwrap_or_else(|_| {
            let temp = std::env::temp_dir().join("lla_temp");
            let _ = std::fs::write(&temp, "");
            let perms = std::fs::metadata(&temp).map(|m| m.permissions()).unwrap();
            let _ = std::fs::remove_file(&temp);
            perms
        })
    }
}

pub fn colorize_permissions(permissions: &Permissions, format: Option<&str>) -> String {
    #[cfg(unix)]
    let mode = permissions.mode();
    #[cfg(not(unix))]
    let mode = 0o644u32; // Default permissions for non-Unix systems

    if is_no_color() {
        return format_permissions_no_color(mode, format);
    }

    let theme = get_theme();

    match format.unwrap_or("symbolic") {
        "octal" => format_octal_permissions(mode, &theme),
        "binary" => format_binary_permissions(mode, &theme),
        "verbose" => format_verbose_permissions(mode, &theme),
        "compact" => format_compact_permissions(mode, &theme),
        _ => format_symbolic_permissions(mode, &theme),
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

fn triplet_no_color(mode: u32, _shift: u32) -> String {
    let file_type = if mode & 0o170000 == 0o120000 {
        "l"
    } else if mode & 0o170000 == 0o040000 {
        "d"
    } else {
        "-"
    };
    let read = |shift| {
        if mode >> shift & 0o4u32 != 0u32 {
            "r"
        } else {
            "-"
        }
    };
    let write = |shift| {
        if mode >> shift & 0o2u32 != 0u32 {
            "w"
        } else {
            "-"
        }
    };
    let exec = |shift| {
        if mode >> shift & 0o1u32 != 0u32 {
            "x"
        } else {
            "-"
        }
    };

    format!(
        "{}{}{}{}{}{}{}{}{}{}",
        file_type,
        read(6),
        write(6),
        exec(6),
        read(3),
        write(3),
        exec(3),
        read(0),
        write(0),
        exec(0)
    )
}

pub fn colorize_date(date: &std::time::SystemTime) -> ColoredString {
    let datetime: chrono::DateTime<chrono::Local> = (*date).into();
    let formatted = datetime.format("%b %d %H:%M").to_string();

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

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        if let Ok(metadata) = path.metadata() {
            return metadata.permissions().mode() & 0o111 != 0;
        }
    }
    false
}

fn get_extension_color(path: &Path) -> Option<Color> {
    if is_no_color() {
        return None;
    }
    get_file_color(path)
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
