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
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

type DecorationCache = DashMap<(String, String), HashMap<String, String>>;
static DECORATION_CACHE: Lazy<DecorationCache> = Lazy::new(DashMap::new);

type CapabilityCache = DashMap<String, Vec<String>>;
static CAPABILITY_CACHE: Lazy<CapabilityCache> = Lazy::new(DashMap::new);

#[derive(Clone)]
pub struct PluginHealth {
    pub is_healthy: bool,
    pub last_error: Option<String>,
    pub last_error_time: Option<u64>,
    pub missing_dependencies: Vec<String>,
}

impl Default for PluginHealth {
    fn default() -> Self {
        PluginHealth {
            is_healthy: true,
            last_error: None,
            last_error_time: None,
            missing_dependencies: Vec::new(),
        }
    }
}

pub struct PluginManager {
    plugins: HashMap<String, (Library, *mut PluginApi)>,
    loaded_paths: HashSet<PathBuf>,
    pub enabled_plugins: HashSet<String>,
    config: Config,
    plugin_health: HashMap<String, PluginHealth>,
}

impl PluginManager {
    pub fn new(config: Config) -> Self {
        let enabled_plugins = HashSet::from_iter(config.enabled_plugins.clone());
        PluginManager {
            plugins: HashMap::new(),
            loaded_paths: HashSet::new(),
            enabled_plugins,
            config,
            plugin_health: HashMap::new(),
        }
    }

