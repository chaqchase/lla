use std::process::Command;
use tempfile::TempDir;

#[test]
fn license_prints_the_full_embedded_notice() {
    let home = TempDir::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lla"))
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("APPDATA", home.path().join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.path().join("AppData/Local"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .current_dir(home.path())
        .arg("--license")
        .output()
        .expect("run license command");

    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, include_bytes!("../LICENSE"));
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn packaged_license_matches_workspace_license() {
    let workspace_license = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../LICENSE");
    // Published crates contain their own license and have no workspace parent.
    if workspace_license.is_file() {
        assert_eq!(
            std::fs::read(workspace_license).unwrap(),
            include_bytes!("../LICENSE")
        );
    }
}
