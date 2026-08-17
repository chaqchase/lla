use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Type};

/// Export a native API v3 plugin and embed its package manifest.
///
/// The plugin manifest is resolved relative to the plugin crate root. This
/// deliberately makes a missing manifest a compile error.
#[proc_macro]
pub fn export_plugin(input: TokenStream) -> TokenStream {
    let plugin_type = parse_macro_input!(input as Type);
    let manifest_path = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(directory) => std::path::PathBuf::from(directory).join("plugin.toml"),
        Err(error) => {
            return syn::Error::new_spanned(
                &plugin_type,
                format!("CARGO_MANIFEST_DIR is unavailable: {error}"),
            )
            .into_compile_error()
            .into();
        }
    };
    if let Err(error) = lla_plugin_interface::manifest::PluginManifest::from_path(&manifest_path) {
        return syn::Error::new_spanned(
            &plugin_type,
            format!("invalid API v3 plugin.toml: {error}"),
        )
        .into_compile_error()
        .into();
    }
    quote! {
        ::lla_plugin_sdk::__export_plugin_v3!(
            #plugin_type,
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/plugin.toml"))
        );
    }
    .into()
}

/// Export a Rust plugin as a WASI Preview 2 Component Model guest.
#[proc_macro]
pub fn export_component(input: TokenStream) -> TokenStream {
    let plugin_type = parse_macro_input!(input as Type);
    let manifest_path = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(directory) => std::path::PathBuf::from(directory).join("plugin.toml"),
        Err(error) => {
            return syn::Error::new_spanned(
                &plugin_type,
                format!("CARGO_MANIFEST_DIR is unavailable: {error}"),
            )
            .into_compile_error()
            .into();
        }
    };
    let manifest = match lla_plugin_interface::manifest::PluginManifest::from_path(&manifest_path) {
        Ok(manifest) => manifest,
        Err(error) => {
            return syn::Error::new_spanned(
                &plugin_type,
                format!("invalid API v3 plugin.toml: {error}"),
            )
            .into_compile_error()
            .into();
        }
    };
    if manifest.plugin.runtime != lla_plugin_interface::manifest::PluginRuntime::WasmComponent {
        return syn::Error::new_spanned(
            &plugin_type,
            "export_component! requires runtime = \"wasm-component\"",
        )
        .into_compile_error()
        .into();
    }
    let wit = include_str!("../wit/lla-plugin.wit");
    quote! {
        extern crate lla_plugin_sdk as wit_bindgen;
        ::lla_plugin_sdk::wit_bindgen::generate!({ inline: #wit });

        struct LlaPluginComponent;

        impl Guest for LlaPluginComponent {
            fn manifest() -> String {
                include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/plugin.toml")).to_string()
            }

            fn handle(request: Vec<u8>) -> Result<Vec<u8>, String> {
                static PLUGIN: std::sync::OnceLock<std::sync::Mutex<#plugin_type>> =
                    std::sync::OnceLock::new();
                let plugin = PLUGIN.get_or_init(|| {
                    std::sync::Mutex::new(<#plugin_type as Default>::default())
                });
                let mut plugin = plugin
                    .lock()
                    .map_err(|_| "plugin state is poisoned".to_string())?;
                Ok(::lla_plugin_sdk::dispatch(&mut *plugin, &request))
            }
        }

        export!(LlaPluginComponent);
    }
    .into()
}
