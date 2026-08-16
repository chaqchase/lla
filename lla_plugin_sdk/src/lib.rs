//! Maintained Rust SDK for lla plugin API v3.

use lla_plugin_interface::proto::{self, plugin_message};
use prost::Message as _;
use std::collections::HashMap;

pub use lla_plugin_interface as interface;
#[cfg(feature = "component")]
pub use lla_plugin_sdk_macros::export_component;
pub use lla_plugin_sdk_macros::export_plugin;
#[cfg(feature = "component")]
#[doc(hidden)]
pub use wit_bindgen;
#[cfg(feature = "component")]
#[doc(hidden)]
pub use wit_bindgen::{resource, rt};

pub type ActionArguments = HashMap<String, proto::TypedValue>;

/// High-level API implemented by native plugins.
///
/// Existing protobuf-based plugins can implement `handle_message` while they
/// migrate. New plugins should override `decorate_entry`, `decorate_batch`,
/// `format_field`, `registered_actions`, and `run_action` as needed. These
/// methods avoid wire encoding and support a true one-call batch.
pub trait Plugin: Default + Send + 'static {
    fn handle_message(&mut self, message: proto::PluginMessage) -> proto::PluginMessage {
        let bytes = self.handle_raw_request(&message.encode_to_vec());
        proto::PluginMessage::decode(bytes.as_slice()).unwrap_or_else(|error| {
            proto::PluginMessage {
                message: Some(plugin_message::Message::ErrorResponse(format!(
                    "plugin returned an invalid response: {error}"
                ))),
            }
        })
    }

    /// Transitional source adapter for plugins written before the high-level
    /// SDK. This is not part of the native ABI and will be removed in a later
    /// SDK major version.
    #[doc(hidden)]
    fn handle_raw_request(&mut self, _request: &[u8]) -> Vec<u8> {
        lla_plugin_interface::encode_plugin_error("plugin request is not implemented")
    }

    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        let original = entry.clone();
        match self
            .handle_message(proto::PluginMessage {
                message: Some(plugin_message::Message::Decorate(entry)),
            })
            .message
        {
            Some(plugin_message::Message::DecoratedResponse(entry)) => entry,
            _ => original,
        }
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        entries
            .into_iter()
            .map(|entry| self.decorate_entry(entry))
            .collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        match self
            .handle_message(proto::PluginMessage {
                message: Some(plugin_message::Message::FormatField(
                    proto::FormatFieldRequest {
                        entry: Some(entry),
                        format,
                    },
                )),
            })
            .message
        {
            Some(plugin_message::Message::FieldResponse(response)) => response.field,
            _ => None,
        }
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        match self
            .handle_message(proto::PluginMessage {
                message: Some(plugin_message::Message::Action(proto::ActionRequest {
                    action,
                    arguments,
                })),
            })
            .message
        {
            Some(plugin_message::Message::ActionResponse(response)) => response,
            Some(plugin_message::Message::ErrorResponse(error)) => proto::ActionResponse {
                success: false,
                error: Some(error),
                output: None,
                structured_error: None,
            },
            Some(plugin_message::Message::StructuredErrorResponse(error)) => {
                proto::ActionResponse {
                    success: false,
                    error: Some(error.message.clone()),
                    output: None,
                    structured_error: Some(error),
                }
            }
            _ => proto::ActionResponse {
                success: false,
                error: Some("plugin returned an invalid action response".to_string()),
                output: None,
                structured_error: Some(proto::PluginError {
                    code: "invalid-action-response".to_string(),
                    message: "plugin returned an invalid action response".to_string(),
                    details: HashMap::new(),
                }),
            },
        }
    }

    /// Declares the action handlers registered by this plugin. The host checks
    /// these IDs against `plugin.toml` during installation and `plugin doctor`.
    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        match self
            .handle_message(proto::PluginMessage {
                message: Some(plugin_message::Message::ListActions(true)),
            })
            .message
        {
            Some(plugin_message::Message::ListActionsResponse(actions)) => actions.actions,
            _ => Vec::new(),
        }
    }
}

