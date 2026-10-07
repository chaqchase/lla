use std::fs;
use std::process::{Command, Output};
use tempfile::TempDir;

fn completion(home: &TempDir, shell: &str, options: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lla"))
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("APPDATA", home.path().join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.path().join("AppData/Local"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("NO_COLOR", "1")
        .current_dir(home.path())
        .args(["completion", shell])
        .args(options)
        .output()
        .expect("run completion command")
}

#[test]
fn completions_default_to_stdout_without_installing_files() {
    for shell in ["bash", "fish", "zsh", "powershell", "elvish"] {
        let home = TempDir::new().unwrap();
        let output = completion(&home, shell, &[]);
        assert!(output.status.success(), "{shell}: {:?}", output);
        let script = String::from_utf8(output.stdout).unwrap();
        assert!(script.contains("lla"), "{shell}: missing completion script");
        assert!(!script.contains("✓"), "{shell}: status message in script");
        assert!(output.stderr.is_empty(), "{shell}: {:?}", output.stderr);
        for path in [
            ".local/share/bash-completion/completions/lla",
            ".config/fish/completions/lla.fish",
            ".zsh/completions/_lla",
            "Documents/WindowsPowerShell/lla.ps1",
            ".elvish/lib/lla.elv",
        ] {
            assert!(!home.path().join(path).exists(), "{shell}: created {path}");
        }
    }
}

#[test]
fn explicit_paths_write_the_same_script_as_stdout() {
    for shell in ["bash", "fish", "zsh", "powershell", "elvish"] {
        let home = TempDir::new().unwrap();
        let expected = completion(&home, shell, &[]);
        assert!(expected.status.success());
        for option in ["--output", "--path"] {
            let path = home.path().join(format!("nested/{shell}/{option}"));
            let output = completion(&home, shell, &[option, path.to_str().unwrap()]);
            assert!(output.status.success(), "{shell} {option}: {:?}", output);
            assert_eq!(fs::read(path).unwrap(), expected.stdout);
        }
    }
}

#[test]
fn output_takes_precedence_over_install_path() {
    let home = TempDir::new().unwrap();
    let output = completion(
        &home,
        "zsh",
        &["--path", "install/_lla", "--output", "output/_lla"],
    );
    assert!(output.status.success(), "{:?}", output);
    assert!(home.path().join("output/_lla").is_file());
    assert!(!home.path().join("install").exists());
}
