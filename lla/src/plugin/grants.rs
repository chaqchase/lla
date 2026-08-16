use lla_plugin_interface::manifest::{PluginManifest, PluginPermissions};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrantStore {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginGrant>,
}

impl Default for GrantStore {
    fn default() -> Self {
        Self {
            schema_version: schema_version(),
            plugins: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginGrant {
    pub version: String,
    pub permissions: PluginPermissions,
}

fn schema_version() -> u32 {
    1
}

impl GrantStore {
    pub fn path(plugins_dir: &Path) -> PathBuf {
        plugins_dir.join("plugin-grants.toml")
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self {
                schema_version: schema_version(),
                plugins: BTreeMap::new(),
            });
        }
        let source = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let store: Self = toml::from_str(&source)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        if store.schema_version != schema_version() {
            return Err(format!(
                "unsupported grant schema {} in {}",
                store.schema_version,
                path.display()
            ));
        }
        Ok(store)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        let source = toml::to_string_pretty(self)
            .map_err(|error| format!("failed to encode grants: {error}"))?;
        let temporary = path.with_extension("toml.tmp");
        fs::write(&temporary, source)
            .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("failed to replace {}: {error}", path.display()))
    }

    pub fn approves(&self, manifest: &PluginManifest) -> bool {
        if manifest.permissions == PluginPermissions::default() {
            return true;
        }
        self.plugins
            .get(&manifest.plugin.id)
            .is_some_and(|grant| contains(&grant.permissions, &manifest.permissions))
    }

    pub fn record(&mut self, manifest: &PluginManifest) {
        self.plugins.insert(
            manifest.plugin.id.clone(),
            PluginGrant {
                version: manifest.plugin.version.clone(),
                permissions: manifest.permissions.clone(),
            },
        );
    }

    pub fn expanded_permissions(&self, manifest: &PluginManifest) -> Vec<String> {
        let previous = self
            .plugins
            .get(&manifest.plugin.id)
            .map(|grant| &grant.permissions);
        let mut expanded = Vec::new();
        for permission in &manifest.permissions.filesystem {
            if previous.is_none_or(|grant| !grant.filesystem.contains(permission)) {
                expanded.push(format!("filesystem:{permission}"));
            }
        }
        for domain in &manifest.permissions.network {
            if previous.is_none_or(|grant| !grant.network.contains(domain)) {
                expanded.push(format!("network:{domain}"));
            }
        }
        for (name, requested, already) in [
            (
                "process",
                manifest.permissions.process,
                previous.is_some_and(|value| value.process),
            ),
            (
                "clipboard",
                manifest.permissions.clipboard,
                previous.is_some_and(|value| value.clipboard),
            ),
            (
                "open-url",
                manifest.permissions.open_url,
                previous.is_some_and(|value| value.open_url),
            ),
        ] {
            if requested && !already {
                expanded.push(name.to_string());
            }
        }
        expanded
    }
}

fn contains(granted: &PluginPermissions, requested: &PluginPermissions) -> bool {
    requested
        .filesystem
        .iter()
        .all(|permission| granted.filesystem.contains(permission))
        && requested
            .network
            .iter()
            .all(|domain| granted.network.contains(domain))
        && (!requested.process || granted.process)
        && (!requested.clipboard || granted.clipboard)
        && (!requested.open_url || granted.open_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_expanded_permissions() {
        let manifest: PluginManifest = toml::from_str(
            r#"
schema_version = 3
[plugin]
id = "dev.lla.test"
name = "test"
version = "0.6.0"
api_min = 3
api_max = 3
runtime = "wasm-component"
entrypoint = "test.wasm"
[permissions]
network = ["example.com"]
"#,
        )
        .unwrap();
        let mut store = GrantStore::default();
        assert_eq!(
            store.expanded_permissions(&manifest),
            ["network:example.com"]
        );
        store.record(&manifest);
        assert!(store.approves(&manifest));
        assert!(store.expanded_permissions(&manifest).is_empty());
    }
}
