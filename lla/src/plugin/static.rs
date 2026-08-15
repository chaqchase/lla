use crate::config::Config;
use crate::error::{LlaError, Result};
use lla_plugin_interface::{proto, ActionInfo};
use std::collections::HashSet;
use std::path::Path;

pub const DYNAMIC_PLUGINS_AVAILABLE: bool = false;
pub const DYNAMIC_PLUGINS_UNAVAILABLE: &str =
    "Dynamic plugins are unavailable in the static musl build; use a GNU build for plugin support.";

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

    pub fn list_plugins(&mut self) -> Vec<(String, String, String)> {
        Vec::new()
    }

    pub fn get_plugin_actions(&mut self, _plugin_name: &str) -> Result<Vec<ActionInfo>> {
        Err(unavailable())
    }

    pub fn discover_plugins<P: AsRef<Path>>(&mut self, _plugin_dir: P) -> Result<()> {
        Ok(())
    }

    pub fn discover_plugins_named<P: AsRef<Path>>(
        &mut self,
        _plugin_dir: P,
        _names: &HashSet<String>,
    ) -> Result<()> {
        Ok(())
    }

    pub fn enable_plugin(&mut self, _name: &str) -> Result<()> {
        Err(unavailable())
    }

    pub fn disable_plugin(&mut self, _name: &str) -> Result<()> {
        Err(unavailable())
    }

    pub fn decorate_entry(&mut self, _entry: &mut proto::DecoratedEntry, _format: &str) {}

    pub fn format_fields(&mut self, _entry: &proto::DecoratedEntry, _format: &str) -> Vec<String> {
        Vec::new()
    }

    pub fn clean_plugins(&mut self) -> Result<()> {
        Err(unavailable())
    }
}

fn unavailable() -> LlaError {
    LlaError::Plugin(DYNAMIC_PLUGINS_UNAVAILABLE.to_string())
}
