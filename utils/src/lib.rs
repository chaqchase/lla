pub mod actions;
pub mod cache;
pub mod config;
pub mod format;
pub mod syntax;
pub mod trash;
pub mod ui;

pub use actions::{Action, ActionHelp, ActionRegistry};
pub use cache::{
    canonical_cache_key, file_fingerprint, plugin_data_dir, text_fingerprint, PersistentCache,
};
pub use config::{ConfigManager, PluginConfig};
pub use syntax::CodeHighlighter;
pub use ui::{
    components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
    TextBlock, TextStyle,
};

use lla_plugin_interface::manifest::{ActionArgument, PluginManifest};
use lla_plugin_interface::proto;
pub use lla_plugin_sdk::manifest_action_infos;
use lla_plugin_sdk::{response, ActionArguments, ActionError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub use lla_plugin_interface::ActionInfo;

#[derive(Clone, Serialize, Deserialize)]
pub struct DecoratedEntry {
    pub path: PathBuf,
    pub metadata: EntryMetadata,
    pub custom_fields: HashMap<String, String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EntryMetadata {
    pub size: u64,
    pub modified: u64,
    pub accessed: u64,
    pub created: u64,
    pub is_dir: bool,
    pub is_file: bool,
    pub is_symlink: bool,
    pub permissions: u32,
    pub uid: u32,
    pub gid: u32,
}

impl From<EntryMetadata> for proto::EntryMetadata {
    fn from(meta: EntryMetadata) -> Self {
        Self {
            size: meta.size,
            modified: meta.modified,
            accessed: meta.accessed,
            created: meta.created,
            is_dir: meta.is_dir,
            is_file: meta.is_file,
            is_symlink: meta.is_symlink,
            permissions: meta.permissions,
            uid: meta.uid,
            gid: meta.gid,
            ..proto::EntryMetadata::default()
        }
    }
}

impl From<proto::EntryMetadata> for EntryMetadata {
    fn from(meta: proto::EntryMetadata) -> Self {
        Self {
            size: meta.size,
            modified: meta.modified,
            accessed: meta.accessed,
            created: meta.created,
            is_dir: meta.is_dir,
            is_file: meta.is_file,
            is_symlink: meta.is_symlink,
            permissions: meta.permissions,
            uid: meta.uid,
            gid: meta.gid,
        }
    }
}

impl From<DecoratedEntry> for proto::DecoratedEntry {
    fn from(entry: DecoratedEntry) -> Self {
        Self {
            path: entry.path.to_string_lossy().to_string(),
            metadata: Some(entry.metadata.into()),
            custom_fields: entry.custom_fields,
            typed_fields: HashMap::new(),
        }
    }
}

fn value_as_strings(value: &proto::TypedValue) -> Vec<String> {
    use proto::typed_value::Value;
    match value.value.as_ref() {
        Some(Value::StringValue(value) | Value::PathValue(value)) => vec![value.clone()],
        Some(Value::IntegerValue(value)) => vec![value.to_string()],
        Some(Value::FloatValue(value)) => vec![value.to_string()],
        Some(Value::BooleanValue(value)) => vec![value.to_string()],
        Some(Value::BytesValue(value) | Value::TimestampValue(value)) => vec![value.to_string()],
        Some(Value::ListValue(values)) => values.values.iter().flat_map(value_as_strings).collect(),
        Some(Value::ObjectValue(_)) | Some(Value::NullValue(_)) | None => Vec::new(),
    }
}

/// Converts validated v3 arguments into the argv shape used by established
/// interactive action implementations.
///
/// New actions should use `lla_plugin_sdk::ActionArgumentsExt` directly. This
/// manifest-aware adapter lets established UIs adopt precise typed schemas
/// without changing their terminal behavior.
pub fn typed_action_arguments_as_strings(
    action_id: &str,
    arguments: &ActionArguments,
    manifest_source: &str,
) -> Result<Vec<String>, ActionError> {
    let manifest: PluginManifest = toml::from_str(manifest_source).map_err(|error| {
        ActionError::new(
            "invalid-embedded-manifest",
            format!("failed to parse embedded plugin manifest: {error}"),
        )
    })?;
    manifest.validate().map_err(|error| {
        ActionError::new(
            "invalid-embedded-manifest",
            format!("embedded plugin manifest is invalid: {error}"),
        )
    })?;
    let action = manifest
        .actions
        .iter()
        .find(|action| action.id == action_id)
        .ok_or_else(|| {
            ActionError::new(
                "unknown-action",
                format!("action '{action_id}' is not declared in plugin.toml"),
            )
        })?;

    let mut positional = action
        .arguments
        .iter()
        .filter(|argument| argument.position.is_some())
        .collect::<Vec<_>>();
    positional.sort_by_key(|argument| argument.position);
    let mut result = Vec::new();
    for argument in positional {
        append_argument(&mut result, argument, arguments)?;
    }
    for argument in action
        .arguments
        .iter()
        .filter(|argument| argument.position.is_none())
    {
        append_argument(&mut result, argument, arguments)?;
    }
    Ok(result)
}

fn append_argument(
    result: &mut Vec<String>,
    descriptor: &ActionArgument,
    arguments: &ActionArguments,
) -> Result<(), ActionError> {
    let Some(value) = arguments.get(&descriptor.name) else {
        return Ok(());
    };
    let values = value_as_strings(value);
    if let Some(option) = descriptor.option.as_ref() {
        if matches!(
            descriptor.argument_type,
            lla_plugin_interface::manifest::ActionArgumentType::Boolean
        ) {
            if values.first().is_some_and(|value| value == "true") {
                result.push(option.clone());
            }
            return Ok(());
        }
        if descriptor.repeatable {
            for value in values {
                result.push(option.clone());
                result.push(value);
            }
        } else if let Some(value) = values.into_iter().next() {
            result.push(option.clone());
            result.push(value);
        }
    } else {
        result.extend(values);
    }
    Ok(())
}

pub fn run_cli_action(
    action_id: &str,
    arguments: ActionArguments,
    manifest_source: &str,
    handler: impl FnOnce(&[String]) -> Result<(), String>,
) -> proto::ActionResponse {
    match typed_action_arguments_as_strings(action_id, &arguments, manifest_source) {
        Ok(arguments) => response::from_result(handler(&arguments)),
        Err(error) => response::error(error),
    }
}

pub fn decode_decorated_entry(entry: proto::DecoratedEntry) -> Result<DecoratedEntry, String> {
    let metadata = entry
        .metadata
        .ok_or_else(|| "Missing metadata in decorated entry".to_string())?;
    Ok(DecoratedEntry {
        path: std::path::PathBuf::from(entry.path),
        metadata: metadata.into(),
        custom_fields: entry.custom_fields,
    })
}

/// Runs an existing entry decorator through the high-level v3 entry API while
/// preserving fields introduced by v3 that the established decorator does not
/// touch.
pub fn map_decorated_entry(
    entry: proto::DecoratedEntry,
    decorate: impl FnOnce(DecoratedEntry) -> DecoratedEntry,
) -> proto::DecoratedEntry {
    let original = entry.clone();
    let Ok(entry) = decode_decorated_entry(entry) else {
        return original;
    };
    let mut decorated: proto::DecoratedEntry = decorate(entry).into();
    decorated.typed_fields = original.typed_fields;
    if let (Some(decorated), Some(original)) =
        (decorated.metadata.as_mut(), original.metadata.as_ref())
    {
        decorated.inode = original.inode;
        decorated.hard_links = original.hard_links;
        decorated.allocated_size = original.allocated_size;
        decorated.xattrs.clone_from(&original.xattrs);
        decorated.has_acl = original.has_acl;
        decorated
            .security_context
            .clone_from(&original.security_context);
        decorated.mount_point.clone_from(&original.mount_point);
        decorated.mount_source.clone_from(&original.mount_source);
        decorated.filesystem.clone_from(&original.filesystem);
    }
    decorated
}

pub fn action_infos(actions: Vec<ActionInfo>) -> Vec<proto::ActionInfo> {
    actions
        .into_iter()
        .map(|action| proto::ActionInfo {
            name: action.name,
            usage: action.usage,
            description: action.description,
            examples: action.examples,
        })
        .collect()
}

pub struct BasePlugin<C: PluginConfig> {
    config_manager: ConfigManager<C>,
}

impl<C: PluginConfig + Default> BasePlugin<C> {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        Self {
            config_manager: ConfigManager::new(plugin_name),
        }
    }

    pub fn with_name(plugin_name: &str) -> Self {
        Self {
            config_manager: ConfigManager::new(plugin_name),
        }
    }

    pub fn config(&self) -> &C {
        self.config_manager.get()
    }

    pub fn config_mut(&mut self) -> &mut C {
        self.config_manager.get_mut()
    }

    pub fn save_config(&self) -> Result<(), String> {
        self.config_manager.save()
    }
}

impl<C: PluginConfig + Default> Default for BasePlugin<C> {
    fn default() -> Self {
        Self::new()
    }
}

pub trait ConfigurablePlugin {
    type Config: PluginConfig;

    fn config(&self) -> &Self::Config;
    fn config_mut(&mut self) -> &mut Self::Config;
}

#[macro_export]
macro_rules! plugin_action {
    ($registry:expr, $name:expr, $usage:expr, $description:expr, $examples:expr, $handler:expr) => {
        $crate::define_action!($registry, $name, $usage, $description, $examples, $handler);
    };
}

#[cfg(test)]
mod v3_tests {
    use super::*;
    use lla_plugin_sdk::value;

    const MANIFEST: &str = r#"
schema_version = 3

[plugin]
id = "dev.lla.fixture"
name = "fixture"
version = "0.6.0"
api_min = 3
api_max = 3
entrypoint = "fixture"

[[actions]]
id = "copy"
description = "Copy paths"
arguments = [
  { name = "sources", type = "path", description = "Source paths.", position = 0, required = true, repeatable = true },
  { name = "destination", type = "path", description = "Destination path.", option = "--destination", required = true },
  { name = "force", type = "boolean", description = "Overwrite existing files.", option = "--force", default = false },
  { name = "tag", type = "string", description = "Tag to apply.", option = "--tag", repeatable = true },
]
"#;

    #[test]
    fn typed_arguments_reconstruct_existing_cli_shape() {
        let arguments = [
            (
                "sources".to_string(),
                value::list([value::path("one"), value::path("two")]),
            ),
            ("destination".to_string(), value::path("archive")),
            ("force".to_string(), value::boolean(true)),
            (
                "tag".to_string(),
                value::list([value::string("work"), value::string("safe")]),
            ),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            typed_action_arguments_as_strings("copy", &arguments, MANIFEST).unwrap(),
            [
                "one",
                "two",
                "--destination",
                "archive",
                "--force",
                "--tag",
                "work",
                "--tag",
                "safe",
            ]
        );
    }

    #[test]
    fn runtime_action_inventory_comes_from_the_manifest() {
        let actions = manifest_action_infos(MANIFEST);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].name, "copy");
        assert_eq!(
            actions[0].usage,
            "copy <sources>... --destination <destination> [--force] [--tag <tag>...]"
        );
        assert_eq!(actions[0].description, "Copy paths");
    }
}
