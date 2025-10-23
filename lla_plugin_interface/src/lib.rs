use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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

pub const CURRENT_PLUGIN_API_VERSION: u32 = 1;

#[repr(C)]
pub struct PluginContext(*mut std::ffi::c_void);

#[macro_export]
macro_rules! declare_plugin {
    ($plugin_type:ty) => {
        static mut PLUGIN_INSTANCE: Option<$plugin_type> = None;

        #[no_mangle]
        pub extern "C" fn _plugin_create() -> *mut $crate::PluginApi {
            let api = Box::new($crate::PluginApi {
                version: $crate::CURRENT_PLUGIN_API_VERSION,
                handle_request: {
                    extern "C" fn handle_request(
                        _ctx: *mut std::ffi::c_void,
                        request: *const u8,
                        len: usize,
                    ) -> $crate::RawBuffer {
                        unsafe {
                            if PLUGIN_INSTANCE.is_none() {
                                PLUGIN_INSTANCE = Some(<$plugin_type>::default());
                            }
                            let plugin = PLUGIN_INSTANCE.as_mut().unwrap();
                            let request_slice = std::slice::from_raw_parts(request, len);
                            let response = plugin.handle_raw_request(request_slice);
                            $crate::RawBuffer::from_vec(response)
                        }
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
    };
}
