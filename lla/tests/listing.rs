use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

/// Runs `lla` with an isolated home directory so no user config, history or
/// plugins leak into the test.
fn lla(home: &TempDir, cwd: &Path, args: &[&str]) -> Output {
    command(home, cwd, args).output().expect("run lla")
}

fn command(home: &TempDir, cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lla"));
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("APPDATA", home.path().join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.path().join("AppData/Local"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("NO_COLOR", "1")
        .current_dir(cwd)
        .args(args);
    command
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Writes a project `.lla.toml`, which lla picks up from the working directory.
/// Pointing HOME at a temp dir doesn't move the global config on Windows.
fn write_profile(dir: &Path, contents: &str) {
    fs::write(dir.join(".lla.toml"), contents).unwrap();
}

fn tree_fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    let path = root.path();
    fs::create_dir_all(path.join("sub/deeper")).unwrap();
    fs::create_dir_all(path.join(".git/objects")).unwrap();
    fs::write(path.join(".git/HEAD"), "").unwrap();
    for name in [
        "file10",
        "file2",
        "README.md",
        ".env",
        "sub/x.rs",
        "sub/deeper/y",
    ] {
        fs::write(path.join(name), "").unwrap();
    }
    root
}

#[test]
fn missing_path_error_names_the_path() {
    let home = TempDir::new().unwrap();
    let output = lla(&home, home.path(), &["does-not-exist"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        stderr(&output).contains("does-not-exist: "),
        "{}",
        stderr(&output)
    );
}

#[test]
fn lists_several_paths_and_keeps_going_after_a_failure() {
    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("one")).unwrap();
    fs::create_dir_all(root.path().join("two")).unwrap();
    fs::write(root.path().join("one/a.txt"), "").unwrap();
    fs::write(root.path().join("two/b.txt"), "").unwrap();

    let output = lla(&home, root.path(), &["one", "two"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    let one = text.find("one:").expect("header for one");
    let two = text.find("two:").expect("header for two");
    assert!(one < text.find("a.txt").unwrap());
    assert!(two < text.find("b.txt").unwrap());
    assert!(one < two);

    let output = lla(&home, root.path(), &["one", "missing", "two"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stdout(&output).contains("b.txt"));
    assert!(stderr(&output).contains("missing: "));
}

#[test]
fn filters_are_case_insensitive_for_uppercase_patterns() {
    let home = TempDir::new().unwrap();
    let root = tree_fixture();
    let output = lla(&home, root.path(), &["-f", "README"]);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("README.md"), "{}", stdout(&output));

    let output = lla(&home, root.path(), &["-f", "readme", "--case-sensitive"]);
    assert!(
        !stdout(&output).contains("README.md"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn tree_is_sorted_shows_dotfiles_and_skips_git_internals() {
    let home = TempDir::new().unwrap();
    let root = tree_fixture();
    let output = lla(&home, root.path(), &["-t", "--no-icons"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);

    assert!(text.contains(".env"), "{text}");
    assert!(text.contains(".git/"), "{text}");
    assert!(!text.contains("HEAD"), "{text}");
    assert!(!text.contains("objects"), "{text}");
    // Natural order is the default: file2 before file10.
    assert!(
        text.find("file2").unwrap() < text.find("file10").unwrap(),
        "{text}"
    );
    assert!(text.contains("y"), "{text}");

    let output = lla(&home, root.path(), &["-t", "--no-dotfiles"]);
    let text = stdout(&output);
    assert!(!text.contains(".env"), "{text}");
    assert!(!text.contains(".git"), "{text}");
}

#[test]
fn tree_honours_sort_options() {
    let home = TempDir::new().unwrap();
    let root = tree_fixture();
    let output = lla(&home, root.path(), &["-t", "--sort-dirs-first", "-r"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    // Directories stay first; files follow in reversed natural order.
    assert!(
        text.find("sub/").unwrap() < text.find("README.md").unwrap(),
        "{text}"
    );
    assert!(
        text.find("README.md").unwrap() < text.find("file10").unwrap(),
        "{text}"
    );
    assert!(
        text.find("file10").unwrap() < text.find("file2").unwrap(),
        "{text}"
    );
}

#[test]
fn config_flags_can_be_switched_off_per_command() {
    let home = TempDir::new().unwrap();
    let root = tree_fixture();
    write_profile(root.path(), "[sort]\ndirs_first = true\n");

    let output = lla(&home, root.path(), &[]);
    let text = stdout(&output);
    assert!(
        text.find("sub").unwrap() < text.find("README.md").unwrap(),
        "{text}"
    );

    let output = lla(&home, root.path(), &["--no-sort-dirs-first"]);
    let text = stdout(&output);
    assert!(
        text.find("README.md").unwrap() < text.find("sub").unwrap(),
        "{text}"
    );
}

#[test]
fn invalid_depth_is_rejected() {
    let home = TempDir::new().unwrap();
    let output = lla(&home, home.path(), &["-t", "-d", "abc"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr(&output).contains("abc"), "{}", stderr(&output));
}

#[test]
fn piped_output_has_no_icons_from_config() {
    let home = TempDir::new().unwrap();
    let root = tree_fixture();
    write_profile(root.path(), "show_icons = true\n");
    let output = lla(&home, root.path(), &[]);
    let text = stdout(&output);
    assert!(
        text.lines().any(|line| line.trim() == "README.md"),
        "{text}"
    );

    let output = lla(&home, root.path(), &["--icons"]);
    assert!(!stdout(&output)
        .lines()
        .any(|line| line.trim() == "README.md"));
}

#[test]
fn listing_does_not_write_a_listing_cache() {
    let home = TempDir::new().unwrap();
    let root = tree_fixture();
    let output = lla(&home, root.path(), &["-t"]);
    assert!(output.status.success(), "{output:?}");
    for cache in [".cache/lla/listings", "Library/Caches/lla/listings"] {
        assert!(!home.path().join(cache).exists(), "created {cache}");
    }
}

#[cfg(unix)]
#[test]
fn closed_pipe_does_not_panic() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    let home = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    for index in 0..3000 {
        fs::write(root.path().join(format!("file-{index}")), "").unwrap();
    }
    let mut child = command(&home, root.path(), &["-R"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let errors = stderr(&output);
    assert!(!errors.contains("panicked"), "{errors}");
    assert_ne!(output.status.code(), Some(101));
}
