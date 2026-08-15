use crate::config::Config;
use crate::error::{LlaError, Result};
use dashmap::DashMap;
use libloading::Library;
use lla_plugin_interface::{
    proto::{self, plugin_message::Message, PluginMessage},
    ActionInfo, PluginApi, CURRENT_PLUGIN_API_VERSION,
};
use once_cell::sync::Lazy;
use prost::Message as _;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

type DecorationCache = DashMap<(String, String), HashMap<String, String>>;
static DECORATION_CACHE: Lazy<DecorationCache> = Lazy::new(DashMap::new);

pub const DYNAMIC_PLUGINS_AVAILABLE: bool = true;
pub const DYNAMIC_PLUGINS_UNAVAILABLE: &str =
    "Dynamic plugins are unavailable in the static musl build; use a GNU build for plugin support.";

fn normalize_plugin_format(format: &str) -> Option<&'static str> {
    match format {
        "default" => Some("default"),
        "long" => Some("long"),
        "table" => Some("long"),
        _ => None,
    }
}

fn plugin_name_hint(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    Some(stem.strip_prefix("lib").unwrap_or(stem).to_string())
}

pub struct PluginManager {
    plugins: HashMap<String, (Library, *mut PluginApi)>,
    loaded_paths: HashSet<PathBuf>,
    supported_formats: HashMap<String, HashSet<String>>,
    pub enabled_plugins: HashSet<String>,
    config: Config,
}

