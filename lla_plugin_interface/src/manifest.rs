use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path};

pub const MANIFEST_FILE_NAME: &str = "plugin.toml";
const FILESYSTEM_PERMISSION_SCOPES: &[&str] = &[
    "metadata:selection",
    "metadata:tree",
    "read:selection",
    "read:tree",
    "read:user-path",
    "write:selected-destination",
    "write:tree",
    "write:user-path",
    "delete:selection",
    "delete:quarantine",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub plugin: PluginDescriptor,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub permissions: PluginPermissions,
    #[serde(default)]
    pub fields: Vec<FieldDescriptor>,
}

impl PluginManifest {
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let source = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let manifest: Self = toml::from_str(&source)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_identifier("plugin.id", &self.plugin.id)?;
        validate_identifier("plugin.name", &self.plugin.name)?;
        if self.plugin.version.trim().is_empty() {
            return Err("plugin.version must not be empty".to_string());
        }
        if self.plugin.api_min == 0 || self.plugin.api_max < self.plugin.api_min {
            return Err("plugin API range is invalid".to_string());
        }
        if self.plugin.entrypoint.trim().is_empty() {
            return Err("plugin.entrypoint must not be empty".to_string());
        }
        let entrypoint = Path::new(&self.plugin.entrypoint);
        if entrypoint.is_absolute()
            || !matches!(
                entrypoint.components().collect::<Vec<_>>().as_slice(),
                [Component::Normal(_)]
            )
        {
            return Err("plugin.entrypoint must be a single package-local file name".to_string());
        }

        validate_unique_nonempty("capabilities.formats", &self.capabilities.formats)?;
        validate_unique_nonempty("capabilities.actions", &self.capabilities.actions)?;
        validate_unique_nonempty("permissions.filesystem", &self.permissions.filesystem)?;
        validate_unique_nonempty("permissions.network", &self.permissions.network)?;
        for scope in &self.permissions.filesystem {
            if !FILESYSTEM_PERMISSION_SCOPES.contains(&scope.as_str()) {
                return Err(format!("unknown filesystem permission scope '{scope}'"));
            }
        }
        for domain in &self.permissions.network {
            if domain != "*"
                && (domain.starts_with('.')
                    || domain.ends_with('.')
                    || domain.contains("..")
                    || !domain.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
                    }))
            {
                return Err(format!("invalid network permission domain '{domain}'"));
            }
        }

        let mut fields = HashSet::new();
        for field in &self.fields {
            validate_identifier("field name", &field.name)?;
            if !fields.insert(field.name.as_str()) {
                return Err(format!("duplicate field name '{}'", field.name));
            }
        }
        Ok(())
    }

    pub fn supports_host_api(&self, host_api: u32) -> bool {
        self.plugin.api_min <= host_api && host_api <= self.plugin.api_max
    }
}

fn validate_identifier(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(format!(
            "{label} must contain only ASCII letters, numbers, '.', '_' or '-'"
        ));
    }
    Ok(())
}

fn validate_unique_nonempty(label: &str, values: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for value in values {
        if value.is_empty() || value.trim() != value {
            return Err(format!("{label} entries must not be empty or padded"));
        }
        if !seen.insert(value.as_str()) {
            return Err(format!("duplicate {label} entry '{value}'"));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_min: u32,
    pub api_max: u32,
    #[serde(default)]
    pub runtime: PluginRuntime,
    pub entrypoint: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PluginRuntime {
    #[default]
    Native,
    WasmWasi,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub decorates_entries: bool,
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub machine_output: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginPermissions {
    #[serde(default)]
    pub filesystem: Vec<String>,
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub process: bool,
    #[serde(default)]
    pub clipboard: bool,
    #[serde(default)]
    pub open_url: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldDescriptor {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    #[serde(default)]
    pub sortable: bool,
    #[serde(default)]
    pub filterable: bool,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    Bytes,
    Timestamp,
    Path,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_validates_manifest() {
        let manifest: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "dev.lla.example"
name = "example"
version = "1.0.0"
api_min = 2
api_max = 2
runtime = "native"
entrypoint = "libexample.so"

[capabilities]
decorates_entries = true
formats = ["default"]

[[fields]]
name = "score"
type = "integer"
sortable = true
"#,
        )
        .unwrap();

        manifest.validate().unwrap();
        assert!(manifest.supports_host_api(2));
        assert!(!manifest.supports_host_api(3));
    }

    #[test]
    fn every_workspace_plugin_has_a_valid_v2_manifest() {
        let plugins_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins");
        if !plugins_dir.is_dir() {
            return;
        }

        let mut plugin_count = 0;
        for entry in fs::read_dir(&plugins_dir).unwrap() {
            let plugin_dir = entry.unwrap().path();
            if !plugin_dir.join("Cargo.toml").is_file() {
                continue;
            }
            plugin_count += 1;
            let manifest = PluginManifest::from_path(&plugin_dir.join(MANIFEST_FILE_NAME))
                .unwrap_or_else(|error| panic!("{}: {error}", plugin_dir.display()));
            assert_eq!(
                manifest.plugin.name,
                plugin_dir.file_name().unwrap().to_string_lossy()
            );
            assert!(manifest.supports_host_api(crate::CURRENT_PLUGIN_API_VERSION));

            let source = fs::read_to_string(plugin_dir.join("src/lib.rs")).unwrap();
            assert!(
                source.contains("declare_plugin!("),
                "{} does not export the shared v1/v2 plugin ABI",
                manifest.plugin.name
            );
            if source.contains("arboard::Clipboard") {
                assert!(
                    manifest.permissions.clipboard,
                    "{} uses the clipboard without declaring it",
                    manifest.plugin.name
                );
            }
            let opens_urls = source.contains("xdg-open")
                || source.contains("Command::new(\"open\")")
                || source.contains("let open_command = \"start\"")
                || source.contains("let cmd = \"start\"");
            if opens_urls {
                assert!(
                    manifest.permissions.open_url,
                    "{} opens URLs without declaring it",
                    manifest.plugin.name
                );
            }
            if source.contains("Command::new") && !opens_urls {
                assert!(
                    manifest.permissions.process,
                    "{} executes processes without declaring it",
                    manifest.plugin.name
                );
            }
        }
        assert!(plugin_count > 0);
    }

    #[test]
    fn rejects_entrypoints_outside_the_package() {
        let mut manifest: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "dev.lla.example"
name = "example"
version = "1.0.0"
api_min = 2
api_max = 2
entrypoint = "example"
"#,
        )
        .unwrap();

        manifest.plugin.entrypoint = "../libexample.so".to_string();
        assert!(manifest.validate().is_err());
        manifest.plugin.entrypoint = "/tmp/libexample.so".to_string();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_capabilities_and_fields() {
        let manifest: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "dev.lla.example"
name = "example"
version = "1.0.0"
api_min = 2
api_max = 2
entrypoint = "example"

[capabilities]
actions = ["help", "help"]

[[fields]]
name = "score"
type = "integer"

[[fields]]
name = "score"
type = "integer"
"#,
        )
        .unwrap();

        assert!(manifest.validate().is_err());
    }

    #[test]
    fn rejects_unknown_or_url_shaped_permissions() {
        let mut manifest: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "dev.lla.example"
name = "example"
version = "1.0.0"
api_min = 2
api_max = 2
entrypoint = "example"

[permissions]
filesystem = ["read:anywhere"]
"#,
        )
        .unwrap();
        assert!(manifest.validate().is_err());

        manifest.permissions.filesystem.clear();
        manifest.permissions.network = vec!["https://example.com".to_string()];
        assert!(manifest.validate().is_err());
    }
}
