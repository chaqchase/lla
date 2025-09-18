pub mod actions;
pub mod config;
pub mod format;
pub mod syntax;
pub mod ui;

pub use actions::{Action, ActionHelp, ActionRegistry};
pub use config::{ConfigManager, PluginConfig};
pub use syntax::CodeHighlighter;
pub use ui::{
    components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
    TextBlock, TextStyle,
};

use lla_plugin_interface::{proto, ConfigRequest, DecoratedEntry, PluginRequest, PluginResponse};

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
                let metadata = entry
                    .metadata
                    .map(|m| lla_plugin_interface::EntryMetadata {
                        size: m.size,
                        modified: m.modified,
                        accessed: m.accessed,
                        created: m.created,
                        is_dir: m.is_dir,
                        is_file: m.is_file,
                        is_symlink: m.is_symlink,
                        permissions: m.permissions,
                        uid: m.uid,
                        gid: m.gid,
                    })
                    .ok_or("Missing metadata in decorated entry")?;

                let decorated = lla_plugin_interface::DecoratedEntry {
                    path: std::path::PathBuf::from(entry.path),
                    metadata,
                    custom_fields: entry.custom_fields,
                    field_types: entry.field_types.into_iter().map(|(k, v)| (k, v.into())).collect(),
                };
                Ok(PluginRequest::Decorate(decorated))
            }
            Some(proto::plugin_message::Message::FormatField(req)) => {
                let entry = req.entry.ok_or("Missing entry in format field request")?;
                let metadata = entry
                    .metadata
                    .map(|m| lla_plugin_interface::EntryMetadata {
                        size: m.size,
                        modified: m.modified,
                        accessed: m.accessed,
                        created: m.created,
                        is_dir: m.is_dir,
                        is_file: m.is_file,
                        is_symlink: m.is_symlink,
                        permissions: m.permissions,
                        uid: m.uid,
                        gid: m.gid,
                    })
                    .ok_or("Missing metadata in decorated entry")?;

                let decorated = lla_plugin_interface::DecoratedEntry {
                    path: std::path::PathBuf::from(entry.path),
                    metadata,
                    custom_fields: entry.custom_fields,
                    field_types: entry.field_types.into_iter().map(|(k, v)| (k, v.into())).collect(),
                };
                Ok(PluginRequest::FormatField(decorated, req.format))
            }
            Some(proto::plugin_message::Message::Action(req)) => {
                Ok(PluginRequest::PerformAction(req.action, req.args))
            }
            Some(proto::plugin_message::Message::BatchDecorate(batch_req)) => {
                let mut entries = Vec::new();
                for entry in batch_req.entries {
                    match DecoratedEntry::try_from(entry) {
                        Ok(e) => entries.push(e),
                        Err(_) => continue,
                    }
                }
                Ok(PluginRequest::BatchDecorate(entries, batch_req.format))
            }
            Some(proto::plugin_message::Message::Config(config_req)) => {
                Ok(PluginRequest::Config(ConfigRequest {
                    config: config_req.config,
                    theme: config_req.theme,
                    shortcuts: config_req.shortcuts,
                }))
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
                let proto_metadata = proto::EntryMetadata {
                    size: entry.metadata.size,
                    modified: entry.metadata.modified,
                    accessed: entry.metadata.accessed,
                    created: entry.metadata.created,
                    is_dir: entry.metadata.is_dir,
                    is_file: entry.metadata.is_file,
                    is_symlink: entry.metadata.is_symlink,
                    permissions: entry.metadata.permissions,
                    uid: entry.metadata.uid,
                    gid: entry.metadata.gid,
                };

                let proto_entry = proto::DecoratedEntry {
                    path: entry.path.to_string_lossy().to_string(),
                    metadata: Some(proto_metadata),
                    custom_fields: entry.custom_fields,
                    field_types: entry.field_types.into_iter().map(|(k, v)| (k, v.into())).collect(),
                };
                proto::plugin_message::Message::DecoratedResponse(proto_entry)
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
                }),
                Err(e) => proto::plugin_message::Message::ActionResponse(proto::ActionResponse {
                    success: false,
                    error: Some(e),
                }),
            },
            PluginResponse::BatchDecorated(entries) => {
                let proto_entries: Vec<proto::DecoratedEntry> = entries
                    .into_iter()
                    .map(|entry| proto::DecoratedEntry {
                        path: entry.path.to_string_lossy().to_string(),
                        metadata: Some(proto::EntryMetadata {
                            size: entry.metadata.size,
                            modified: entry.metadata.modified,
                            accessed: entry.metadata.accessed,
                            created: entry.metadata.created,
                            is_dir: entry.metadata.is_dir,
                            is_file: entry.metadata.is_file,
                            is_symlink: entry.metadata.is_symlink,
                            permissions: entry.metadata.permissions,
                            uid: entry.metadata.uid,
                            gid: entry.metadata.gid,
                        }),
                        custom_fields: entry.custom_fields,
                        field_types: entry.field_types.into_iter().map(|(k, v)| (k, v.into())).collect(),
                    })
                    .collect();

                proto::plugin_message::Message::BatchDecoratedResponse(proto::BatchDecorateResponse {
                    entries: proto_entries,
                })
            }
            PluginResponse::ConfigResult(result) => match result {
                Ok(()) => proto::plugin_message::Message::ConfigResponse(proto::ConfigResponse {
                    success: true,
                    error: None,
                }),
                Err(e) => proto::plugin_message::Message::ConfigResponse(proto::ConfigResponse {
                    success: false,
                    error: Some(e),
                }),
            },
            PluginResponse::Error(e) => proto::plugin_message::Message::ErrorResponse(e),
        };

        let proto_msg = proto::PluginMessage {
            message: Some(response_msg),
        };
        let mut buf = bytes::BytesMut::with_capacity(proto_msg.encoded_len());
        proto_msg.encode(&mut buf).unwrap();
        buf.to_vec()
    }

    fn encode_error(&self, error: &str) -> Vec<u8> {
        use prost::Message;
        let error_msg = proto::PluginMessage {
            message: Some(proto::plugin_message::Message::ErrorResponse(
                error.to_string(),
            )),
        };
        let mut buf = bytes::BytesMut::with_capacity(error_msg.encoded_len());
        error_msg.encode(&mut buf).unwrap();
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

        lla_plugin_interface::declare_plugin!($plugin);
    };
}