#[doc(hidden)]
pub fn dispatch<P: Plugin>(plugin: &mut P, bytes: &[u8]) -> Vec<u8> {
    let message = match proto::PluginMessage::decode(bytes) {
        Ok(message) => message,
        Err(error) => return lla_plugin_interface::encode_plugin_error(&error.to_string()),
    };
    let response = match message.message {
        Some(plugin_message::Message::Decorate(entry)) => proto::PluginMessage {
            message: Some(plugin_message::Message::DecoratedResponse(
                plugin.decorate_entry(entry),
            )),
        },
        Some(plugin_message::Message::DecorateBatch(batch)) => proto::PluginMessage {
            message: if batch.entries.len() > lla_plugin_interface::MAX_BATCH_ENTRIES {
                Some(plugin_message::Message::StructuredErrorResponse(
                    proto::PluginError {
                        code: "batch-limit-exceeded".to_string(),
                        message: format!(
                            "batch has {} entries; maximum is {}",
                            batch.entries.len(),
                            lla_plugin_interface::MAX_BATCH_ENTRIES
                        ),
                        details: HashMap::new(),
                    },
                ))
            } else {
                Some(plugin_message::Message::DecorateBatchResponse(
                    proto::BatchDecorateResponse {
                        entries: plugin.decorate_batch(batch.entries, &batch.format),
                    },
                ))
            },
        },
        Some(plugin_message::Message::FormatField(request)) => proto::PluginMessage {
            message: Some(plugin_message::Message::FieldResponse(
                proto::FormattedFieldResponse {
                    field: request
                        .entry
                        .and_then(|entry| plugin.format_field(entry, request.format)),
                },
            )),
        },
        Some(plugin_message::Message::Action(action)) => proto::PluginMessage {
            message: Some(plugin_message::Message::ActionResponse(
                plugin.run_action(action.action, action.arguments),
            )),
        },
        Some(plugin_message::Message::ListActions(_)) => proto::PluginMessage {
            message: Some(plugin_message::Message::ListActionsResponse(
                proto::ListActionsResponse {
                    actions: plugin.registered_actions(),
                },
            )),
        },
        message => plugin.handle_message(proto::PluginMessage { message }),
    };
    response.encode_to_vec()
}

