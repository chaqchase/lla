pub mod actions;
pub mod config;
pub mod format;
pub mod syntax;
pub mod trash;
pub mod ui;

pub use actions::{Action, ActionHelp, ActionRegistry};
pub use config::{ConfigManager, PluginConfig};
pub use syntax::CodeHighlighter;
pub use ui::{
    components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
    TextBlock, TextStyle,
};

use lla_plugin_interface::proto;
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

#[derive(Serialize, Deserialize)]
pub enum PluginRequest {
    GetName,
    GetVersion,
    GetDescription,
    GetSupportedFormats,
    Decorate(DecoratedEntry),
    FormatField(DecoratedEntry, String),
    PerformAction(String, Vec<String>),
    GetAvailableActions,
}

#[derive(Serialize, Deserialize)]
pub enum PluginResponse {
    Name(String),
    Version(String),
    Description(String),
    SupportedFormats(Vec<String>),
    Decorated(DecoratedEntry),
    FormattedField(Option<String>),
    ActionResult(Result<(), String>),
    AvailableActions(Vec<ActionInfo>),
    Error(String),
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

fn value_as_strings(value: proto::TypedValue) -> Vec<String> {
    use proto::typed_value::Value;
    match value.value {
        Some(Value::StringValue(value) | Value::PathValue(value)) => vec![value],
        Some(Value::IntegerValue(value)) => vec![value.to_string()],
        Some(Value::FloatValue(value)) => vec![value.to_string()],
        Some(Value::BooleanValue(value)) => vec![value.to_string()],
        Some(Value::BytesValue(value) | Value::TimestampValue(value)) => vec![value.to_string()],
        Some(Value::ListValue(values)) => values
            .values
            .into_iter()
            .flat_map(value_as_strings)
            .collect(),
        Some(Value::ObjectValue(_)) | Some(Value::NullValue(_)) | None => Vec::new(),
    }
}

fn action_arguments_as_strings(
    arguments: std::collections::HashMap<String, proto::TypedValue>,
) -> Vec<String> {
    if let Some(arguments) = arguments.get("args").cloned() {
        return value_as_strings(arguments);
    }
    let mut arguments = arguments.into_iter().collect::<Vec<_>>();
    arguments.sort_by(|(left, _), (right, _)| {
        let left_index = left.parse::<usize>().unwrap_or(usize::MAX);
        let right_index = right.parse::<usize>().unwrap_or(usize::MAX);
        left_index.cmp(&right_index).then_with(|| left.cmp(right))
    });
    arguments
        .into_iter()
        .flat_map(|(_, value)| value_as_strings(value))
        .collect()
}

fn decode_decorated_entry(entry: proto::DecoratedEntry) -> Result<DecoratedEntry, String> {
    let metadata = entry
        .metadata
        .ok_or_else(|| "Missing metadata in decorated entry".to_string())?;
    Ok(DecoratedEntry {
        path: std::path::PathBuf::from(entry.path),
        metadata: metadata.into(),
        custom_fields: entry.custom_fields,
    })
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

pub trait ProtobufHandler {
    fn decode_request(&self, request: &[u8]) -> Result<PluginRequest, String> {
        use prost::Message;
        let proto_msg = proto::PluginMessage::decode(request)
            .map_err(|e| format!("Failed to decode request: {}", e))?;

        match proto_msg.message {
            Some(proto::plugin_message::Message::GetName(_)) => Ok(PluginRequest::GetName),
            Some(proto::plugin_message::Message::GetVersion(_)) => Ok(PluginRequest::GetVersion),
            Some(proto::plugin_message::Message::GetDescription(_)) => {
                Ok(PluginRequest::GetDescription)
            }
            Some(proto::plugin_message::Message::GetSupportedFormats(_)) => {
                Ok(PluginRequest::GetSupportedFormats)
            }
            Some(proto::plugin_message::Message::Decorate(entry)) => {
                Ok(PluginRequest::Decorate(decode_decorated_entry(entry)?))
            }
            Some(proto::plugin_message::Message::FormatField(req)) => {
                let entry = req.entry.ok_or("Missing entry in format field request")?;
                Ok(PluginRequest::FormatField(
                    decode_decorated_entry(entry)?,
                    req.format,
                ))
            }
            Some(proto::plugin_message::Message::Action(req)) => Ok(PluginRequest::PerformAction(
                req.action,
                action_arguments_as_strings(req.arguments),
            )),
            Some(proto::plugin_message::Message::ListActions(_)) => {
                Ok(PluginRequest::GetAvailableActions)
            }
            _ => Err("Invalid request type".to_string()),
        }
    }

    fn encode_response(&self, response: PluginResponse) -> Vec<u8> {
        use prost::Message;
        let response_msg = match response {
            PluginResponse::Name(name) => proto::plugin_message::Message::NameResponse(name),
            PluginResponse::Version(version) => {
                proto::plugin_message::Message::VersionResponse(version)
            }
            PluginResponse::Description(desc) => {
                proto::plugin_message::Message::DescriptionResponse(desc)
            }
            PluginResponse::SupportedFormats(formats) => {
                proto::plugin_message::Message::FormatsResponse(proto::SupportedFormatsResponse {
                    formats,
                })
            }
            PluginResponse::Decorated(entry) => {
                proto::plugin_message::Message::DecoratedResponse(entry.into())
            }
            PluginResponse::FormattedField(field) => {
                proto::plugin_message::Message::FieldResponse(proto::FormattedFieldResponse {
                    field,
                })
            }
            PluginResponse::ActionResult(result) => match result {
                Ok(()) => proto::plugin_message::Message::ActionResponse(proto::ActionResponse {
                    success: true,
                    error: None,
                    output: Some(proto::ActionOutput {
                        output: Some(proto::action_output::Output::None(true)),
                    }),
                    structured_error: None,
                }),
                Err(e) => proto::plugin_message::Message::ActionResponse(proto::ActionResponse {
                    success: false,
                    error: Some(e),
                    output: None,
                    structured_error: None,
                }),
            },
            PluginResponse::AvailableActions(actions) => {
                let proto_actions: Vec<proto::ActionInfo> = actions
                    .into_iter()
                    .map(|action| proto::ActionInfo {
                        name: action.name,
                        usage: action.usage,
                        description: action.description,
                        examples: action.examples,
                    })
                    .collect();
                proto::plugin_message::Message::ListActionsResponse(proto::ListActionsResponse {
                    actions: proto_actions,
                })
            }
            PluginResponse::Error(e) => proto::plugin_message::Message::ErrorResponse(e),
        };

        let proto_msg = proto::PluginMessage {
            message: Some(response_msg),
        };
        let mut buf = bytes::BytesMut::with_capacity(proto_msg.encoded_len());
        proto_msg
            .encode(&mut buf)
            .map(|_| buf.to_vec())
            .unwrap_or_else(|e| {
                self.encode_error(&format!("failed to encode plugin response: {}", e))
            })
    }

    fn encode_error(&self, error: &str) -> Vec<u8> {
        use prost::Message;
        let error_msg = proto::PluginMessage {
            message: Some(proto::plugin_message::Message::ErrorResponse(
                error.to_string(),
            )),
        };
        let mut buf = bytes::BytesMut::with_capacity(error_msg.encoded_len());
        if error_msg.encode(&mut buf).is_err() {
            return Vec::new();
        }
        buf.to_vec()
    }
}

#[macro_export]
macro_rules! plugin_action {
    ($registry:expr, $name:expr, $usage:expr, $description:expr, $examples:expr, $handler:expr) => {
        $crate::define_action!($registry, $name, $usage, $description, $examples, $handler);
    };
}

#[macro_export]
macro_rules! create_plugin {
    ($plugin:ty) => {
        impl Default for $plugin {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $crate::ConfigurablePlugin for $plugin {
            type Config = <$plugin as std::ops::Deref>::Target;

            fn config(&self) -> &Self::Config {
                self.base.config()
            }

            fn config_mut(&mut self) -> &mut Self::Config {
                self.base.config_mut()
            }
        }

        impl $crate::ProtobufHandler for $plugin {}

        lla_plugin_sdk::export_plugin!($plugin);
    };
}
