use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub recursive: bool,
    pub max_depth: usize,
    pub include_hidden: bool,
    pub follow_symlinks: bool,
    pub same_filesystem: bool,
}

#[derive(Debug, Clone)]
pub struct ScannedEntry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_file: bool,
    pub size: u64,
    pub modified_secs: u64,
    pub created_secs: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    pub root: PathBuf,
    pub entries: Vec<ScannedEntry>,
    pub ignored: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationKind {
    Organize,
    Quarantine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanAction {
    pub id: usize,
    pub kind: OperationKind,
    pub source: PathBuf,
    pub target: PathBuf,
    pub reason: String,
    pub category: Option<String>,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupPlan {
    pub id: String,
    pub created_at: String,
    pub root: PathBuf,
    pub profile: String,
    pub actions: Vec<PlanAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub id: String,
    pub plan_id: String,
    pub created_at: String,
    pub root: PathBuf,
    pub actions: Vec<ManifestAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestAction {
    pub operation: OperationKind,
    pub source: PathBuf,
    pub target: PathBuf,
    pub reason: String,
    pub hash: Option<String>,
    #[serde(default = "default_completed")]
    pub completed: bool,
    #[serde(default)]
    pub restored: bool,
}

fn default_completed() -> bool {
    true
}