impl PluginManager {
    pub fn new(config: Config) -> Self {
        let enabled_plugins = HashSet::from_iter(config.enabled_plugins.clone());
        PluginManager {
            plugins: HashMap::new(),
            loaded_paths: HashSet::new(),
            supported_formats: HashMap::new(),
            enabled_plugins,
            config,
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

    fn send_request(&self, plugin_name: &str, request: PluginMessage) -> Result<PluginMessage> {
        if let Some((_, api)) = self.plugins.get(plugin_name) {
            let mut buf = Vec::with_capacity(request.encoded_len());
            request.encode(&mut buf).map_err(|e| {
                LlaError::Plugin(format!(
                    "Failed to encode request for plugin '{}': {}",
                    plugin_name, e
                ))
            })?;

            unsafe {
                let raw_response =
                    ((**api).handle_request)(std::ptr::null_mut(), buf.as_ptr(), buf.len());
                if raw_response.ptr.is_null() {
                    return Err(LlaError::Plugin(format!(
                        "Plugin '{}' returned an empty response",
                        plugin_name
                    )));
                }
                let response_vec = raw_response.into_vec();
                let response_msg =
                    proto::PluginMessage::decode(&response_vec[..]).map_err(|e| {
                        LlaError::Plugin(format!(
                            "Failed to decode response from plugin '{}': {}",
                            plugin_name, e
                        ))
                    })?;
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
            // Check if the plugin exists
            if !self.plugins.contains_key(plugin_name) {
                // List available plugins to help the user
                let available: Vec<String> = self.plugins.keys().cloned().collect();
                let suggestion = if available.is_empty() {
                    "No plugins are currently installed. Run 'lla install' to install plugins."
                        .to_string()
                } else {
                    format!("Available plugins: {}", available.join(", "))
                };
                return Err(LlaError::Plugin(format!(
                    "Plugin '{}' not found.\n\n{}",
                    plugin_name, suggestion
                )));
            }

            // Auto-enable the plugin with a warning
            eprintln!(
                "⚠️  Plugin '{}' was disabled. Enabling it now...",
                plugin_name
            );
            self.enable_plugin(plugin_name)?;
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
                    let error_msg = response
                        .error
                        .unwrap_or_else(|| "Unknown error".to_string());

                    // If it's an unknown action error, try to list available actions
                    if error_msg.to_lowercase().contains("unknown action") {
                        if let Ok(actions) = self.get_plugin_actions(plugin_name) {
                            let action_list: Vec<String> = actions
                                .iter()
                                .map(|a| format!("  • {} - {}", a.name, a.description))
                                .collect();

                            if !action_list.is_empty() {
                                return Err(LlaError::Plugin(format!(
                                    "{}\n\nAvailable actions for '{}':\n{}",
                                    error_msg,
                                    plugin_name,
                                    action_list.join("\n")
                                )));
                            }
                        }
                    }

                    Err(LlaError::Plugin(error_msg))
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

    pub fn get_plugin_actions(&mut self, plugin_name: &str) -> Result<Vec<ActionInfo>> {
        if !self.plugins.contains_key(plugin_name) {
            return Err(LlaError::Plugin(format!(
                "Plugin '{}' not found",
                plugin_name
            )));
        }

        let request = PluginMessage {
            message: Some(Message::ListActions(true)),
        };

        match self.send_request(plugin_name, request)?.message {
            Some(Message::ListActionsResponse(response)) => {
                let actions = response
                    .actions
                    .into_iter()
                    .map(|action| ActionInfo {
                        name: action.name,
                        usage: action.usage,
                        description: action.description,
                        examples: action.examples,
                    })
                    .collect();
                Ok(actions)
            }
            _ => Err(LlaError::Plugin(
                "Invalid response type for list actions".to_string(),
            )),
        }
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
                            if let Err(e) = request.encode(&mut buf) {
                                eprintln!("⚠️ Failed to encode name request for {:?}: {}", path, e);
                                return Ok(());
                            }

                            match ((*api).handle_request)(
                                std::ptr::null_mut(),
                                buf.as_ptr(),
                                buf.len(),
                            ) {
                                raw_response => {
                                    if raw_response.ptr.is_null() {
                                        eprintln!(
                                            "⚠️ Plugin returned an empty response for {:?}",
                                            path
                                        );
                                        return Ok(());
                                    }
                                    let response_vec = raw_response.into_vec();
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
        self.discover_plugins_impl(plugin_dir.as_ref(), None)
    }

    pub fn discover_plugins_named<P: AsRef<Path>>(
        &mut self,
        plugin_dir: P,
        names: &HashSet<String>,
    ) -> Result<()> {
        let plugin_dir = plugin_dir.as_ref();
        if names.is_empty() {
            if !plugin_dir.is_dir() {
                fs::create_dir_all(plugin_dir).map_err(|e| {
                    LlaError::Plugin(format!(
                        "Failed to create plugin directory {:?}: {}",
                        plugin_dir, e
                    ))
                })?;
            }
            return Ok(());
        }
        self.discover_plugins_impl(plugin_dir, Some(names))
    }

    fn discover_plugins_impl(
        &mut self,
        plugin_dir: &Path,
        names: Option<&HashSet<String>>,
    ) -> Result<()> {
        if !plugin_dir.is_dir() {
            fs::create_dir_all(plugin_dir).map_err(|e| {
                LlaError::Plugin(format!(
                    "Failed to create plugin directory {:?}: {}",
                    plugin_dir, e
                ))
            })?;
        }

        let mut candidates = Vec::new();
        for entry in fs::read_dir(plugin_dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(extension) = path.extension() {
                if extension == "so" || extension == "dll" || extension == "dylib" {
                    candidates.push(path);
                }
            }
        }

        if let Some(names) = names {
            // Installed libraries conventionally use lib<plugin-name>.<ext>. Load
            // just the requested libraries on the hot path. If a third-party plugin
            // uses a non-standard filename, fall back to discovery so compatibility
            // and the existing error suggestions are preserved.
            let (matching, remaining): (Vec<_>, Vec<_>) = candidates
                .into_iter()
                .partition(|path| plugin_name_hint(path).is_some_and(|name| names.contains(&name)));

            self.load_plugin_candidates(matching);
            if names.iter().any(|name| !self.plugins.contains_key(name)) {
                self.load_plugin_candidates(remaining);
            }
        } else {
            self.load_plugin_candidates(candidates);
        }

        Ok(())
    }

    fn load_plugin_candidates(&mut self, candidates: Vec<PathBuf>) {
        for path in candidates {
            if let Err(e) = self.load_plugin(&path) {
                eprintln!("Failed to load plugin {:?}: {}", path, e);
            }
        }
    }

    fn supports_format(&mut self, plugin_name: &str, format: &str) -> bool {
        if let Some(formats) = self.supported_formats.get(plugin_name) {
            return formats.contains(format);
        }

        let formats = self
            .send_request(
                plugin_name,
                PluginMessage {
                    message: Some(Message::GetSupportedFormats(true)),
                },
            )
            .ok()
            .and_then(|response| match response.message {
                Some(Message::FormatsResponse(response)) => {
                    Some(response.formats.into_iter().collect::<HashSet<_>>())
                }
                _ => None,
            })
            .unwrap_or_default();
        let supports = formats.contains(format);
        self.supported_formats
            .insert(plugin_name.to_string(), formats);
        supports
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
        let Some(plugin_format) = normalize_plugin_format(format) else {
            return;
        };
        if self.enabled_plugins.is_empty() {
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

        let enabled_names: Vec<_> = self.enabled_plugins.iter().cloned().collect();
        let supported_names: Vec<_> = enabled_names
            .into_iter()
            .filter(|name| self.supports_format(name, plugin_format))
            .collect();

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
        let Some(plugin_format) = normalize_plugin_format(format) else {
            return Vec::new();
        };
        if self.enabled_plugins.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(self.enabled_plugins.len());
        let enabled_names: Vec<_> = self.enabled_plugins.iter().cloned().collect();
        for name in enabled_names {
            if self.supports_format(&name, plugin_format) {
                let request = PluginMessage {
                    message: Some(Message::FormatField(proto::FormatFieldRequest {
                        entry: Some(entry.clone()),
                        format: plugin_format.to_string(),
                    })),
                };

                if let Ok(response) = self.send_request(&name, request) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_name_hint_handles_platform_library_prefix() {
        assert_eq!(
            plugin_name_hint(Path::new("libfile_hash.dylib")).as_deref(),
            Some("file_hash")
        );
        assert_eq!(
            plugin_name_hint(Path::new("file_hash.dll")).as_deref(),
            Some("file_hash")
        );
    }

    #[test]
    fn empty_selective_discovery_still_creates_custom_plugin_directory() {
        let root = tempfile::tempdir().unwrap();
        let plugin_dir = root.path().join("plugins");
        let mut manager = PluginManager::new(Config::default());

        manager
            .discover_plugins_named(&plugin_dir, &HashSet::new())
            .unwrap();

        assert!(plugin_dir.is_dir());
    }
}
