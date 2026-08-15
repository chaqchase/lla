use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub mod manifest;
#[doc(hidden)]
pub use prost as prost_runtime;

pub mod proto {
    #[cfg(not(feature = "regenerate-protobuf"))]
    include!("generated/mod.rs");

    #[cfg(feature = "regenerate-protobuf")]
    include!(concat!(env!("OUT_DIR"), "/lla_plugin.rs"));
}

pub trait Plugin: Default {
    fn handle_raw_request(&mut self, request: &[u8]) -> Vec<u8>;
}

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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActionInfo {
    pub name: String,
    pub usage: String,
    pub description: String,
    pub examples: Vec<String>,
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
        proto::EntryMetadata {
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
        EntryMetadata {
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
        proto::DecoratedEntry {
            path: entry.path.to_string_lossy().to_string(),
            metadata: Some(entry.metadata.into()),
            custom_fields: entry.custom_fields,
            typed_fields: HashMap::new(),
        }
    }
}

impl TryFrom<proto::DecoratedEntry> for DecoratedEntry {
    type Error = std::io::Error;

    fn try_from(entry: proto::DecoratedEntry) -> Result<Self, Self::Error> {
        Ok(DecoratedEntry {
            path: PathBuf::from(entry.path),
            metadata: entry.metadata.unwrap_or_default().into(),
            custom_fields: entry.custom_fields,
        })
    }
}

#[repr(C)]
pub struct RawBuffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

impl RawBuffer {
    pub fn from_vec(mut vec: Vec<u8>) -> Self {
        let ptr = vec.as_mut_ptr();
        let len = vec.len();
        let capacity = vec.capacity();
        std::mem::forget(vec);
        RawBuffer { ptr, len, capacity }
    }

    /// # Safety
    ///
    /// The buffer must have been created by `RawBuffer::from_vec`, and it must not have already
    /// been converted back into a `Vec` or otherwise freed.
    pub unsafe fn into_vec(self) -> Vec<u8> {
        Vec::from_raw_parts(self.ptr, self.len, self.capacity)
    }
}

#[repr(C)]
pub struct PluginApi {
    pub version: u32,
    pub handle_request: extern "C" fn(*mut std::ffi::c_void, *const u8, usize) -> RawBuffer,
    pub free_response: extern "C" fn(*mut RawBuffer),
}

pub const LEGACY_PLUGIN_API_VERSION: u32 = 1;
pub const CURRENT_PLUGIN_API_VERSION: u32 = 2;

/// A v2 response buffer is always released by the plugin that allocated it.
/// The host may read `len` bytes from `ptr`, then must call `free_response`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PluginBufferV2 {
    pub ptr: *mut u8,
    pub len: usize,
}

impl PluginBufferV2 {
    pub fn from_vec(vec: Vec<u8>) -> Self {
        let boxed = vec.into_boxed_slice();
        let len = boxed.len();
        let ptr = Box::into_raw(boxed) as *mut u8;
        Self { ptr, len }
    }
}

#[repr(C)]
pub struct PluginApiV2 {
    pub abi_version: u32,
    pub min_host_api: u32,
    pub max_host_api: u32,
    pub handle_request: extern "C" fn(*const u8, usize) -> PluginBufferV2,
    pub free_response: extern "C" fn(PluginBufferV2),
    pub destroy: extern "C" fn(*mut PluginApiV2),
}

#[doc(hidden)]
pub fn encode_plugin_error(error: &str) -> Vec<u8> {
    use prost::Message as _;
    proto::PluginMessage {
        message: Some(proto::plugin_message::Message::ErrorResponse(
            error.to_string(),
        )),
    }
    .encode_to_vec()
}

#[repr(C)]
pub struct PluginContext(*mut std::ffi::c_void);

