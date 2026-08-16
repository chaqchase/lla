use crate::error::{LlaError, Result as LlaResult};
use lla_plugin_interface::{manifest::PluginManifest, proto, MAX_RESPONSE_BYTES};
use prost::Message as _;
use std::path::{Component as PathComponent, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Cache, CacheConfig, Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{
    DirPerms, FilePerms, ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView,
};
use wasmtime_wasi_http::p2::{
    self as wasi_http, HttpResult, WasiHttpCtxView, WasiHttpHooks, WasiHttpView,
};
use wasmtime_wasi_http::WasiHttpCtx;

wasmtime::component::bindgen!({
    path: "../sdk/wit",
    world: "plugin",
});

struct HostState {
    table: ResourceTable,
    wasi: WasiCtx,
    limits: StoreLimits,
    http: WasiHttpCtx,
    http_hooks: DomainAllowlist,
    allow_clipboard: bool,
    allow_open_url: bool,
}

impl lla::plugin::host::Host for HostState {
    fn clipboard_write(&mut self, text: String) -> std::result::Result<(), String> {
        if !self.allow_clipboard {
            return Err("clipboard permission denied".to_string());
        }
        let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
        clipboard.set_text(text).map_err(|error| error.to_string())
    }

    fn open_url(&mut self, url: String) -> std::result::Result<(), String> {
        if !self.allow_open_url {
            return Err("open-url permission denied".to_string());
        }
        let parsed = url::Url::parse(&url).map_err(|error| error.to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("only HTTP(S) URLs may be opened".to_string());
        }
        open::that_detached(url).map_err(|error| error.to_string())
    }
}

struct DomainAllowlist {
    domains: std::collections::HashSet<String>,
}

impl WasiHttpHooks for DomainAllowlist {
    fn send_request(
        &mut self,
        request: hyper::Request<wasi_http::body::HyperOutgoingBody>,
        config: wasi_http::types::OutgoingRequestConfig,
    ) -> HttpResult<wasi_http::types::HostFutureIncomingResponse> {
        let allowed = request
            .uri()
            .host()
            .is_some_and(|host| self.domains.contains(host));
        if !allowed {
            return Err(wasi_http::bindings::http::types::ErrorCode::DestinationNotFound.into());
        }
        Ok(wasi_http::default_send_request(request, config))
    }
}

impl WasiHttpView for HostState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.http_hooks,
        }
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

pub struct WasmPlugin {
    engine: Engine,
    component: Component,
    linker: Linker<HostState>,
    manifest: PluginManifest,
}