    fn _convert_metadata(metadata: &std::fs::Metadata) -> proto::EntryMetadata {
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

    fn record_plugin_error(&mut self, plugin_name: &str, error: String) {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let health = self.plugin_health.entry(plugin_name.to_string()).or_default();
        health.is_healthy = false;
        health.last_error = Some(error);
        health.last_error_time = Some(current_time);
    }

    fn send_request(&mut self, plugin_name: &str, request: PluginMessage) -> Result<PluginMessage> {
        if let Some((_, api)) = self.plugins.get(plugin_name) {
            let mut buf = Vec::with_capacity(request.encoded_len());
            request.encode(&mut buf).unwrap();

            unsafe {
                let raw_response =
                    ((**api).handle_request)(std::ptr::null_mut(), buf.as_ptr(), buf.len());
                let response_vec =
                    Vec::from_raw_parts(raw_response.ptr, raw_response.len, raw_response.capacity);
                match proto::PluginMessage::decode(&response_vec[..]) {
                    Ok(response_msg) => {
                        // Mark plugin as healthy on successful response
                        self.plugin_health.entry(plugin_name.to_string()).or_default().is_healthy = true;
                        Ok(response_msg)
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to decode response: {}", e);
                        self.record_plugin_error(plugin_name, error_msg.clone());
                        Err(LlaError::Plugin(error_msg))
                    }
                }
            }
        } else {
            let error_msg = format!("Plugin '{}' not found", plugin_name);
            self.record_plugin_error(plugin_name, error_msg.clone());
            Err(LlaError::Plugin(error_msg))
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

    pub fn list_plugins(&mut self) -> Vec<proto::PluginInfo> {
        let mut result = Vec::new();
        let plugin_names: Vec<String> = self.plugins.keys().cloned().collect();
        for plugin_name in plugin_names {
            let name = match self
                .send_request(
                    &plugin_name,
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
                    &plugin_name,
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
                    &plugin_name,
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

            // Get health information
            let health = self.plugin_health.get(&plugin_name).cloned().unwrap_or_default();
            let proto_health = proto::PluginHealth {
                is_healthy: health.is_healthy,
                last_error: health.last_error,
                last_error_time: health.last_error_time,
                missing_dependencies: health.missing_dependencies,
            };

            result.push(proto::PluginInfo {
                name,
                version,
                description,
                health: Some(proto_health),
            });
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
                                                ) = self.plugins.entry(name.clone())
                                                {
                                                    e.insert((library, api));
                                                    self.loaded_paths.insert(path);

                                                    // Initialize plugin health
                                                    self.plugin_health.insert(name.clone(), PluginHealth::default());
                                                    // Send config/context message to newly loaded plugin
                                                    self.send_config_to_plugin(&name);
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

    fn send_config_to_plugin(&mut self, plugin_name: &str) {
        let mut config_map = std::collections::HashMap::new();

        // Add basic config values
        config_map.insert("version".to_string(), "0.4.1".to_string());
        config_map.insert("api_version".to_string(), CURRENT_PLUGIN_API_VERSION.to_string());

        // Add user preferences from config
        config_map.insert("theme".to_string(), self.config.theme.clone());
        config_map.insert("default_format".to_string(), self.config.default_format.clone());
        config_map.insert("show_icons".to_string(), self.config.show_icons.to_string());

        // Extract shortcuts as strings
        let shortcuts: Vec<String> = self.config.shortcuts
            .iter()
            .map(|(key, cmd)| format!("{}:{}", key, cmd.action))
            .collect();

        let config_request = proto::PluginMessage {
            message: Some(proto::plugin_message::Message::Config(proto::ConfigRequest {
                config: config_map,
                theme: self.config.theme.clone(),
                shortcuts,
            })),
        };

        // Send config message - ignore response as it's optional for plugins
        let _ = self.send_request(plugin_name, config_request);
    }

    fn get_supported_formats(&mut self, plugin_name: &str) -> Vec<String> {
        if let Some(cached_formats) = CAPABILITY_CACHE.get(plugin_name) {
            return cached_formats.clone();
        }

        let request = PluginMessage {
            message: Some(Message::GetSupportedFormats(true)),
        };

        let formats = if let Ok(response) = self.send_request(plugin_name, request) {
            if let Some(Message::FormatsResponse(formats_response)) = response.message {
                formats_response.formats
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        CAPABILITY_CACHE.insert(plugin_name.to_string(), formats.clone());
        formats
    }

    pub fn decorate_entries_batch(&mut self, entries: &mut [proto::DecoratedEntry], format: &str) {
        if self.enabled_plugins.is_empty() || (format != "default" && format != "long") {
            return;
        }

        // Pre-collect supported plugin names to avoid borrowing conflicts
        let plugin_names: Vec<String> = self.enabled_plugins.iter().cloned().collect();
        let mut supported_names = Vec::new();
        for name in plugin_names {
            let formats = self.get_supported_formats(&name);
            if formats.contains(&format.to_string()) {
                supported_names.push(name);
            }
        }

        if supported_names.is_empty() {
            return;
        }

        for name in supported_names {
            // Try batch decoration first
            let batch_request = proto::PluginMessage {
                message: Some(proto::plugin_message::Message::BatchDecorate(
                    proto::BatchDecorateRequest {
                        entries: entries.to_vec(),
                        format: format.to_string(),
                    },
                )),
            };

            if let Ok(response) = self.send_request(&name, batch_request) {
                if let Some(proto::plugin_message::Message::BatchDecoratedResponse(batch_response)) = response.message {
                    // Apply batch results to entries
                    for (i, decorated_entry) in batch_response.entries.into_iter().enumerate() {
                        if i < entries.len() {
                            entries[i].custom_fields.extend(decorated_entry.custom_fields);
                        }
                    }
                    continue;
                }
            }

            // Fall back to individual decoration
            for entry in entries.iter_mut() {
                let request = proto::PluginMessage {
                    message: Some(proto::plugin_message::Message::Decorate(entry.clone())),
                };

                if let Ok(response) = self.send_request(&name, request) {
                    if let Some(proto::plugin_message::Message::DecoratedResponse(decorated)) = response.message {
                        entry.custom_fields.extend(decorated.custom_fields);
                    }
                }
            }
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

        // Pre-collect supported plugin names to avoid borrowing conflicts
        let plugin_names: Vec<String> = self.enabled_plugins.iter().cloned().collect();
        let mut supported_names = Vec::new();
        for name in plugin_names {
            let formats = self.get_supported_formats(&name);
            if formats.contains(&format.to_string()) {
                supported_names.push(name);
            }
        }

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

        // Pre-collect supported plugin names to avoid borrowing conflicts
        let plugin_names: Vec<String> = self.enabled_plugins.iter().cloned().collect();
        let mut supported_names = Vec::new();
        for name in plugin_names {
            let formats = self.get_supported_formats(&name);
            if formats.contains(&format.to_string()) {
                supported_names.push(name);
            }
        }

        let mut result = Vec::with_capacity(supported_names.len());
        for name in supported_names.iter() {
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