#[macro_export]
macro_rules! declare_plugin {
    ($plugin_type:ty) => {
        static PLUGIN_INSTANCE: std::sync::OnceLock<std::sync::Mutex<$plugin_type>> =
            std::sync::OnceLock::new();

        #[no_mangle]
        pub extern "C" fn _plugin_create() -> *mut $crate::PluginApi {
            let api = Box::new($crate::PluginApi {
                version: $crate::LEGACY_PLUGIN_API_VERSION,
                handle_request: {
                    extern "C" fn handle_request(
                        _ctx: *mut std::ffi::c_void,
                        request: *const u8,
                        len: usize,
                    ) -> $crate::RawBuffer {
                        if request.is_null() {
                            return $crate::RawBuffer::from_vec(Vec::new());
                        }
                        let request_slice = unsafe { std::slice::from_raw_parts(request, len) };
                        let plugin = PLUGIN_INSTANCE
                            .get_or_init(|| std::sync::Mutex::new(<$plugin_type>::default()));
                        let response = std::panic::catch_unwind(
                            std::panic::AssertUnwindSafe(|| match plugin.lock() {
                                Ok(mut plugin) => plugin.handle_raw_request(request_slice),
                                Err(_) => $crate::encode_plugin_error("plugin state is poisoned"),
                            }),
                        )
                        .unwrap_or_else(|_| $crate::encode_plugin_error("plugin panicked"));
                        $crate::RawBuffer::from_vec(response)
                    }
                    handle_request
                },
                free_response: {
                    extern "C" fn free_response(response: *mut $crate::RawBuffer) {
                        unsafe {
                            let buffer = Box::from_raw(response);
                            drop(Vec::from_raw_parts(buffer.ptr, buffer.len, buffer.capacity));
                        }
                    }
                    free_response
                },
            });
            Box::into_raw(api)
        }

        #[no_mangle]
        pub extern "C" fn _plugin_create_v2() -> *mut $crate::PluginApiV2 {
            extern "C" fn handle_request_v2(
                request: *const u8,
                len: usize,
            ) -> $crate::PluginBufferV2 {
                if request.is_null() {
                    return $crate::PluginBufferV2 {
                        ptr: std::ptr::null_mut(),
                        len: 0,
                    };
                }
                use $crate::prost_runtime::Message as _;
                let request_slice = unsafe { std::slice::from_raw_parts(request, len) };
                let plugin = PLUGIN_INSTANCE
                    .get_or_init(|| std::sync::Mutex::new(<$plugin_type>::default()));
                let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match plugin.lock() {
                        Ok(mut plugin) => match $crate::proto::PluginMessage::decode(request_slice) {
                            Ok($crate::proto::PluginMessage {
                                message: Some(
                                    $crate::proto::plugin_message::Message::DecorateBatch(batch),
                                ),
                            }) => {
                                let mut decorated = Vec::with_capacity(batch.entries.len());
                                for entry in batch.entries {
                                    let original = entry.clone();
                                    let request = $crate::proto::PluginMessage {
                                        message: Some(
                                            $crate::proto::plugin_message::Message::Decorate(entry),
                                        ),
                                    };
                                    let mut request_bytes =
                                        Vec::with_capacity(request.encoded_len());
                                    if request.encode(&mut request_bytes).is_err() {
                                        decorated.push(original);
                                        continue;
                                    }
                                    let response = std::panic::catch_unwind(
                                        std::panic::AssertUnwindSafe(|| {
                                            <$plugin_type as $crate::Plugin>::handle_raw_request(
                                                &mut *plugin,
                                                &request_bytes,
                                            )
                                        }),
                                    )
                                    .unwrap_or_else(|_| {
                                        $crate::encode_plugin_error("plugin panicked")
                                    });
                                    let decorated_entry = if let Ok(response) =
                                        $crate::proto::PluginMessage::decode(&response[..])
                                    {
                                        if let Some(
                                            $crate::proto::plugin_message::Message::DecoratedResponse(
                                                entry,
                                            ),
                                        ) = response.message
                                        {
                                            entry
                                        } else {
                                            original
                                        }
                                    } else {
                                        original
                                    };
                                    decorated.push(decorated_entry);
                                }
                                let response = $crate::proto::PluginMessage {
                                    message: Some(
                                        $crate::proto::plugin_message::Message::DecorateBatchResponse(
                                            $crate::proto::BatchDecorateResponse {
                                                entries: decorated,
                                            },
                                        ),
                                    ),
                                };
                                let mut bytes = Vec::with_capacity(response.encoded_len());
                                if response.encode(&mut bytes).is_ok() {
                                    bytes
                                } else {
                                    $crate::encode_plugin_error(
                                        "failed to encode batch decoration response",
                                    )
                                }
                            }
                            _ => <$plugin_type as $crate::Plugin>::handle_raw_request(
                                &mut *plugin,
                                request_slice,
                            ),
                        },
                        Err(_) => $crate::encode_plugin_error("plugin state is poisoned"),
                    }
                }))
                .unwrap_or_else(|_| $crate::encode_plugin_error("plugin panicked"));
                $crate::PluginBufferV2::from_vec(response)
            }

            extern "C" fn free_response_v2(buffer: $crate::PluginBufferV2) {
                if buffer.ptr.is_null() {
                    return;
                }
                unsafe {
                    let slice = std::ptr::slice_from_raw_parts_mut(buffer.ptr, buffer.len);
                    drop(Box::from_raw(slice));
                }
            }

            extern "C" fn destroy_v2(api: *mut $crate::PluginApiV2) {
                if !api.is_null() {
                    unsafe {
                        drop(Box::from_raw(api));
                    }
                }
            }

            Box::into_raw(Box::new($crate::PluginApiV2 {
                abi_version: $crate::CURRENT_PLUGIN_API_VERSION,
                min_host_api: $crate::CURRENT_PLUGIN_API_VERSION,
                max_host_api: $crate::CURRENT_PLUGIN_API_VERSION,
                handle_request: handle_request_v2,
                free_response: free_response_v2,
                destroy: destroy_v2,
            }))
        }
    };
}