impl WasmPlugin {
    pub fn load(path: &Path, packaged_manifest: &PluginManifest) -> LlaResult<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        configure_compilation_cache(&mut config);
        let engine = Engine::new(&config)
            .map_err(|error| LlaError::Plugin(format!("failed to initialize Wasmtime: {error}")))?;
        let component = Component::from_file(&engine, path).map_err(|error| {
            LlaError::Plugin(format!(
                "failed to compile WASM component {}: {error} [wasm-invalid-component]",
                path.display()
            ))
        })?;
        let mut linker = Linker::new(&engine);
        Plugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state).map_err(|error| {
            LlaError::Plugin(format!("failed to link lla host capabilities: {error}"))
        })?;
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|error| LlaError::Plugin(format!("failed to link WASI Preview 2: {error}")))?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_sync(&mut linker).map_err(|error| {
            LlaError::Plugin(format!("failed to link allowlisted WASI HTTP: {error}"))
        })?;

        let mut store = build_store(&engine, packaged_manifest, &[])?;
        let instance = Plugin::instantiate(&mut store, &component, &linker).map_err(|error| {
            LlaError::Plugin(format!(
                "failed to instantiate WASM component: {error} [wasm-instantiation-failed]"
            ))
        })?;
        let embedded = instance.call_manifest(&mut store).map_err(|error| {
            LlaError::Plugin(format!(
                "WASM manifest export trapped: {error} [wasm-manifest-trap]"
            ))
        })?;
        let manifest: PluginManifest = toml::from_str(&embedded).map_err(|error| {
            LlaError::Plugin(format!(
                "WASM component embeds an invalid manifest: {error}"
            ))
        })?;
        manifest.validate().map_err(LlaError::Plugin)?;
        if manifest != *packaged_manifest {
            return Err(LlaError::Plugin(
                "packaged and embedded WASM manifests do not match".to_string(),
            ));
        }
        Ok(Self {
            engine,
            component,
            linker,
            manifest,
        })
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn send(&mut self, request: &[u8], timeout: Duration) -> LlaResult<Vec<u8>> {
        let mut store = build_store(&self.engine, &self.manifest, request)?;
        store.set_epoch_deadline(1);
        let engine = self.engine.clone();
        let (cancel_timeout, timeout_signal) = std::sync::mpsc::channel();
        let timed_out = Arc::new(AtomicBool::new(false));
        let timeout_state = Arc::clone(&timed_out);
        std::thread::spawn(move || {
            if matches!(
                timeout_signal.recv_timeout(timeout),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                timeout_state.store(true, Ordering::Release);
                engine.increment_epoch();
            }
        });
        let instance =
            Plugin::instantiate(&mut store, &self.component, &self.linker).map_err(|error| {
                let _ = cancel_timeout.send(());
                if timed_out.load(Ordering::Acquire) {
                    LlaError::Plugin("WASM plugin timed out [wasm-timeout]".to_string())
                } else {
                    LlaError::Plugin(format!(
                        "failed to instantiate WASM component: {error} [wasm-instantiation-failed]"
                    ))
                }
            })?;
        let result = instance.call_handle(&mut store, request);
        let _ = cancel_timeout.send(());
        let response = result
            .map_err(|error| {
                if timed_out.load(Ordering::Acquire) {
                    LlaError::Plugin("WASM plugin timed out [wasm-timeout]".to_string())
                } else {
                    LlaError::Plugin(format!("WASM plugin trapped: {error} [wasm-trap]"))
                }
            })?
            .map_err(|error| {
                LlaError::Plugin(format!("WASM plugin error: {error} [wasm-plugin-error]"))
            })?;
        if response.len() > MAX_RESPONSE_BYTES {
            return Err(LlaError::Plugin(format!(
                "WASM response exceeds the {} MiB limit [wasm-response-too-large]",
                MAX_RESPONSE_BYTES / (1024 * 1024)
            )));
        }
        Ok(response)
    }
}

fn configure_compilation_cache(config: &mut Config) {
    let Some(directory) = compilation_cache_dir() else {
        return;
    };
    let mut cache_config = CacheConfig::new();
    cache_config.with_directory(directory);
    if let Ok(cache) = Cache::new(cache_config) {
        config.cache(Some(cache));
    }
}

