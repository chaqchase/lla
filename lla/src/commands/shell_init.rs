//! `lla shell-init <shell>`: shell functions that change directory, which a
//! program can't do for its parent shell on its own.
//!
//! - `j` jumps to a bookmarked or recent folder (`lla jump`).
//! - `lcd [path]` picks a folder with the fuzzy finder and goes there.

use crate::error::{LlaError, Result};

pub const SHELLS: [&str; 4] = ["bash", "zsh", "fish", "powershell"];

const POSIX: &str = r#"# lla shell integration
# bash: add `eval "$(lla shell-init bash)"` to ~/.bashrc
# zsh:  add `eval "$(lla shell-init zsh)"` to ~/.zshrc

# j: jump to a bookmarked or recent folder
j() {
    local dir
    dir="$(command lla jump)" || return
    if [ -n "$dir" ] && [ -d "$dir" ]; then
        cd -- "$dir" || return
    fi
}

# lcd [path]: pick a folder with the fuzzy finder and go there
lcd() {
    local dir
    dir="$(command lla --dirs "${1:-.}" | head -n 1)" || return
    if [ -n "$dir" ] && [ -d "$dir" ]; then
        cd -- "$dir" || return
    fi
}
"#;

const FISH: &str = r#"# lla shell integration
# add `lla shell-init fish | source` to ~/.config/fish/config.fish

# j: jump to a bookmarked or recent folder
function j
    set -l dir (command lla jump)
    if test -n "$dir"; and test -d "$dir"
        cd -- $dir
    end
end

# lcd [path]: pick a folder with the fuzzy finder and go there
function lcd
    set -l root .
    if test (count $argv) -gt 0
        set root $argv[1]
    end
    set -l picked (command lla --dirs $root)
    if test (count $picked) -gt 0; and test -d "$picked[1]"
        cd -- $picked[1]
    end
end
"#;

const POWERSHELL: &str = r#"# lla shell integration
# add this line to your profile ($PROFILE):
#   Invoke-Expression (& lla shell-init powershell | Out-String)

# j: jump to a bookmarked or recent folder
function j {
    $dir = & lla jump | Select-Object -First 1
    if ($dir -and (Test-Path -LiteralPath $dir -PathType Container)) {
        Set-Location -LiteralPath $dir
    }
}

# lcd [path]: pick a folder with the fuzzy finder and go there
function lcd {
    param([string]$Path = '.')
    $dir = & lla --dirs $Path | Select-Object -First 1
    if ($dir -and (Test-Path -LiteralPath $dir -PathType Container)) {
        Set-Location -LiteralPath $dir
    }
}
"#;

pub fn script(shell: &str) -> Option<&'static str> {
    match shell {
        "bash" | "zsh" => Some(POSIX),
        "fish" => Some(FISH),
        "powershell" | "pwsh" => Some(POWERSHELL),
        _ => None,
    }
}

pub fn run(shell: &str) -> Result<()> {
    let script = script(shell).ok_or_else(|| {
        LlaError::Other(format!(
            "unsupported shell: {shell} (supported: {})",
            SHELLS.join(", ")
        ))
    })?;
    print!("{script}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Checks the syntax with each shell that is installed here, so CI covers
    /// PowerShell on Windows and bash everywhere else.
    #[test]
    fn scripts_parse_in_their_shells() {
        let dir = tempfile::tempdir().unwrap();
        let checks: [(&str, &str, &[&str]); 5] = [
            ("bash", "sh", &["-n"]),
            ("zsh", "sh", &["-n"]),
            ("fish", "fish", &["--no-execute"]),
            (
                "powershell",
                "ps1",
                &[
                    "-NoProfile",
                    "-Command",
                    "$null = [ScriptBlock]::Create((Get-Content -Raw -LiteralPath $env:LLA_SCRIPT))",
                ],
            ),
            (
                "pwsh",
                "ps1",
                &[
                    "-NoProfile",
                    "-Command",
                    "$null = [ScriptBlock]::Create((Get-Content -Raw -LiteralPath $env:LLA_SCRIPT))",
                ],
            ),
        ];
        for (shell, ext, args) in checks {
            // On Windows `bash` may be the WSL launcher, which fails without a
            // Linux install; PowerShell is what matters there.
            if cfg!(windows) && !ext.starts_with("ps") {
                continue;
            }
            let file = dir.path().join(format!("{shell}.{ext}"));
            std::fs::write(&file, script(shell).unwrap()).unwrap();
            let mut command = Command::new(shell);
            command.args(args).env("LLA_SCRIPT", &file);
            if !ext.starts_with("ps") {
                command.arg(&file);
            }
            let output = command.output();
            match output {
                Ok(output) => assert!(
                    output.status.success(),
                    "{shell}: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
                // Not installed on this machine.
                Err(_) => continue,
            }
        }
    }

    #[test]
    fn unknown_shells_are_refused() {
        assert!(script("tcsh").is_none());
        assert!(run("tcsh").is_err());
    }
}