#[cfg(test)]
mod v2_tests {
    use super::*;
    use prost::Message as _;

    #[derive(Default)]
    struct EchoPlugin;

    impl Plugin for EchoPlugin {
        fn handle_raw_request(&mut self, request: &[u8]) -> Vec<u8> {
            let request = proto::PluginMessage::decode(request).unwrap();
            let message = match request.message {
                Some(proto::plugin_message::Message::Decorate(mut entry)) => {
                    if entry.path == "panic" {
                        panic!("intentional test panic");
                    }
                    if entry.path == "malformed" {
                        return vec![0xff];
                    }
                    entry
                        .custom_fields
                        .insert("echo".to_string(), "ok".to_string());
                    proto::plugin_message::Message::DecoratedResponse(entry)
                }
                _ => proto::plugin_message::Message::ErrorResponse("unsupported".to_string()),
            };
            let response = proto::PluginMessage {
                message: Some(message),
            };
            response.encode_to_vec()
        }
    }

    declare_plugin!(EchoPlugin);

    #[test]
    fn v2_preserves_legacy_non_batch_request_behavior() {
        let request = proto::PluginMessage {
            message: Some(proto::plugin_message::Message::Decorate(
                proto::DecoratedEntry {
                    path: "README.md".to_string(),
                    metadata: Some(proto::EntryMetadata::default()),
                    custom_fields: HashMap::new(),
                    typed_fields: HashMap::new(),
                },
            )),
        }
        .encode_to_vec();

        let v1 = _plugin_create();
        let v1_response = unsafe {
            ((*v1).handle_request)(std::ptr::null_mut(), request.as_ptr(), request.len()).into_vec()
        };
        unsafe { drop(Box::from_raw(v1)) };

        let v2 = _plugin_create_v2();
        let v2_buffer = unsafe { ((*v2).handle_request)(request.as_ptr(), request.len()) };
        let v2_response =
            unsafe { std::slice::from_raw_parts(v2_buffer.ptr, v2_buffer.len).to_vec() };
        unsafe {
            ((*v2).free_response)(v2_buffer);
            ((*v2).destroy)(v2);
        }

        assert_eq!(v2_response, v1_response);
    }

    #[test]
    fn v2_batch_adapter_returns_plugin_owned_buffer() {
        let api = _plugin_create_v2();
        assert!(!api.is_null());
        let request = proto::PluginMessage {
            message: Some(proto::plugin_message::Message::DecorateBatch(
                proto::BatchDecorateRequest {
                    entries: vec![
                        proto::DecoratedEntry {
                            path: "malformed".to_string(),
                            metadata: Some(proto::EntryMetadata::default()),
                            custom_fields: HashMap::new(),
                            typed_fields: HashMap::new(),
                        },
                        proto::DecoratedEntry {
                            path: "panic".to_string(),
                            metadata: Some(proto::EntryMetadata::default()),
                            custom_fields: HashMap::new(),
                            typed_fields: HashMap::new(),
                        },
                        proto::DecoratedEntry {
                            path: "README.md".to_string(),
                            metadata: Some(proto::EntryMetadata::default()),
                            custom_fields: HashMap::new(),
                            typed_fields: HashMap::new(),
                        },
                    ],
                    format: "default".to_string(),
                },
            )),
        }
        .encode_to_vec();

        let response = unsafe { ((*api).handle_request)(request.as_ptr(), request.len()) };
        assert!(!response.ptr.is_null());
        let bytes = unsafe { std::slice::from_raw_parts(response.ptr, response.len).to_vec() };
        unsafe { ((*api).free_response)(response) };

        let response = proto::PluginMessage::decode(&bytes[..]).unwrap();
        let Some(proto::plugin_message::Message::DecorateBatchResponse(batch)) = response.message
        else {
            panic!("expected batch response");
        };
        assert_eq!(batch.entries.len(), 3);
        assert_eq!(batch.entries[0].path, "malformed");
        assert!(batch.entries[0].custom_fields.is_empty());
        assert_eq!(batch.entries[1].path, "panic");
        assert!(batch.entries[1].custom_fields.is_empty());
        assert_eq!(batch.entries[2].custom_fields["echo"], "ok");

        unsafe { ((*api).destroy)(api) };
    }
}
