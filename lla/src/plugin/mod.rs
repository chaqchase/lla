use crate::config::Config;
use crate::error::{LlaError, Result};
use dashmap::DashMap;
use libloading::Library;
use lla_plugin_interface::{
    manifest::{FieldType, PluginManifest, PluginRuntime, MANIFEST_FILE_NAME},
    proto::{self, plugin_message::Message, PluginMessage},
    ActionInfo, PluginApi, PluginApiV2, CURRENT_PLUGIN_API_VERSION, LEGACY_PLUGIN_API_VERSION,
};
use once_cell::sync::Lazy;
use prost::Message as _;
use std::collections::{HashMap, HashSet};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub(crate) mod package;

#[derive(Clone, Default)]
struct CachedDecoration {
    custom_fields: HashMap<String, String>,
    typed_fields: HashMap<String, proto::TypedValue>,
}

type DecorationCache = DashMap<(String, String, String), CachedDecoration>;
static DECORATION_CACHE: Lazy<DecorationCache> = Lazy::new(DashMap::new);
const MAX_PLUGIN_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

struct ScopedEnvVar {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl ScopedEnvVar {
    fn set_path(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

pub const DYNAMIC_PLUGINS_AVAILABLE: bool = true;
pub const DYNAMIC_PLUGINS_UNAVAILABLE: &str =
    "Dynamic plugins are unavailable in the static musl build; use a GNU build for plugin support.";

enum PluginHandle {
    V1(*mut PluginApi),
    V2(*mut PluginApiV2),
}

impl Drop for PluginHandle {
    fn drop(&mut self) {
        if let Self::V2(api) = self {
            unsafe {
                if !api.is_null() {
                    ((**api).destroy)(*api);
                    *api = std::ptr::null_mut();
                }
            }
        }
    }
}

struct LoadedPlugin {
    // `api` must be destroyed before the dynamic library is unloaded. Rust
    // drops fields in declaration order, so keep it before `_library`.
    api: PluginHandle,
    // The library must outlive every function pointer stored in `api`.
    _library: Library,
    path: PathBuf,
}

impl PluginHandle {
    unsafe fn send(&self, request: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::V1(api) => {
                let response =
                    ((**api).handle_request)(std::ptr::null_mut(), request.as_ptr(), request.len());
                if response.ptr.is_null() {
                    return Err(LlaError::Plugin(
                        "Plugin returned an empty response".to_string(),
                    ));
                }
                // v1 requires reconstructing plugin-owned memory in the host. It remains
                // available only as a compatibility path; v2 always frees in the plugin.
                Ok(response.into_vec())
            }
            Self::V2(api) => {
                let response = ((**api).handle_request)(request.as_ptr(), request.len());
                if response.ptr.is_null() {
                    return Err(LlaError::Plugin(
                        "Plugin returned an empty response".to_string(),
                    ));
                }
                if response.len > MAX_PLUGIN_RESPONSE_BYTES {
                    ((**api).free_response)(response);
                    return Err(LlaError::Plugin(format!(
                        "Plugin response exceeds the {} MiB limit",
                        MAX_PLUGIN_RESPONSE_BYTES / (1024 * 1024)
                    )));
                }
                let bytes = std::slice::from_raw_parts(response.ptr, response.len).to_vec();
                ((**api).free_response)(response);
                Ok(bytes)
            }
        }
    }

    fn is_v2(&self) -> bool {
        matches!(self, Self::V2(_))
    }
}

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
    plugins: HashMap<String, LoadedPlugin>,
    loaded_paths: HashSet<PathBuf>,
    supported_formats: HashMap<String, HashSet<String>>,
    manifests: HashMap<String, PluginManifest>,
    shadowed_plugins: Vec<(String, PathBuf)>,
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
            manifests: HashMap::new(),
            shadowed_plugins: Vec::new(),
            enabled_plugins,
            config,
        }
    }

    fn convert_metadata(metadata: &std::fs::Metadata) -> proto::EntryMetadata {
        #[cfg(unix)]
        let (permissions, uid, gid) = (metadata.mode(), metadata.uid(), metadata.gid());
        #[cfg(not(unix))]
        let (permissions, uid, gid) = (0, 0, 0);

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
            permissions,
            uid,
            gid,
        }
    }

    fn send_request(&self, plugin_name: &str, request: PluginMessage) -> Result<PluginMessage> {
        if let Some(plugin) = self.plugins.get(plugin_name) {
            let mut buf = Vec::with_capacity(request.encoded_len());
            request.encode(&mut buf).map_err(|e| {
                LlaError::Plugin(format!(
                    "Failed to encode request for plugin '{}': {}",
                    plugin_name, e
                ))
            })?;

            let response_vec = unsafe { plugin.api.send(&buf) }.map_err(|error| {
                LlaError::Plugin(format!("Plugin '{}' failed: {}", plugin_name, error))
            })?;
            proto::PluginMessage::decode(&response_vec[..]).map_err(|e| {
                LlaError::Plugin(format!(
                    "Failed to decode response from plugin '{}': {}",
                    plugin_name, e
                ))
            })
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
            if let Some(manifest) = self.manifests.get(plugin_name) {
                result.push((
                    manifest.plugin.name.clone(),
                    manifest.plugin.version.clone(),
                    manifest.plugin.description.clone(),
                ));
                continue;
            }
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
            let library = Library::new(&path).map_err(|error| {
                LlaError::Plugin(format!(
                    "Failed to load plugin library {}: {error}",
                    path.display()
                ))
            })?;

            let api = if let Ok(create_v2) =
                library.get::<unsafe fn() -> *mut PluginApiV2>(b"_plugin_create_v2")
            {
                let api = create_v2();
                if api.is_null() {
                    return Err(LlaError::Plugin(format!(
                        "Plugin {} returned a null v2 API",
                        path.display()
                    )));
                }
                let compatible = (*api).abi_version == CURRENT_PLUGIN_API_VERSION
                    && (*api).min_host_api <= CURRENT_PLUGIN_API_VERSION
                    && (*api).max_host_api >= CURRENT_PLUGIN_API_VERSION;
                let handle = PluginHandle::V2(api);
                if !compatible {
                    return Err(LlaError::Plugin(format!(
                        "Plugin {} has an incompatible v2 API",
                        path.display()
                    )));
                }
                handle
            } else if let Ok(create_v1) =
                library.get::<unsafe fn() -> *mut PluginApi>(b"_plugin_create")
            {
                let api = create_v1();
                if api.is_null() || (*api).version != LEGACY_PLUGIN_API_VERSION {
                    return Err(LlaError::Plugin(format!(
                        "Plugin {} has an incompatible legacy API",
                        path.display()
                    )));
                }
                PluginHandle::V1(api)
            } else {
                return Err(LlaError::Plugin(format!(
                    "Plugin {} has no supported create function",
                    path.display()
                )));
            };

            let request = PluginMessage {
                message: Some(Message::GetName(true)),
            };
            let mut buf = Vec::with_capacity(request.encoded_len());
            request.encode(&mut buf).map_err(|error| {
                LlaError::Plugin(format!("Failed to encode plugin name request: {error}"))
            })?;
            let response = api.send(&buf)?;
            let decoded = proto::PluginMessage::decode(&response[..]).map_err(|error| {
                LlaError::Plugin(format!("Failed to decode plugin name response: {error}"))
            })?;
            let name = match decoded.message {
                Some(Message::NameResponse(name)) => name,
                _ => {
                    return Err(LlaError::Plugin(format!(
                        "Plugin {} returned an invalid name response",
                        path.display()
                    )))
                }
            };

            if let std::collections::hash_map::Entry::Vacant(entry) = self.plugins.entry(name) {
                entry.insert(LoadedPlugin {
                    api,
                    _library: library,
                    path: path.clone(),
                });
                self.loaded_paths.insert(path);
            }
        }
        Ok(())
    }

    pub fn discover_plugin_paths(&mut self, plugin_dirs: &[PathBuf]) -> Result<()> {
        for (index, plugin_dir) in plugin_dirs.iter().enumerate() {
            if index == 0 || plugin_dir.is_dir() {
                self.discover_plugins_impl(plugin_dir, None)?;
            }
        }
        Ok(())
    }

    pub fn discover_plugin_paths_named(
        &mut self,
        plugin_dirs: &[PathBuf],
        names: &HashSet<String>,
    ) -> Result<()> {
        if names.is_empty() {
            if let Some(primary) = plugin_dirs.first() {
                fs::create_dir_all(primary)?;
            }
            return Ok(());
        }
        for (index, plugin_dir) in plugin_dirs.iter().enumerate() {
            if index == 0 || plugin_dir.is_dir() {
                self.discover_plugins_impl(plugin_dir, Some(names))?;
            }
        }
        Ok(())
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

        let mut paths = fs::read_dir(plugin_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();

        let mut package_candidates = Vec::new();
        let mut legacy_candidates = Vec::new();
        for path in paths {
            if is_regular_directory(&path) {
                let manifest_path = path.join(MANIFEST_FILE_NAME);
                if is_regular_file(&manifest_path) {
                    match PluginManifest::from_path(&manifest_path) {
                        Ok(manifest) => {
                            if !manifest.supports_host_api(CURRENT_PLUGIN_API_VERSION) {
                                eprintln!(
                                    "⚠️ Plugin '{}' does not support host API v{}",
                                    manifest.plugin.name, CURRENT_PLUGIN_API_VERSION
                                );
                                continue;
                            }
                            if manifest.plugin.runtime != PluginRuntime::Native {
                                eprintln!(
                                    "⚠️ Plugin '{}' uses runtime {:?}, which is not enabled yet",
                                    manifest.plugin.name, manifest.plugin.runtime
                                );
                                continue;
                            }
                            if names.is_some_and(|requested| {
                                !requested.contains(&manifest.plugin.name)
                                    && !requested.contains(&manifest.plugin.id)
                            }) {
                                continue;
                            }
                            match resolve_manifest_entrypoint(&path, &manifest.plugin.entrypoint) {
                                Some(entrypoint) => {
                                    match package::verify_package_checksums(&entrypoint) {
                                        Ok(_) => package_candidates.push((
                                            entrypoint,
                                            manifest,
                                            manifest_path,
                                        )),
                                        Err(error) => eprintln!(
                                        "⚠️ Plugin package '{}' failed checksum verification: {}",
                                        manifest.plugin.name, error
                                    ),
                                    }
                                }
                                None => eprintln!(
                                    "⚠️ Plugin '{}' entrypoint '{}' was not found in {}",
                                    manifest.plugin.name,
                                    manifest.plugin.entrypoint,
                                    path.display()
                                ),
                            }
                        }
                        Err(error) => eprintln!("⚠️ {error}"),
                    }
                }
                continue;
            }
            if let Some(extension) = path.extension() {
                if extension == "so" || extension == "dll" || extension == "dylib" {
                    legacy_candidates.push(path);
                }
            }
        }

        for (entrypoint, manifest, manifest_path) in package_candidates {
            let name = manifest.plugin.name.clone();
            if self.plugins.contains_key(&name) {
                self.shadowed_plugins.push((name, manifest_path));
                continue;
            }

            if let Err(error) = self.load_plugin(&entrypoint) {
                eprintln!(
                    "⚠️ Failed to load plugin package '{}': {}",
                    manifest.plugin.name, error
                );
                continue;
            }
            if self.loaded_plugin_matches_manifest(&manifest, &entrypoint) {
                self.manifests.insert(name, manifest);
            } else {
                self.unload_plugin_path(&entrypoint);
                eprintln!(
                    "⚠️ Plugin package '{}' does not match its v2 manifest",
                    manifest.plugin.name
                );
            }
        }

        if let Some(names) = names {
            // Installed libraries conventionally use lib<plugin-name>.<ext>. Load
            // just the requested libraries on the hot path. If a third-party plugin
            // uses a non-standard filename, fall back to discovery so compatibility
            // and the existing error suggestions are preserved.
            let (matching, remaining): (Vec<_>, Vec<_>) = legacy_candidates
                .into_iter()
                .partition(|path| plugin_name_hint(path).is_some_and(|name| names.contains(&name)));

            self.load_plugin_candidates(matching);
            let all_requested_loaded = names.iter().all(|requested| {
                self.plugins.contains_key(requested)
                    || self
                        .manifests
                        .values()
                        .any(|manifest| manifest.plugin.id == *requested)
            });
            if !all_requested_loaded {
                self.load_plugin_candidates(remaining);
            }
        } else {
            self.load_plugin_candidates(legacy_candidates);
        }

        Ok(())
    }

    pub fn doctor(&self, paths: &[PathBuf]) -> Result<bool> {
        let mut healthy = true;
        let mut found = 0usize;
        let mut seen_names = HashMap::<String, PathBuf>::new();
        let mut seen_ids = HashMap::<String, PathBuf>::new();
        let verification_data = tempfile::tempdir().map_err(|error| {
            LlaError::Plugin(format!(
                "Failed to isolate plugin verification data: {error}"
            ))
        })?;
        let _data_guard = ScopedEnvVar::set_path("LLA_PLUGIN_DATA_DIR", verification_data.path());
        println!("Plugin Platform v2 diagnostics");

        for plugin_dir in paths {
            if !plugin_dir.is_dir() {
                println!("  · {} (not present)", plugin_dir.display());
                continue;
            }
            println!("  ✓ {}", plugin_dir.display());
            let mut package_paths = fs::read_dir(plugin_dir)?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            package_paths.sort();
            for path in package_paths {
                if !is_regular_directory(&path) {
                    continue;
                }
                let manifest_path = path.join(MANIFEST_FILE_NAME);
                if !is_regular_file(&manifest_path) {
                    continue;
                }
                found += 1;
                match PluginManifest::from_path(&manifest_path) {
                    Ok(manifest) => {
                        if let Some(selected) = seen_names.get(&manifest.plugin.name) {
                            println!(
                                "    ! {} at {} is shadowed by {}",
                                manifest.plugin.name,
                                manifest_path.display(),
                                selected.display()
                            );
                        } else {
                            seen_names.insert(manifest.plugin.name.clone(), manifest_path.clone());
                        }
                        if let Some(selected) = seen_ids.get(&manifest.plugin.id) {
                            println!(
                                "    ! ID {} at {} is already provided by {}",
                                manifest.plugin.id,
                                manifest_path.display(),
                                selected.display()
                            );
                        } else {
                            seen_ids.insert(manifest.plugin.id.clone(), manifest_path.clone());
                        }
                        let compatible = manifest.supports_host_api(CURRENT_PLUGIN_API_VERSION);
                        let entrypoint =
                            resolve_manifest_entrypoint(&path, &manifest.plugin.entrypoint);
                        let checksum_result = entrypoint.as_ref().map_or(Ok(false), |entrypoint| {
                            package::verify_package_checksums(entrypoint)
                        });
                        let runtime_matches = checksum_result.is_ok()
                            && entrypoint.as_ref().is_some_and(|entrypoint| {
                                self.manifest_matches_runtime(&manifest, entrypoint)
                            });
                        if compatible && runtime_matches && checksum_result.is_ok() {
                            println!(
                                "    ✓ {} v{} (API {}..={}{})",
                                manifest.plugin.name,
                                manifest.plugin.version,
                                manifest.plugin.api_min,
                                manifest.plugin.api_max,
                                if matches!(checksum_result, Ok(true)) {
                                    ", checksums + contract verified"
                                } else {
                                    ""
                                }
                            );
                        } else {
                            healthy = false;
                            let checksum_error = checksum_result.err();
                            println!(
                                "    ✗ {}: {}{}",
                                manifest.plugin.name,
                                if !compatible {
                                    "incompatible API"
                                } else if entrypoint.is_none() {
                                    "missing entrypoint"
                                } else if let Some(error) = checksum_error.as_deref() {
                                    error
                                } else if !runtime_matches {
                                    "manifest/runtime metadata mismatch"
                                } else {
                                    ""
                                },
                                ""
                            );
                        }
                    }
                    Err(error) => {
                        healthy = false;
                        println!("    ✗ {error}");
                    }
                }
            }
        }
        println!("  {} v2 plugin package(s) found", found);
        if !self.shadowed_plugins.is_empty() {
            for (name, path) in &self.shadowed_plugins {
                println!("    ! {name} is shadowed at {}", path.display());
            }
        }
        Ok(healthy)
    }

    fn manifest_matches_runtime(&self, manifest: &PluginManifest, entrypoint: &Path) -> bool {
        let mut probe = PluginManager::new(self.config.clone());
        if let Err(error) = probe.load_plugin(entrypoint) {
            eprintln!(
                "⚠️ Plugin '{}' could not be loaded for verification: {}",
                manifest.plugin.name, error
            );
            return false;
        }
        if !probe.loaded_plugin_matches_manifest(manifest, entrypoint) {
            return false;
        }
        if let Err(error) = probe.verify_functional_contract(manifest) {
            eprintln!(
                "⚠️ Plugin '{}' failed v2 functional verification: {}",
                manifest.plugin.name, error
            );
            return false;
        }
        true
    }

    fn loaded_plugin_matches_manifest(
        &mut self,
        manifest: &PluginManifest,
        entrypoint: &Path,
    ) -> bool {
        let Ok(entrypoint) = entrypoint.canonicalize() else {
            return false;
        };
        let Some(plugin) = self.plugins.get(&manifest.plugin.name) else {
            return false;
        };
        if plugin.path != entrypoint || !plugin.api.is_v2() {
            return false;
        }

        let runtime_version = self
            .send_request(
                &manifest.plugin.name,
                PluginMessage {
                    message: Some(Message::GetVersion(true)),
                },
            )
            .ok()
            .and_then(|response| match response.message {
                Some(Message::VersionResponse(version)) => Some(version),
                _ => None,
            });
        if runtime_version.as_deref() != Some(manifest.plugin.version.as_str()) {
            return false;
        }

        let runtime_formats = self
            .send_request(
                &manifest.plugin.name,
                PluginMessage {
                    message: Some(Message::GetSupportedFormats(true)),
                },
            )
            .ok()
            .and_then(|response| match response.message {
                Some(Message::FormatsResponse(formats)) => Some(formats.formats),
                _ => None,
            });
        let Some(runtime_formats) = runtime_formats else {
            return false;
        };
        let runtime_format_set = runtime_formats.iter().cloned().collect::<HashSet<_>>();
        if runtime_formats
            .iter()
            .any(|format| format.trim().is_empty())
            || runtime_format_set.len() != runtime_formats.len()
        {
            return false;
        }
        let declared_formats = manifest
            .capabilities
            .formats
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        if runtime_format_set != declared_formats {
            return false;
        }

        let Ok(runtime_actions) = self.get_plugin_actions(&manifest.plugin.name) else {
            return false;
        };
        if runtime_actions.iter().any(|action| {
            action.name.trim().is_empty()
                || action.usage.trim().is_empty()
                || action.description.trim().is_empty()
        }) {
            return false;
        }
        let runtime_action_set = runtime_actions
            .iter()
            .map(|action| action.name.clone())
            .collect::<HashSet<_>>();
        if runtime_action_set.len() != runtime_actions.len() {
            return false;
        }
        let declared_actions = manifest
            .capabilities
            .actions
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        runtime_action_set == declared_actions
    }

    fn verify_functional_contract(&mut self, manifest: &PluginManifest) -> Result<()> {
        let fixture = tempfile::tempdir().map_err(|error| {
            LlaError::Plugin(format!(
                "Failed to create plugin verification fixture: {error}"
            ))
        })?;
        let fixture_path = fixture.path().join("lla-v2-fixture.txt");
        fs::write(&fixture_path, b"lla plugin platform v2\n")?;
        let mut entries = Vec::new();
        for path in [&fixture_path, fixture.path()] {
            let metadata = fs::metadata(path)?;
            entries.push(proto::DecoratedEntry {
                path: path.to_string_lossy().to_string(),
                metadata: Some(Self::convert_metadata(&metadata)),
                custom_fields: HashMap::new(),
                typed_fields: HashMap::new(),
            });
        }

        let mut singles = Vec::with_capacity(entries.len());
        for entry in &entries {
            let decorated = self
                .send_request(
                    &manifest.plugin.name,
                    PluginMessage {
                        message: Some(Message::Decorate(entry.clone())),
                    },
                )?
                .message
                .and_then(|message| match message {
                    Message::DecoratedResponse(entry) => Some(entry),
                    _ => None,
                })
                .ok_or_else(|| {
                    LlaError::Plugin(
                        "single-entry decoration returned the wrong response".to_string(),
                    )
                })?;
            singles.push(decorated);
        }

        let format = manifest
            .capabilities
            .formats
            .first()
            .map(String::as_str)
            .unwrap_or("default");
        let batch = self
            .send_request(
                &manifest.plugin.name,
                PluginMessage {
                    message: Some(Message::DecorateBatch(proto::BatchDecorateRequest {
                        entries,
                        format: format.to_string(),
                    })),
                },
            )?
            .message
            .and_then(|message| match message {
                Message::DecorateBatchResponse(response)
                    if response.entries.len() == singles.len() =>
                {
                    Some(response.entries)
                }
                _ => None,
            })
            .ok_or_else(|| {
                LlaError::Plugin("batch decoration returned the wrong response shape".to_string())
            })?;
        if batch != singles {
            return Err(LlaError::Plugin(
                "batch decoration does not preserve single-entry behavior".to_string(),
            ));
        }

        self.manifests
            .insert(manifest.plugin.name.clone(), manifest.clone());
        for entry in &mut singles {
            self.apply_typed_fields(entry);
            for field in &manifest.fields {
                if entry.custom_fields.contains_key(&field.name)
                    && !entry.typed_fields.contains_key(&field.name)
                {
                    return Err(LlaError::Plugin(format!(
                        "field '{}' cannot be converted to its declared {:?} type",
                        field.name, field.field_type
                    )));
                }
            }
        }

        for format in &manifest.capabilities.formats {
            for entry in &singles {
                let has_declared_fields = manifest
                    .fields
                    .iter()
                    .any(|field| entry.custom_fields.contains_key(&field.name));
                let response = self.send_request(
                    &manifest.plugin.name,
                    PluginMessage {
                        message: Some(Message::FormatField(proto::FormatFieldRequest {
                            entry: Some(entry.clone()),
                            format: format.clone(),
                        })),
                    },
                )?;
                match response.message {
                    Some(Message::FieldResponse(response))
                        if !has_declared_fields || response.field.is_some() => {}
                    Some(Message::FieldResponse(_)) => {
                        return Err(LlaError::Plugin(format!(
                            "format '{}' omitted present declared fields",
                            format
                        )));
                    }
                    _ => {
                        return Err(LlaError::Plugin(format!(
                            "format '{}' returned the wrong response",
                            format
                        )));
                    }
                }
            }
        }

        let unknown_action = "__lla_v2_contract_probe__";
        let response = self.send_request(
            &manifest.plugin.name,
            PluginMessage {
                message: Some(Message::Action(proto::ActionRequest {
                    action: unknown_action.to_string(),
                    args: Vec::new(),
                })),
            },
        )?;
        if !matches!(
            response.message,
            Some(Message::ActionResponse(proto::ActionResponse {
                success: false,
                ..
            }))
        ) {
            return Err(LlaError::Plugin(
                "action dispatch did not reject an unknown action safely".to_string(),
            ));
        }

        Ok(())
    }

    fn unload_plugin_path(&mut self, path: &Path) {
        let Ok(path) = path.canonicalize() else {
            return;
        };
        self.plugins.retain(|_, plugin| plugin.path != path);
        self.loaded_paths.remove(&path);
    }

    pub fn print_manifest(&self, plugin_name: &str, permissions_only: bool) -> Result<()> {
        let manifest = self.manifests.get(plugin_name).ok_or_else(|| {
            LlaError::Plugin(format!(
                "Plugin '{}' has no v2 manifest or is not installed",
                plugin_name
            ))
        })?;

        if !permissions_only {
            println!("{} v{}", manifest.plugin.name, manifest.plugin.version);
            println!("  ID: {}", manifest.plugin.id);
            println!("  Runtime: {:?}", manifest.plugin.runtime);
            println!(
                "  API: {}..={}",
                manifest.plugin.api_min, manifest.plugin.api_max
            );
            if !manifest.plugin.description.is_empty() {
                println!("  {}", manifest.plugin.description);
            }
            if !manifest.capabilities.formats.is_empty() {
                println!("  Formats: {}", manifest.capabilities.formats.join(", "));
            }
            if !manifest.fields.is_empty() {
                println!(
                    "  Fields: {}",
                    manifest
                        .fields
                        .iter()
                        .map(|field| field.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }

        println!("Permissions for {}:", manifest.plugin.name);
        println!(
            "  Filesystem: {}",
            if manifest.permissions.filesystem.is_empty() {
                "none".to_string()
            } else {
                manifest.permissions.filesystem.join(", ")
            }
        );
        println!(
            "  Network: {}",
            if manifest.permissions.network.is_empty() {
                "none".to_string()
            } else {
                manifest.permissions.network.join(", ")
            }
        );
        println!("  Process execution: {}", manifest.permissions.process);
        println!("  Clipboard: {}", manifest.permissions.clipboard);
        println!("  Open URL: {}", manifest.permissions.open_url);
        if manifest.plugin.runtime == PluginRuntime::Native {
            println!("  Note: native permissions are declarations; native code is fully trusted.");
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
        let cache_key = (
            path_str.clone(),
            format.to_string(),
            self.decoration_cache_scope(),
        );
        if let Some(fields) = DECORATION_CACHE.get(&cache_key) {
            entry.custom_fields.extend(
                fields
                    .custom_fields
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
            entry.typed_fields.extend(
                fields
                    .typed_fields
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
            self.apply_typed_fields(entry);
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
        let mut new_typed_decorations = HashMap::with_capacity(supported_names.len() * 2);
        for name in supported_names {
            let request = PluginMessage {
                message: Some(Message::Decorate(entry.clone())),
            };

            if let Ok(response) = self.send_request(&name, request) {
                if let Some(Message::DecoratedResponse(decorated)) = response.message {
                    new_decorations.extend(decorated.custom_fields);
                    new_typed_decorations.extend(decorated.typed_fields);
                }
            }
        }

        if !new_decorations.is_empty() || !new_typed_decorations.is_empty() {
            entry
                .custom_fields
                .extend(new_decorations.iter().map(|(k, v)| (k.clone(), v.clone())));
            entry.typed_fields.extend(
                new_typed_decorations
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
            DECORATION_CACHE.insert(
                cache_key,
                CachedDecoration {
                    custom_fields: new_decorations,
                    typed_fields: new_typed_decorations,
                },
            );
        }
        self.apply_typed_fields(entry);
    }

    pub fn decorate_entries(&mut self, entries: &mut [proto::DecoratedEntry], format: &str) {
        let Some(plugin_format) = normalize_plugin_format(format) else {
            return;
        };
        if entries.is_empty() || self.enabled_plugins.is_empty() {
            return;
        }

        let cache_scope = self.decoration_cache_scope();
        let enabled_names: Vec<_> = self.enabled_plugins.iter().cloned().collect();
        let supported_names: Vec<_> = enabled_names
            .into_iter()
            .filter(|name| self.supports_format(name, plugin_format))
            .collect();

        for name in supported_names {
            let is_v2 = self
                .plugins
                .get(&name)
                .is_some_and(|plugin| plugin.api.is_v2());
            if !is_v2 {
                self.decorate_entries_individually(&name, entries);
                continue;
            }

            let request = PluginMessage {
                message: Some(Message::DecorateBatch(proto::BatchDecorateRequest {
                    entries: entries.to_vec(),
                    format: plugin_format.to_string(),
                })),
            };
            let batch_entries = self.send_request(&name, request).ok().and_then(|response| {
                match response.message {
                    Some(Message::DecorateBatchResponse(batch))
                        if batch.entries.len() == entries.len()
                            && entries
                                .iter()
                                .zip(&batch.entries)
                                .all(|(entry, decorated)| entry.path == decorated.path) =>
                    {
                        Some(batch.entries)
                    }
                    _ => None,
                }
            });
            if let Some(batch_entries) = batch_entries {
                for (entry, decorated) in entries.iter_mut().zip(batch_entries) {
                    entry.custom_fields.extend(decorated.custom_fields);
                    entry.typed_fields.extend(decorated.typed_fields);
                }
            } else {
                self.decorate_entries_individually(&name, entries);
            }
        }

        for entry in entries {
            self.apply_typed_fields(entry);
            if !entry.custom_fields.is_empty() || !entry.typed_fields.is_empty() {
                DECORATION_CACHE.insert(
                    (entry.path.clone(), format.to_string(), cache_scope.clone()),
                    CachedDecoration {
                        custom_fields: entry.custom_fields.clone(),
                        typed_fields: entry.typed_fields.clone(),
                    },
                );
            }
        }
    }

    fn decorate_entries_individually(
        &self,
        plugin_name: &str,
        entries: &mut [proto::DecoratedEntry],
    ) {
        for entry in entries {
            let request = PluginMessage {
                message: Some(Message::Decorate(entry.clone())),
            };
            if let Ok(response) = self.send_request(plugin_name, request) {
                if let Some(Message::DecoratedResponse(decorated)) = response.message {
                    entry.custom_fields.extend(decorated.custom_fields);
                    entry.typed_fields.extend(decorated.typed_fields);
                }
            }
        }
    }

    fn decoration_cache_scope(&self) -> String {
        let mut plugins = self.enabled_plugins.iter().cloned().collect::<Vec<_>>();
        plugins.sort();
        plugins.join("\0")
    }

    fn apply_typed_fields(&self, entry: &mut proto::DecoratedEntry) {
        for manifest in self.manifests.values() {
            for field in &manifest.fields {
                let Some(value) = entry.custom_fields.get(&field.name) else {
                    continue;
                };
                let typed = match field.field_type {
                    FieldType::String => {
                        Some(proto::typed_value::Value::StringValue(value.clone()))
                    }
                    FieldType::Integer => value
                        .parse::<i64>()
                        .ok()
                        .map(proto::typed_value::Value::IntegerValue),
                    FieldType::Float => value
                        .parse::<f64>()
                        .ok()
                        .map(proto::typed_value::Value::FloatValue),
                    FieldType::Boolean => value
                        .parse::<bool>()
                        .ok()
                        .map(proto::typed_value::Value::BooleanValue),
                    FieldType::Bytes => value
                        .parse::<u64>()
                        .ok()
                        .map(proto::typed_value::Value::BytesValue),
                    FieldType::Timestamp => value
                        .parse::<u64>()
                        .ok()
                        .map(proto::typed_value::Value::TimestampValue),
                    FieldType::Path => Some(proto::typed_value::Value::PathValue(value.clone())),
                };
                if let Some(value) = typed {
                    entry
                        .typed_fields
                        .entry(field.name.clone())
                        .or_insert(proto::TypedValue { value: Some(value) });
                }
            }
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

    pub fn clean_plugins(&mut self, plugins_dir: &Path) -> Result<()> {
        println!("🔄 Starting plugin cleaning...");

        let mut failed_plugins = Vec::new();

        for entry in fs::read_dir(plugins_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) == Some(".quarantine") {
                    continue;
                }
                let manifest_path = path.join(MANIFEST_FILE_NAME);
                println!("📦 Checking v2 plugin package: {:?}", path);
                let valid = is_regular_directory(&path)
                    && is_regular_file(&manifest_path)
                    && PluginManifest::from_path(&manifest_path)
                        .ok()
                        .filter(|manifest| manifest.supports_host_api(CURRENT_PLUGIN_API_VERSION))
                        .and_then(|manifest| {
                            resolve_manifest_entrypoint(&path, &manifest.plugin.entrypoint)
                                .map(|entrypoint| (manifest, entrypoint))
                        })
                        .is_some_and(|(manifest, entrypoint)| {
                            package::verify_package_checksums(&entrypoint).is_ok()
                                && self.manifest_matches_runtime(&manifest, &entrypoint)
                        });
                if valid {
                    println!("✅ Plugin package is valid: {:?}", path);
                } else {
                    println!("❌ Plugin package is invalid: {:?}", path);
                    failed_plugins.push(path);
                }
                continue;
            }

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

        let quarantine = plugins_dir
            .join(".quarantine")
            .join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string());
        for path in failed_plugins {
            fs::create_dir_all(&quarantine)?;
            let Some(file_name) = path.file_name() else {
                continue;
            };
            let mut destination = quarantine.join(file_name);
            if destination.exists() {
                destination = quarantine.join(format!(
                    "{}-{}",
                    file_name.to_string_lossy(),
                    std::process::id()
                ));
            }
            if let Err(e) = fs::rename(&path, &destination) {
                eprintln!("⚠️ Failed to quarantine invalid plugin {:?}: {}", path, e);
            } else {
                println!(
                    "🗑️ Quarantined invalid plugin at {:?} (recoverable)",
                    destination
                );
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

            let api = if let Ok(create_v2) =
                library.get::<unsafe fn() -> *mut PluginApiV2>(b"_plugin_create_v2")
            {
                let api = create_v2();
                if api.is_null()
                    || (*api).abi_version != CURRENT_PLUGIN_API_VERSION
                    || (*api).min_host_api > CURRENT_PLUGIN_API_VERSION
                    || (*api).max_host_api < CURRENT_PLUGIN_API_VERSION
                {
                    return Ok(false);
                }
                PluginHandle::V2(api)
            } else {
                let create_v1 =
                    match library.get::<unsafe fn() -> *mut PluginApi>(b"_plugin_create") {
                        Ok(create) => create,
                        Err(_) => return Ok(false),
                    };
                let api = create_v1();
                if api.is_null() || (*api).version != LEGACY_PLUGIN_API_VERSION {
                    return Ok(false);
                }
                PluginHandle::V1(api)
            };

            let request = PluginMessage {
                message: Some(Message::GetName(true)),
            };
            let mut buf = Vec::with_capacity(request.encoded_len());
            if request.encode(&mut buf).is_err() {
                return Ok(false);
            }

            let response_vec = match std::panic::catch_unwind(|| api.send(&buf)) {
                Ok(Ok(response)) if !response.is_empty() && response.len() <= 1024 * 1024 => {
                    response
                }
                _ => return Ok(false),
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

fn resolve_manifest_entrypoint(plugin_dir: &Path, entrypoint: &str) -> Option<PathBuf> {
    let exact = plugin_dir.join(entrypoint);
    let platform_name = format!(
        "{}{}{}",
        std::env::consts::DLL_PREFIX,
        entrypoint,
        std::env::consts::DLL_SUFFIX
    );
    let without_prefix = format!("{}{}", entrypoint, std::env::consts::DLL_SUFFIX);
    [
        exact,
        plugin_dir.join(platform_name),
        plugin_dir.join(without_prefix),
    ]
    .into_iter()
    .find(|candidate| is_regular_file(candidate))
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

fn is_regular_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
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
            .discover_plugin_paths_named(&[plugin_dir.clone()], &HashSet::new())
            .unwrap();

        assert!(plugin_dir.is_dir());
    }

    #[test]
    fn resolves_logical_manifest_entrypoint() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join(format!(
            "{}example{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ));
        fs::write(&library, b"not a real library").unwrap();
        assert_eq!(
            resolve_manifest_entrypoint(root.path(), "example").as_deref(),
            Some(library.as_path())
        );
    }

    #[test]
    fn loading_an_invalid_library_returns_an_error() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join(format!(
            "{}invalid{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ));
        fs::write(&library, b"not a dynamic library").unwrap();
        let mut manager = PluginManager::new(Config::default());

        assert!(manager.load_plugin(&library).is_err());
    }
}
