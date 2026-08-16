use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Component, Path};

pub const MANIFEST_FILE_NAME: &str = "plugin.toml";
pub const MANIFEST_SCHEMA_VERSION: u32 = 3;
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub plugin: PluginDescriptor,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub permissions: PluginPermissions,
    #[serde(default)]
    pub fields: Vec<FieldDescriptor>,
    #[serde(default)]
    pub actions: Vec<ActionDescriptor>,
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
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(format!(
                "unsupported plugin manifest schema {}; expected {}",
                self.schema_version, MANIFEST_SCHEMA_VERSION
            ));
        }
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
        validate_unique_nonempty("permissions.filesystem", &self.permissions.filesystem)?;
        validate_unique_nonempty("permissions.network", &self.permissions.network)?;
        for scope in &self.permissions.filesystem {
            if !FILESYSTEM_PERMISSION_SCOPES.contains(&scope.as_str()) {
                return Err(format!("unknown filesystem permission scope '{scope}'"));
            }
        }
        for domain in &self.permissions.network {
            if domain == "*"
                || (domain.starts_with('.')
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
        let mut actions = HashSet::new();
        for action in &self.actions {
            action.validate()?;
            if !actions.insert(action.id.as_str()) {
                return Err(format!("duplicate action id '{}'", action.id));
            }
        }
        if self.plugin.runtime == PluginRuntime::WasmComponent && self.permissions.process {
            return Err("WASM plugins cannot request process permission in API v3".to_string());
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
    WasmComponent,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub decorates_entries: bool,
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub machine_output: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActionDescriptor {
    pub id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub arguments: Vec<ActionArgument>,
    #[serde(default)]
    pub output: ActionOutputSchema,
}

impl ActionDescriptor {
    fn validate(&self) -> Result<(), String> {
        validate_identifier("action id", &self.id)?;
        if self.description.trim().is_empty() {
            return Err(format!("action '{}' needs a description", self.id));
        }
        validate_unique_nonempty("action examples", &self.examples)?;
        let mut names = HashSet::new();
        let mut positions = HashSet::new();
        let mut options = HashSet::new();
        for argument in &self.arguments {
            validate_identifier("action argument name", &argument.name)?;
            if !names.insert(argument.name.as_str()) {
                return Err(format!(
                    "duplicate argument '{}' in action '{}'",
                    argument.name, self.id
                ));
            }
            if let Some(position) = argument.position {
                if !positions.insert(position) {
                    return Err(format!(
                        "duplicate positional index {position} in action '{}'",
                        self.id
                    ));
                }
            }
            if let Some(option) = &argument.option {
                if !option.starts_with("--") || option.len() < 3 || !options.insert(option.as_str())
                {
                    return Err(format!(
                        "invalid or duplicate option '{option}' in action '{}'",
                        self.id
                    ));
                }
            }
            if argument.position.is_none() && argument.option.is_none() {
                return Err(format!(
                    "argument '{}' in action '{}' needs position or option",
                    argument.name, self.id
                ));
            }
            argument.validate(&self.id)?;
        }
        self.output.validate(&self.id)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActionArgument {
    pub name: String,
    #[serde(rename = "type")]
    pub argument_type: ActionArgumentType,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<ManifestValue>,
    #[serde(default)]
    pub repeatable: bool,
    #[serde(default)]
    pub choices: Vec<ManifestValue>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub position: Option<u32>,
    #[serde(default)]
    pub option: Option<String>,
}

impl ActionArgument {
    fn validate(&self, action: &str) -> Result<(), String> {
        if self.description.trim().is_empty() {
            return Err(format!(
                "argument '{}' in action '{action}' needs a description",
                self.name
            ));
        }
        if self.required && self.default.is_some() {
            return Err(format!(
                "required argument '{}' in action '{action}' cannot have a default",
                self.name
            ));
        }
        if self.min.zip(self.max).is_some_and(|(min, max)| min > max) {
            return Err(format!(
                "argument '{}' in action '{action}' has min greater than max",
                self.name
            ));
        }
        if (self.min.is_some() || self.max.is_some())
            && !matches!(
                self.argument_type,
                ActionArgumentType::Integer | ActionArgumentType::Float
            )
        {
            return Err(format!(
                "argument '{}' in action '{action}' uses a numeric range with a non-numeric type",
                self.name
            ));
        }
        for value in self.default.iter().chain(self.choices.iter()) {
            if !value.matches(self.argument_type) {
                return Err(format!(
                    "argument '{}' in action '{action}' contains a value with the wrong type",
                    self.name
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ActionArgumentType {
    String,
    Integer,
    Float,
    Boolean,
    Path,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ManifestValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

impl ManifestValue {
    fn matches(&self, expected: ActionArgumentType) -> bool {
        matches!(
            (self, expected),
            (
                Self::String(_),
                ActionArgumentType::String | ActionArgumentType::Path
            ) | (Self::Integer(_), ActionArgumentType::Integer)
                | (Self::Float(_), ActionArgumentType::Float)
                | (Self::Integer(_), ActionArgumentType::Float)
                | (Self::Boolean(_), ActionArgumentType::Boolean)
        )
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ActionOutputSchema {
    #[default]
    None,
    Text,
    Value {
        #[serde(default)]
        schema: BTreeMap<String, toml::Value>,
    },
    Table {
        columns: Vec<TableColumn>,
    },
}

impl ActionOutputSchema {
    fn validate(&self, action: &str) -> Result<(), String> {
        if let Self::Table { columns } = self {
            if columns.is_empty() {
                return Err(format!(
                    "table output for action '{action}' needs at least one column"
                ));
            }
            let mut names = HashSet::new();
            for column in columns {
                validate_identifier("table column", &column.name)?;
                if !names.insert(column.name.as_str()) {
                    return Err(format!(
                        "duplicate table column '{}' in action '{action}'",
                        column.name
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    #[serde(default)]
    pub description: String,
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
schema_version = 3

[plugin]
id = "dev.lla.example"
name = "example"
version = "1.0.0"
api_min = 3
api_max = 3
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
        assert!(manifest.supports_host_api(3));
        assert!(!manifest.supports_host_api(2));
    }

    #[test]
    fn every_workspace_plugin_has_a_valid_v3_manifest() {
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
            assert!(manifest.supports_host_api(crate::PLUGIN_API_VERSION));

            let source = fs::read_to_string(plugin_dir.join("src/lib.rs")).unwrap();
            assert!(
                source.contains("export_plugin!("),
                "{} does not export the shared v3 plugin ABI",
                manifest.plugin.name
            );
            for legacy_api in [
                "handle_raw_request",
                "PluginRequest",
                "PluginResponse",
                "ProtobufHandler",
            ] {
                assert!(
                    !source.contains(legacy_api),
                    "{} still uses removed source adapter {legacy_api}",
                    manifest.plugin.name
                );
            }
            if manifest.capabilities.decorates_entries {
                assert!(
                    source.contains("fn decorate_entry") && source.contains("fn decorate_batch"),
                    "{} must implement both v3 decoration paths",
                    manifest.plugin.name
                );
                assert!(
                    source.contains("insert_field(")
                        || source.contains("promote_")
                        || source.contains("typed_fields"),
                    "{} declares typed fields without emitting v3 values",
                    manifest.plugin.name
                );
            }
            if !manifest.actions.is_empty() {
                assert!(
                    source.contains("fn run_action") && source.contains("fn registered_actions"),
                    "{} must register and implement its v3 actions",
                    manifest.plugin.name
                );
                for action in &manifest.actions {
                    assert!(
                        !action.description.starts_with("Run the "),
                        "{}:{} still has a generated action description",
                        manifest.plugin.name,
                        action.id
                    );
                    let command_prefix =
                        format!("lla plugin run {} {}", manifest.plugin.name, action.id);
                    assert!(
                        action
                            .examples
                            .iter()
                            .all(|example| example.starts_with(&command_prefix)),
                        "{}:{} must use the canonical v3 command in every example",
                        manifest.plugin.name,
                        action.id
                    );
                    for argument in &action.arguments {
                        assert_ne!(
                            argument.name, "args",
                            "{}:{} still exposes the legacy untyped args bag",
                            manifest.plugin.name, action.id
                        );
                    }
                }
            }
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
schema_version = 3

[plugin]
id = "dev.lla.example"
name = "example"
version = "1.0.0"
api_min = 3
api_max = 3
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
schema_version = 3

[plugin]
id = "dev.lla.example"
name = "example"
version = "1.0.0"
api_min = 3
api_max = 3
entrypoint = "example"

[[actions]]
id = "help"

[[actions]]
id = "help"

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
schema_version = 3

[plugin]
id = "dev.lla.example"
name = "example"
version = "1.0.0"
api_min = 3
api_max = 3
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