fn compilation_cache_dir() -> Option<PathBuf> {
    std::env::var_os("LLA_WASMTIME_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::cache_dir().map(|directory| directory.join("lla").join("wasmtime")))
}

fn build_store(
    engine: &Engine,
    manifest: &PluginManifest,
    request: &[u8],
) -> LlaResult<Store<HostState>> {
    let mut wasi = WasiCtxBuilder::new();
    let data_root = std::env::var_os("LLA_PLUGIN_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".config/lla/plugins")))
        .unwrap_or_else(|| PathBuf::from(".lla-plugin-data"))
        .join(&manifest.plugin.name);
    std::fs::create_dir_all(&data_root)?;
    wasi.env("LLA_PLUGIN_DATA_DIR", "/data");
    wasi.preopened_dir(
        &data_root,
        "/data",
        DirPerms::READ | DirPerms::MUTATE,
        FilePerms::READ | FilePerms::WRITE,
    )
    .map_err(|error| {
        LlaError::Plugin(format!(
            "failed to preopen private plugin data {}: {error}",
            data_root.display()
        ))
    })?;
    let filesystem = &manifest.permissions.filesystem;
    if !filesystem.is_empty() {
        let accepts_user_paths = filesystem
            .iter()
            .any(|scope| scope.ends_with(":user-path") || scope == "write:selected-destination");
        let mut roots = std::collections::BTreeMap::<(PathBuf, PathBuf), PreopenAccess>::new();
        for requested in request_paths(request, accepts_user_paths) {
            let access = PreopenAccess::for_request(requested.kind, filesystem);
            if !access.directory_read {
                continue;
            }
            let (host_root, guest_root) = preopen_roots(&requested.path, access.tree)?;
            roots
                .entry((host_root, guest_root))
                .or_default()
                .merge(access);
        }
        for ((host_root, guest_root), access) in roots {
            let guest = guest_root.to_string_lossy().to_string();
            let mut dir_perms = DirPerms::READ;
            if access.directory_mutate {
                dir_perms |= DirPerms::MUTATE;
            }
            let mut file_perms = FilePerms::empty();
            if access.file_read {
                file_perms |= FilePerms::READ;
            }
            if access.file_write {
                file_perms |= FilePerms::WRITE;
            }
            wasi.preopened_dir(&host_root, &guest, dir_perms, file_perms)
                .map_err(|error| {
                    LlaError::Plugin(format!(
                        "failed to preopen approved path {}: {error}",
                        host_root.display()
                    ))
                })?;
        }
    }
    let state = HostState {
        table: ResourceTable::new(),
        wasi: wasi.build(),
        limits: StoreLimitsBuilder::new()
            .memory_size(128 * 1024 * 1024)
            .build(),
        http: WasiHttpCtx::new(),
        http_hooks: DomainAllowlist {
            domains: manifest.permissions.network.iter().cloned().collect(),
        },
        allow_clipboard: manifest.permissions.clipboard,
        allow_open_url: manifest.permissions.open_url,
    };
    let mut store = Store::new(engine, state);
    store.limiter(|state| &mut state.limits);
    store.set_epoch_deadline(u64::MAX);
    Ok(store)
}

#[derive(Clone, Copy)]
enum RequestPathKind {
    Selection,
    UserPath,
}

struct RequestedPath {
    path: PathBuf,
    kind: RequestPathKind,
}

#[derive(Clone, Copy, Default)]
struct PreopenAccess {
    directory_read: bool,
    directory_mutate: bool,
    file_read: bool,
    file_write: bool,
    tree: bool,
}

impl PreopenAccess {
    fn for_request(kind: RequestPathKind, scopes: &[String]) -> Self {
        let has = |scope: &str| scopes.iter().any(|candidate| candidate == scope);
        match kind {
            RequestPathKind::Selection => Self {
                directory_read: has("metadata:selection")
                    || has("metadata:tree")
                    || has("read:selection")
                    || has("read:tree")
                    || has("write:tree")
                    || has("delete:selection"),
                directory_mutate: has("write:tree") || has("delete:selection"),
                file_read: has("read:selection") || has("read:tree"),
                file_write: has("write:tree"),
                tree: has("metadata:tree") || has("read:tree") || has("write:tree"),
            },
            RequestPathKind::UserPath => Self {
                directory_read: has("read:user-path")
                    || has("write:user-path")
                    || has("write:selected-destination"),
                directory_mutate: has("write:user-path") || has("write:selected-destination"),
                file_read: has("read:user-path"),
                file_write: has("write:user-path") || has("write:selected-destination"),
                tree: false,
            },
        }
    }

    fn merge(&mut self, other: Self) {
        self.directory_read |= other.directory_read;
        self.directory_mutate |= other.directory_mutate;
        self.file_read |= other.file_read;
        self.file_write |= other.file_write;
        self.tree |= other.tree;
    }
}

fn request_paths(request: &[u8], user_path_access: bool) -> Vec<RequestedPath> {
    use proto::plugin_message::Message;
    let Ok(message) = proto::PluginMessage::decode(request) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    match message.message {
        Some(Message::Decorate(entry)) => paths.push(RequestedPath {
            path: PathBuf::from(entry.path),
            kind: RequestPathKind::Selection,
        }),
        Some(Message::DecorateBatch(batch)) => {
            paths.extend(batch.entries.into_iter().map(|entry| RequestedPath {
                path: PathBuf::from(entry.path),
                kind: RequestPathKind::Selection,
            }));
        }
        Some(Message::FormatField(request)) => {
            if let Some(entry) = request.entry {
                paths.push(RequestedPath {
                    path: PathBuf::from(entry.path),
                    kind: RequestPathKind::Selection,
                });
            }
        }
        Some(Message::Action(action)) => {
            if user_path_access {
                for value in action.arguments.into_values() {
                    collect_value_paths(value, &mut paths);
                }
            }
        }
        _ => {}
    }
    paths
}

fn collect_value_paths(value: proto::TypedValue, paths: &mut Vec<RequestedPath>) {
    use proto::typed_value::Value;
    match value.value {
        Some(Value::PathValue(path)) => paths.push(RequestedPath {
            path: PathBuf::from(path),
            kind: RequestPathKind::UserPath,
        }),
        Some(Value::ListValue(list)) => {
            for value in list.values {
                collect_value_paths(value, paths);
            }
        }
        Some(Value::ObjectValue(object)) => {
            for value in object.fields.into_values() {
                collect_value_paths(value, paths);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
fn canonical_preopen_root(path: &Path, tree_access: bool) -> LlaResult<PathBuf> {
    preopen_roots(path, tree_access).map(|(host_root, _)| host_root)
}

fn preopen_roots(path: &Path, tree_access: bool) -> LlaResult<(PathBuf, PathBuf)> {
    if path
        .components()
        .any(|component| component == PathComponent::ParentDir)
    {
        return Err(LlaError::Plugin(format!(
            "parent traversal is not allowed in granted path {}",
            path.display()
        )));
    }
    let is_absolute = path.is_absolute();
    let absolute = if is_absolute {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let metadata = std::fs::symlink_metadata(&absolute).ok();
    let host_candidate = if tree_access
        && metadata
            .as_ref()
            .is_some_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
    {
        absolute
    } else {
        absolute
            .parent()
            .ok_or_else(|| LlaError::Plugin("granted path has no parent".to_string()))?
            .to_path_buf()
    };
    let host_root = host_candidate.canonicalize().map_err(|error| {
        LlaError::Plugin(format!(
            "failed to canonicalize granted directory {}: {error}",
            host_candidate.display()
        ))
    })?;
    let guest_root = if is_absolute {
        host_candidate
    } else if tree_access
        && metadata
            .as_ref()
            .is_some_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
    {
        path.to_path_buf()
    } else {
        path.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    Ok((host_root, guest_root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preopen_roots_reject_parent_traversal_and_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        assert!(canonical_preopen_root(Path::new("../escape"), true).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = tempfile::tempdir().unwrap();
            let link = root.path().join("escape");
            symlink(outside.path(), &link).unwrap();
            assert_eq!(
                canonical_preopen_root(&link, true).unwrap(),
                root.path().canonicalize().unwrap()
            );
        }
    }

    #[test]
    fn preopen_roots_preserve_the_guest_path_alias() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("entry.txt");
        std::fs::write(&file, b"test").unwrap();
        let (host_root, guest_root) = preopen_roots(&file, false).unwrap();

        assert_eq!(host_root, root.path().canonicalize().unwrap());
        assert_eq!(guest_root, root.path());
    }

    #[test]
    fn preopen_roots_preserve_relative_guest_paths() {
        let current_dir = std::env::current_dir().unwrap();
        let relative = Path::new("Cargo.toml");
        let (host_root, guest_root) = preopen_roots(relative, false).unwrap();

        assert_eq!(host_root, current_dir.canonicalize().unwrap());
        assert_eq!(guest_root, Path::new("."));
    }

    #[test]
    fn network_allowlist_is_exact() {
        let allowlist = DomainAllowlist {
            domains: ["api.example.com".to_string()].into_iter().collect(),
        };
        assert!(allowlist.domains.contains("api.example.com"));
        assert!(!allowlist.domains.contains("example.com"));
        assert!(!allowlist.domains.contains("evil.api.example.com"));
    }

    #[test]
    fn destination_grants_do_not_make_selected_entries_writable() {
        let scopes = vec![
            "read:tree".to_string(),
            "write:selected-destination".to_string(),
        ];
        let selection = PreopenAccess::for_request(RequestPathKind::Selection, &scopes);
        let destination = PreopenAccess::for_request(RequestPathKind::UserPath, &scopes);

        assert!(selection.file_read);
        assert!(!selection.file_write);
        assert!(!selection.directory_mutate);
        assert!(destination.file_write);
        assert!(destination.directory_mutate);
    }
}