#[doc(hidden)]
#[macro_export]
macro_rules! __export_plugin_v3 {
    ($plugin_type:ty, $manifest:expr) => {
        const _: () = {
            static MANIFEST: &str = $manifest;

            extern "C" fn handle_request(
                context: *mut std::ffi::c_void,
                request: *const u8,
                len: usize,
            ) -> ::lla_plugin_sdk::interface::PluginBufferV3 {
                if context.is_null() || request.is_null() {
                    return ::lla_plugin_sdk::interface::PluginBufferV3::empty();
                }
                let plugin = unsafe { &*(context as *const std::sync::Mutex<$plugin_type>) };
                let request = unsafe { std::slice::from_raw_parts(request, len) };
                let response =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        match plugin.lock() {
                            Ok(mut plugin) => ::lla_plugin_sdk::dispatch(&mut *plugin, request),
                            Err(_) => ::lla_plugin_sdk::interface::encode_plugin_error(
                                "plugin state is poisoned",
                            ),
                        }
                    }))
                    .unwrap_or_else(|_| {
                        ::lla_plugin_sdk::interface::encode_plugin_error("plugin panicked")
                    });
                ::lla_plugin_sdk::interface::PluginBufferV3::from_vec(response)
            }

            extern "C" fn free_response(buffer: ::lla_plugin_sdk::interface::PluginBufferV3) {
                unsafe { ::lla_plugin_sdk::interface::free_plugin_buffer(buffer) }
            }

            extern "C" fn destroy(api: *mut ::lla_plugin_sdk::interface::PluginApiV3) {
                if api.is_null() {
                    return;
                }
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    let api = Box::from_raw(api);
                    if !api.context.is_null() {
                        drop(Box::from_raw(
                            api.context as *mut std::sync::Mutex<$plugin_type>,
                        ));
                    }
                }));
            }

            #[no_mangle]
            pub extern "C" fn _plugin_create_v3() -> *mut ::lla_plugin_sdk::interface::PluginApiV3 {
                let plugin = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    <$plugin_type as Default>::default()
                })) {
                    Ok(plugin) => plugin,
                    Err(_) => return std::ptr::null_mut(),
                };
                let context =
                    Box::into_raw(Box::new(std::sync::Mutex::new(plugin))) as *mut std::ffi::c_void;
                Box::into_raw(Box::new(::lla_plugin_sdk::interface::PluginApiV3 {
                    abi_version: ::lla_plugin_sdk::interface::PLUGIN_API_VERSION,
                    min_host_api: ::lla_plugin_sdk::interface::PLUGIN_API_VERSION,
                    max_host_api: ::lla_plugin_sdk::interface::PLUGIN_API_VERSION,
                    manifest_ptr: MANIFEST.as_ptr(),
                    manifest_len: MANIFEST.len(),
                    handle_request,
                    free_response,
                    destroy,
                    context,
                }))
            }
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use lla_plugin_interface::proto::plugin_message::Message;

    #[derive(Default)]
    struct BatchPlugin {
        batch_calls: usize,
    }

    impl Plugin for BatchPlugin {
        fn handle_message(&mut self, message: proto::PluginMessage) -> proto::PluginMessage {
            proto::PluginMessage {
                message: message.message,
            }
        }

        fn decorate_entry(&mut self, mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
            entry.custom_fields.insert("mode".into(), "single".into());
            entry
        }

        fn decorate_batch(
            &mut self,
            mut entries: Vec<proto::DecoratedEntry>,
            _format: &str,
        ) -> Vec<proto::DecoratedEntry> {
            self.batch_calls += 1;
            for entry in &mut entries {
                entry.custom_fields.insert("mode".into(), "batch".into());
            }
            entries
        }
    }

    #[test]
    fn native_batch_override_executes_once() {
        let entries = (0..8)
            .map(|index| proto::DecoratedEntry {
                path: format!("file-{index}"),
                ..Default::default()
            })
            .collect();
        let request = proto::PluginMessage {
            message: Some(Message::DecorateBatch(proto::BatchDecorateRequest {
                entries,
                format: "default".into(),
            })),
        }
        .encode_to_vec();
        let mut plugin = BatchPlugin::default();
        let response =
            proto::PluginMessage::decode(dispatch(&mut plugin, &request).as_slice()).unwrap();
        assert_eq!(plugin.batch_calls, 1);
        let Some(Message::DecorateBatchResponse(response)) = response.message else {
            panic!("expected batch response");
        };
        assert!(response
            .entries
            .iter()
            .all(|entry| entry.custom_fields["mode"] == "batch"));
    }

    #[test]
    fn response_buffer_remains_plugin_owned_until_free() {
        let buffer = interface::PluginBufferV3::from_vec(vec![1, 2, 3, 4]);
        assert!(!buffer.ptr.is_null());
        assert_eq!(buffer.len, 4);
        unsafe { interface::free_plugin_buffer(buffer) };
    }

    #[derive(Default)]
    struct TypedActionPlugin;

    impl Plugin for TypedActionPlugin {
        fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
            vec![proto::ActionInfo {
                name: "echo".into(),
                usage: "<value>".into(),
                description: "Return a typed value".into(),
                examples: vec!["lla plugin run fixture echo -- hello".into()],
            }]
        }

        fn run_action(
            &mut self,
            _action: String,
            arguments: ActionArguments,
        ) -> proto::ActionResponse {
            proto::ActionResponse {
                success: true,
                error: None,
                output: Some(proto::ActionOutput {
                    output: arguments
                        .get("value")
                        .cloned()
                        .map(proto::action_output::Output::Value),
                }),
                structured_error: None,
            }
        }
    }

    #[test]
    fn typed_actions_dispatch_without_wire_level_plugin_code() {
        let mut plugin = TypedActionPlugin;
        let list = proto::PluginMessage {
            message: Some(Message::ListActions(true)),
        }
        .encode_to_vec();
        let list = proto::PluginMessage::decode(dispatch(&mut plugin, &list).as_slice()).unwrap();
        assert!(matches!(
            list.message,
            Some(Message::ListActionsResponse(proto::ListActionsResponse { actions }))
                if actions.len() == 1 && actions[0].name == "echo"
        ));

        let value = proto::TypedValue {
            value: Some(proto::typed_value::Value::StringValue("hello".into())),
        };
        let action = proto::PluginMessage {
            message: Some(Message::Action(proto::ActionRequest {
                action: "echo".into(),
                arguments: [("value".to_string(), value.clone())].into_iter().collect(),
            })),
        }
        .encode_to_vec();
        let response =
            proto::PluginMessage::decode(dispatch(&mut plugin, &action).as_slice()).unwrap();
        assert!(matches!(
            response.message,
            Some(Message::ActionResponse(proto::ActionResponse {
                output: Some(proto::ActionOutput {
                    output: Some(proto::action_output::Output::Value(returned)),
                }),
                ..
            })) if returned == value
        ));
    }

    #[test]
    fn published_wit_world_matches_component_export_macro() {
        assert_eq!(
            include_str!("../wit/lla-plugin.wit"),
            include_str!("../../lla_plugin_sdk_macros/wit/lla-plugin.wit")
        );
    }
}
