use crate::config::Config;
use crate::error::{LlaError, Result};
use dashmap::DashMap;
use libloading::Library;
use lla_plugin_interface::{
    proto::{self, plugin_message::Message, PluginMessage},
    PluginApi, CURRENT_PLUGIN_API_VERSION,
};
use once_cell::sync::Lazy;
use prost::Message as _;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

type DecorationCache = DashMap<(String, String), HashMap<String, String>>;
static DECORATION_CACHE: Lazy<DecorationCache> = Lazy::new(DashMap::new);

pub struct PluginManager {
    plugins: HashMap<String, (Library, *mut PluginApi)>,
    loaded_paths: HashSet<PathBuf>,
    pub enabled_plugins: HashSet<String>,
    config: Config,
}

impl PluginManager {
    pub fn new(config: Config) -> Self {
        let enabled_plugins = HashSet::from_iter(config.enabled_plugins.clone());
        PluginManager {
            plugins: HashMap::new(),
            loaded_paths: HashSet::new(),
            enabled_plugins,
            config,
        }
    }

    fn _convert_metadata(metadata: &std::fs::Metadata) -> proto::EntryMetadata {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            proto::EntryMetadata {
                size: metadata.len(),
                modified: metadata
                    .modified()
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    })
                    .unwrap_or(0),
                accessed: metadata
                    .accessed()
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    })
                    .unwrap_or(0),
                created: metadata
                    .created()
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    })
                    .unwrap_or(0),
                is_dir: metadata.is_dir(),
                is_file: metadata.is_file(),
                is_symlink: metadata.is_symlink(),
                permissions: metadata.mode(),
                uid: metadata.uid(),
                gid: metadata.gid(),
            }
        }
        #[cfg(not(unix))]
        {
            proto::EntryMetadata {
                size: metadata.len(),
                modified: metadata
                    .modified()
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    })
                    .unwrap_or(0),
                accessed: metadata
                    .accessed()
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    })
                    .unwrap_or(0),
                created: metadata
                    .created()
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    })
                    .unwrap_or(0),
                is_dir: metadata.is_dir(),
                is_file: metadata.is_file(),
                is_symlink: metadata.is_symlink(),
                permissions: 0o644,
                uid: 0,
                gid: 0,
            }
        }
    }

    fn send_request(&self, plugin_name: &str, request: PluginMessage) -> Result<PluginMessage> {
        if let Some((_, api)) = self.plugins.get(plugin_name) {
            let mut buf = Vec::with_capacity(request.encoded_len());
            request.encode(&mut buf).unwrap();

            unsafe {
                let raw_response =
                    ((**api).handle_request)(std::ptr::null_mut(), buf.as_ptr(), buf.len());
                let response_vec =
                    Vec::from_raw_parts(raw_response.ptr, raw_response.len, raw_response.capacity);
                let response_msg = proto::PluginMessage::decode(&response_vec[..])
                    .map_err(|e| LlaError::Plugin(format!("Failed to decode response: {}", e)))?;
                Ok(response_msg)
            }
        } else {
            Err(LlaError::Plugin(format!(
                "Plugin '{}' not found",
                plugin_name
            )))
        }
    }

    pub fn perform_plugin_action(
        &mut self,
        plugin_name: &str,
        action: &str,
        args: &[String],
    ) -> Result<()> {
        if !self.enabled_plugins.contains(plugin_name) {
            return Err(LlaError::Plugin(format!(
                "Plugin '{}' is not enabled",
                plugin_name
            )));
        }

        let request = PluginMessage {
            message: Some(Message::Action(proto::ActionRequest {
                action: action.to_string(),
                args: args.to_vec(),
            })),
        };

        match self.send_request(plugin_name, request)?.message {
            Some(Message::ActionResponse(response)) => {
                if response.success {
                    Ok(())
                } else {
                    Err(LlaError::Plugin(
                        response
                            .error
                            .unwrap_or_else(|| "Unknown error".to_string()),
                    ))
                }
            }
            _ => Err(LlaError::Plugin("Invalid response type".to_string())),
        }
    }

    pub fn list_plugins(&mut self) -> Vec<(String, String, String)> {
        let mut result = Vec::new();
        for plugin_name in self.plugins.keys() {
            let name = match self
                .send_request(
                    plugin_name,
                    PluginMessage {
                        message: Some(Message::GetName(true)),
                    },
                )
                .and_then(|msg| match msg.message {
                    Some(Message::NameResponse(name)) => Ok(name),
                    _ => Err(LlaError::Plugin("Invalid response type".to_string())),
                }) {
                Ok(name) => name,
                Err(_) => continue,
            };

            let version = match self
                .send_request(
                    plugin_name,
                    PluginMessage {
                        message: Some(Message::GetVersion(true)),
                    },
                )
                .and_then(|msg| match msg.message {
                    Some(Message::VersionResponse(version)) => Ok(version),
                    _ => Err(LlaError::Plugin("Invalid response type".to_string())),
                }) {
                Ok(version) => version,
                Err(_) => continue,
            };

            let description = match self
                .send_request(
                    plugin_name,
                    PluginMessage {
                        message: Some(Message::GetDescription(true)),
                    },
                )
                .and_then(|msg| match msg.message {
                    Some(Message::DescriptionResponse(description)) => Ok(description),
                    _ => Err(LlaError::Plugin("Invalid response type".to_string())),
                }) {
                Ok(description) => description,
                Err(_) => continue,
            };

            result.push((name, version, description));
        }
        result
    }

    pub fn load_plugin<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref().canonicalize()?;
        if self.loaded_paths.contains(&path) {
            return Ok(());
        }

        unsafe {
            match Library::new(&path) {
                Ok(library) => {
                    match library.get::<unsafe fn() -> *mut PluginApi>(b"_plugin_create") {
                        Ok(create_fn) => {
                            let api = create_fn();
                            if (*api).version != CURRENT_PLUGIN_API_VERSION {
                                eprintln!(
                                    "⚠️ Plugin version mismatch for {:?}: expected {}, got {} run `lla clean` to remove invalid plugins",
                                    path,
                                    CURRENT_PLUGIN_API_VERSION,
                                    (*api).version
                                );
                                return Ok(());
                            }

                            let request = PluginMessage {
                                message: Some(Message::GetName(true)),
                            };
                            let mut buf = Vec::with_capacity(request.encoded_len());
                            request.encode(&mut buf).unwrap();

                            match ((*api).handle_request)(
                                std::ptr::null_mut(),
                                buf.as_ptr(),
                                buf.len(),
                            ) {
                                raw_response => {
                                    let response_vec = Vec::from_raw_parts(
                                        raw_response.ptr,
                                        raw_response.len,
                                        raw_response.capacity,
                                    );
                                    match proto::PluginMessage::decode(&response_vec[..]) {
                                        Ok(response_msg) => match response_msg.message {
                                            Some(Message::NameResponse(name)) => {
                                                if let std::collections::hash_map::Entry::Vacant(
                                                    e,
                                                ) = self.plugins.entry(name)
                                                {
                                                    e.insert((library, api));
                                                    self.loaded_paths.insert(path);
                                                }
                                            }
                                            _ => eprintln!(
                                                "⚠️ Failed to get plugin name for {:?}",
                                                path
                                            ),
                                        },
                                        Err(e) => eprintln!(
                                            "⚠️ Failed to decode response for {:?}: {}",
                                            path, e
                                        ),
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("⚠️ Plugin doesn't have a create function {:?}: {}", path, e)
                        }
                    }
                }
                Err(e) => eprintln!("⚠️ Failed to load plugin library {:?}: {}", path, e),
            }
        }
        Ok(())
    }

    pub fn discover_plugins<P: AsRef<Path>>(&mut self, plugin_dir: P) -> Result<()> {
        let plugin_dir = plugin_dir.as_ref();
        if !plugin_dir.is_dir() {
            fs::create_dir_all(plugin_dir).map_err(|e| {
                LlaError::Plugin(format!(
                    "Failed to create plugin directory {:?}: {}",
                    plugin_dir, e
                ))
            })?;
        }

        for entry in fs::read_dir(plugin_dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(extension) = path.extension() {
                if extension == "so" || extension == "dll" || extension == "dylib" {
                    match self.load_plugin(&path) {
                        Ok(_) => (),
                        Err(e) => eprintln!("Failed to load plugin {:?}: {}", path, e),
                    }
                }
            }
        }

        Ok(())
    }

    pub fn enable_plugin(&mut self, name: &str) -> Result<()> {
        if self.plugins.contains_key(name) {
            self.enabled_plugins.insert(name.to_string());
            self.config.enable_plugin(name)?;
            Ok(())
        } else {
            Err(LlaError::Plugin(format!("Plugin '{}' not found", name)))
        }
    }

    pub fn disable_plugin(&mut self, name: &str) -> Result<()> {
        if self.plugins.contains_key(name) {
            self.enabled_plugins.remove(name);
            self.config.disable_plugin(name)?;
            Ok(())
        } else {
            Err(LlaError::Plugin(format!("Plugin '{}' not found", name)))
        }
    }

    pub fn decorate_entry(&mut self, entry: &mut proto::DecoratedEntry, format: &str) {
        if self.enabled_plugins.is_empty() || (format != "default" && format != "long") {
            return;
        }

        let path_str = entry.path.clone();
        let cache_key = (path_str.clone(), format.to_string());
        if let Some(fields) = DECORATION_CACHE.get(&cache_key) {
            entry
                .custom_fields
                .extend(fields.value().iter().map(|(k, v)| (k.clone(), v.clone())));
            return;
        }

        let supported_names: Vec<_> = {
            let mut names = Vec::new();
            for name in self.enabled_plugins.iter() {
                let request = PluginMessage {
                    message: Some(Message::GetSupportedFormats(true)),
                };

                if let Ok(response) = self.send_request(name, request) {
                    if let Some(Message::FormatsResponse(formats_response)) = response.message {
                        if formats_response.formats.contains(&format.to_string()) {
                            names.push(name.clone());
                        }
                    }
                }
            }
            names
        };

        if supported_names.is_empty() {
            return;
        }

        let mut new_decorations = HashMap::with_capacity(supported_names.len() * 2);
        for name in supported_names {
            let request = PluginMessage {
                message: Some(Message::Decorate(entry.clone())),
            };

            if let Ok(response) = self.send_request(&name, request) {
                if let Some(Message::DecoratedResponse(decorated)) = response.message {
                    new_decorations.extend(decorated.custom_fields);
                }
            }
        }

        if !new_decorations.is_empty() {
            entry
                .custom_fields
                .extend(new_decorations.iter().map(|(k, v)| (k.clone(), v.clone())));
            DECORATION_CACHE.insert(cache_key, new_decorations);
        }
    }

    pub fn format_fields(&mut self, entry: &proto::DecoratedEntry, format: &str) -> Vec<String> {
        if self.enabled_plugins.is_empty() || (format != "default" && format != "long") {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(self.enabled_plugins.len());
        for name in self.enabled_plugins.iter() {
            let supports_format = match self.send_request(
                name,
                PluginMessage {
                    message: Some(Message::GetSupportedFormats(true)),
                },
            ) {
                Ok(response) => {
                    if let Some(Message::FormatsResponse(formats)) = response.message {
                        formats.formats.contains(&format.to_string())
                    } else {
                        false
                    }
                }
                Err(_) => false,
            };

            if supports_format {
                let request = PluginMessage {
                    message: Some(Message::FormatField(proto::FormatFieldRequest {
                        entry: Some(entry.clone()),
                        format: format.to_string(),
                    })),
                };

                if let Ok(response) = self.send_request(name, request) {
                    if let Some(Message::FieldResponse(field_response)) = response.message {
                        if let Some(field) = field_response.field {
                            result.push(field);
                        }
                    }
                }
            }
        }
        result
    }

    pub fn clean_plugins(&mut self) -> Result<()> {
        println!("🔄 Starting plugin cleaning...");

        let plugins_dir = self.config.plugins_dir.clone();
        let mut failed_plugins = Vec::new();

        for entry in fs::read_dir(&plugins_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(extension) = path.extension() {
                if extension == "so" || extension == "dll" || extension == "dylib" {
                    println!("📦 Checking plugin: {:?}", path);

                    match std::panic::catch_unwind(|| self.validate_plugin(&path)) {
                        Ok(Ok(true)) => println!("✅ Plugin is valid: {:?}", path),
                        Ok(Ok(false)) => {
                            println!("❌ Plugin is invalid: {:?}", path);
                            failed_plugins.push(path);
                        }
                        Ok(Err(e)) => {
                            println!("❌ Error validating plugin {:?}: {}", path, e);
                            failed_plugins.push(path);
                        }
                        Err(_) => {
                            println!("❌ Plugin validation panicked: {:?}", path);
                            failed_plugins.push(path);
                        }
                    }
                }
            }
        }

        for path in failed_plugins {
            if let Err(e) = fs::remove_file(&path) {
                eprintln!("⚠️ Failed to remove invalid plugin {:?}: {}", path, e);
            } else {
                println!("🗑️ Removed invalid plugin: {:?}", path);
            }
        }

        println!("✨ Plugin cleaning complete");
        Ok(())
    }

    fn validate_plugin<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        unsafe {
            let library = match Library::new(path.as_ref()) {
                Ok(lib) => lib,
                Err(_) => return Ok(false),
            };

            let create_fn = match library.get::<unsafe fn() -> *mut PluginApi>(b"_plugin_create") {
                Ok(f) => f,
                Err(_) => return Ok(false),
            };

            let api = match create_fn() {
                api if api.is_null() => return Ok(false),
                api => api,
            };

            if (api as usize) % std::mem::align_of::<PluginApi>() != 0 {
                return Ok(false);
            }

            if (*api).version != CURRENT_PLUGIN_API_VERSION {
                return Ok(false);
            }

            let request = PluginMessage {
                message: Some(Message::GetName(true)),
            };
            let mut buf = Vec::with_capacity(request.encoded_len());
            if request.encode(&mut buf).is_err() {
                return Ok(false);
            }

            let raw_response = match std::panic::catch_unwind(|| {
                ((*api).handle_request)(std::ptr::null_mut(), buf.as_ptr(), buf.len())
            }) {
                Ok(response) => response,
                Err(_) => return Ok(false),
            };

            if raw_response.ptr.is_null() || raw_response.len == 0 || raw_response.len > 1024 * 1024
            {
                return Ok(false);
            }

            let response_vec = match std::panic::catch_unwind(|| {
                Vec::from_raw_parts(raw_response.ptr, raw_response.len, raw_response.capacity)
            }) {
                Ok(vec) => vec,
                Err(_) => return Ok(false),
            };

            match proto::PluginMessage::decode(&response_vec[..]) {
                Ok(response_msg) => match response_msg.message {
                    Some(Message::NameResponse(_)) => Ok(true),
                    _ => Ok(false),
                },
                Err(_) => Ok(false),
            }
        }
    }
}
