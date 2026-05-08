use std::path::PathBuf;

pub fn plugin_state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("lla")
        .join("plugins")
        .join("folder_cleaner")
}

pub fn plans_dir() -> PathBuf {
    plugin_state_dir().join("plans")
}

pub fn runs_dir() -> PathBuf {
    plugin_state_dir().join("runs")
}

pub fn plan_path(plan_id: &str) -> PathBuf {
    plans_dir().join(format!("{}.json", plan_id))
}

pub fn run_path(run_id: &str) -> PathBuf {
    runs_dir().join(format!("{}.json", run_id))
}
