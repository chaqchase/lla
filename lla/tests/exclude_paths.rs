use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_exclude_paths_recursive_and_nonrecursive() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let incl_dir = root.join("KeepMe");
    let excl_dir = root.join("Library").join("Mobile Documents");
    fs::create_dir_all(&incl_dir).unwrap();
    fs::create_dir_all(&excl_dir).unwrap();
    fs::write(incl_dir.join("file.txt"), b"ok").unwrap();
    fs::write(excl_dir.join("secret.txt"), b"nope").unwrap();

    let bin = std::env::var("CARGO_BIN_EXE_lla").expect("binary path not set by cargo");
    let tmp_home = tmp.path().join("home");
    fs::create_dir_all(&tmp_home).unwrap();

    // Initialize config
    let status = Command::new(&bin)
        .arg("init")
        .env("HOME", &tmp_home)
        .status()
        .expect("failed to run lla init");
    assert!(status.success());

    // Set exclude_paths to the Mobile Documents directory
    let exclude_json = format!("[\"{}\"]", excl_dir.to_string_lossy().replace('\\', "\\\\"));
    let status = Command::new(&bin)
        .args(["config", "--set", "exclude_paths", &exclude_json])
        .env("HOME", &tmp_home)
        .status()
        .expect("failed to set exclude_paths");
    assert!(status.success());

    // Non-recursive JSON listing at repository root
    let output = Command::new(&bin)
        .arg(root)
        .arg("--json")
        .env("HOME", &tmp_home)
        .output()
        .expect("failed to run lla");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let arr = json.as_array().expect("json array");
    let paths: Vec<PathBuf> = arr
        .iter()
        .filter_map(|v| v.get("path").and_then(|p| p.as_str()))
        .map(PathBuf::from)
        .collect();
    assert!(paths.iter().any(|p| p.ends_with("KeepMe")));
    // The nested Mobile Documents should not be returned at top-level
    assert!(!paths.iter().any(|p| p.ends_with("Mobile Documents")));

    // Non-recursive listing inside Library should hide the excluded directory itself
    let output = Command::new(&bin)
        .arg(root.join("Library"))
        .arg("--json")
        .env("HOME", &tmp_home)
        .output()
        .expect("failed to run lla in Library");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let arr = json.as_array().expect("json array");
    let paths_in_lib: Vec<PathBuf> = arr
        .iter()
        .filter_map(|v| v.get("path").and_then(|p| p.as_str()))
        .map(PathBuf::from)
        .collect();
    assert!(!paths_in_lib.iter().any(|p| p.ends_with("Mobile Documents")));

    // Recursive JSON listing should not contain files under excluded directory
    let output = Command::new(&bin)
        .arg(root)
        .arg("-R")
        .arg("--json")
        .env("HOME", &tmp_home)
        .output()
        .expect("failed to run lla -R");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let arr = json.as_array().expect("json array");
    let paths_recursive: Vec<PathBuf> = arr
        .iter()
        .filter_map(|v| v.get("path").and_then(|p| p.as_str()))
        .map(PathBuf::from)
        .collect();
    assert!(paths_recursive.iter().any(|p| p.ends_with("KeepMe")));
    assert!(!paths_recursive.iter().any(|p| p.ends_with("secret.txt")));
}
