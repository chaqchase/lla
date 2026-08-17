use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod manifest;
#[doc(hidden)]
pub use prost as prost_runtime;

pub mod proto {
    #[cfg(not(feature = "regenerate-protobuf"))]
    include!("generated/mod.rs");

    #[cfg(feature = "regenerate-protobuf")]
    include!(concat!(env!("OUT_DIR"), "/lla_plugin.rs"));
}

pub const PLUGIN_API_VERSION: u32 = 3;
pub const PLUGIN_CREATE_SYMBOL_V3: &[u8] = b"_plugin_create_v3\0";
pub const MAX_BATCH_ENTRIES: usize = 512;
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// A response allocated by a plugin. The host may read `len` bytes, then must
/// return the buffer to the same plugin through `PluginApiV3::free_response`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PluginBufferV3 {
    pub ptr: *mut u8,
    pub len: usize,
}

impl PluginBufferV3 {
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        let bytes = bytes.into_boxed_slice();
        let len = bytes.len();
        let ptr = Box::into_raw(bytes).cast::<u8>();
        Self { ptr, len }
    }

    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
        }
    }
}

/// Stable native ABI for plugin contract v3.
///
/// The manifest pointer addresses immutable bytes embedded in the plugin and is
/// valid until `destroy` is called. Responses remain owned by the plugin.
#[repr(C)]
pub struct PluginApiV3 {
    pub abi_version: u32,
    pub min_host_api: u32,
    pub max_host_api: u32,
    pub manifest_ptr: *const u8,
    pub manifest_len: usize,
    pub handle_request: extern "C" fn(*mut std::ffi::c_void, *const u8, usize) -> PluginBufferV3,
    pub free_response: extern "C" fn(PluginBufferV3),
    pub destroy: extern "C" fn(*mut PluginApiV3),
    pub context: *mut std::ffi::c_void,
}

/// Release a buffer previously created with `PluginBufferV3::from_vec`.
///
/// This helper is primarily used by SDK-generated exports.
///
/// # Safety
///
/// `buffer` must originate from `PluginBufferV3::from_vec` and must be freed
/// exactly once.
pub unsafe fn free_plugin_buffer(buffer: PluginBufferV3) {
    if buffer.ptr.is_null() {
        return;
    }
    let slice = std::ptr::slice_from_raw_parts_mut(buffer.ptr, buffer.len);
    drop(Box::from_raw(slice));
}

pub fn encode_plugin_error(error: &str) -> Vec<u8> {
    encode_plugin_error_code("plugin-error", error)
}

pub fn encode_plugin_error_code(code: &str, error: &str) -> Vec<u8> {
    use prost::Message as _;
    proto::PluginMessage {
        message: Some(proto::plugin_message::Message::StructuredErrorResponse(
            proto::PluginError {
                code: code.to_string(),
                message: error.to_string(),
                details: HashMap::new(),
            },
        )),
    }
    .encode_to_vec()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActionInfo {
    pub name: String,
    pub usage: String,
    pub description: String,
    pub examples: Vec<String>,
}
