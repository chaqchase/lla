use crate::commands::args::PluginOutputFormat;
use crate::config::Config;
use crate::error::{LlaError, Result};
use lla_plugin_interface::{proto, ActionInfo};
use std::collections::HashSet;
use std::path::PathBuf;

pub const DYNAMIC_PLUGINS_AVAILABLE: bool = false;
pub const DYNAMIC_PLUGINS_UNAVAILABLE: &str =
    "Dynamic plugins are unavailable in this build; install a full-featured lla binary.";

pub(crate) fn wasm_runtime_supported(architecture: &str) -> bool {
    matches!(architecture, "x86_64" | "aarch64")
}

pub struct PluginManager {
    pub enabled_plugins: HashSet<String>,
}

impl PluginManager {
    pub fn new(_config: Config) -> Self {
        Self {
            enabled_plugins: HashSet::new(),
        }
    }

    pub fn perform_plugin_action(
        &mut self,
        _plugin_name: &str,
        _action: &str,
        _args: &[String],
    ) -> Result<()> {
        Err(unavailable())
    }

    pub fn run_plugin_action(
        &mut self,
        _plugin_name: &str,
        _action: &str,
        _args: &[String],
        _output: PluginOutputFormat,
    ) -> Result<()> {
        Err(unavailable())
    }

    pub fn list_plugins(&mut self) -> Vec<(String, String, String)> {
        Vec::new()
    }

    pub fn get_plugin_actions(&mut self, _plugin_name: &str) -> Result<Vec<ActionInfo>> {
        Err(unavailable())
    }

    pub fn discover_plugin_paths(&mut self, _plugin_dirs: &[PathBuf]) -> Result<()> {
        Ok(())
    }

    pub fn discover_plugin_paths_named(
        &mut self,
        _plugin_dirs: &[PathBuf],
        _names: &HashSet<String>,
    ) -> Result<()> {
        Ok(())
    }

    pub fn doctor(&self, _paths: &[PathBuf]) -> Result<bool> {
        Err(unavailable())
    }

    pub fn print_manifest(&self, _plugin_name: &str, _permissions_only: bool) -> Result<()> {
        Err(unavailable())
    }

    pub fn enable_plugin(&mut self, _name: &str) -> Result<()> {
        Err(unavailable())
    }

    pub fn disable_plugin(&mut self, _name: &str) -> Result<()> {
        Err(unavailable())
    }

    pub fn decorate_entry(&mut self, _entry: &mut proto::DecoratedEntry, _format: &str) {}

    pub fn decorate_entries(&mut self, _entries: &mut [proto::DecoratedEntry], _format: &str) {}

    pub fn format_fields(&mut self, _entry: &proto::DecoratedEntry, _format: &str) -> Vec<String> {
        Vec::new()
    }

    pub fn clean_plugins(&mut self, _plugins_dir: &std::path::Path) -> Result<()> {
        Err(unavailable())
    }
}

fn unavailable() -> LlaError {
    LlaError::Plugin(DYNAMIC_PLUGINS_UNAVAILABLE.to_string())
}
