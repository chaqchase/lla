# `lla` - plugin interface

This crate provides a plugin interface for the `lla` command line tool.

## Plugin Platform v2

API v2 exports `_plugin_create_v2` and uses `PluginBufferV2`. A response is
copied by the host and released through the plugin's own `free_response`
callback, so Rust allocator ownership never crosses the dynamic-library
boundary. The host releases the API object through its plugin-owned `destroy`
callback before unloading the library. The declaration macro catches unwinding
plugin panics at the FFI boundary and also exports the legacy v1 symbol during
the migration period.

Every distributed plugin must include a `plugin.toml` validated through
`lla_plugin_interface::manifest::PluginManifest`. Manifests declare an API
range instead of a single exact version and describe the plugin's runtime,
capabilities, permissions, and typed fields. Native permissions are
declarative; native plugins must be treated as trusted code.

The host accepts a v2 package only when the loaded runtime exports the v2 ABI
and its name, version, supported formats, and action IDs match the manifest.
Release checksum inventories are verified before native code is loaded.

The protobuf contract includes batch decoration and typed values. Existing
plugin implementations receive batch requests through the v2 adapter, which
serializes them through their established per-entry handler while holding one
thread-safe plugin instance.

## Plugin Architecture

The plugin system in `lla` is designed to be robust and version-independent, using a message-passing architecture that ensures ABI compatibility across different Rust versions. Here's how it works:

### Core Components

1. **Protocol Buffer Interface**

   - All communication between the main application and plugins uses Protocol Buffers
   - Messages are defined in `plugin.proto`, providing a language-agnostic contract
   - Supports various operations like decoration, field formatting, and custom actions

2. **FFI Boundary**
   - Plugins are loaded dynamically using `libloading`
   - Communication crosses the FFI boundary using only C-compatible types
   - Raw bytes are used for data transfer, avoiding Rust-specific ABI details

### ABI Compatibility

The plugin system solves the ABI compatibility problem through several mechanisms:

1. **Message-Based Communication**

   - Instead of direct function calls, all interaction happens through serialized Protocol Buffer messages
   - This eliminates dependency on Rust's internal ABI, which can change between versions
   - Plugins and the main application can be compiled with different Rust versions

2. **Version Control**

   - Each plugin declares its API version
   - The system performs version checking during plugin loading
   - Incompatible plugins are rejected with clear error messages

3. **Stable Interface**
   - The FFI layer uses only C-compatible types, ensuring ABI stability
   - Complex Rust types are serialized before crossing plugin boundaries
   - The Protocol Buffer schema acts as a stable contract between components

### Plugin Development

To create a plugin:

1. Implement the plugin interface defined in the Protocol Buffer schema
2. Use the provided macros and traits for proper FFI setup
3. Compile as a dynamic library (`.so`, `.dll`, or `.dylib`)

The main application will handle loading, version verification, and communication with your plugin automatically.

## Example Plugin

Here's a simple example of a file type categorizer plugin that demonstrates the key concepts:

```rust
use lla_plugin_interface::{DecoratedEntry, Plugin};
use prost::Message as ProstMessage;

/// A simple plugin that categorizes files based on their extensions
pub struct SimpleCategorizerPlugin {
    categories: Vec<(String, Vec<String>)>,  // (category_name, extensions)
}

impl SimpleCategorizerPlugin {
    pub fn new() -> Self {
        Self {
            categories: vec![
                ("Document".to_string(), vec!["txt", "pdf", "doc"].into_iter().map(String::from).collect()),
                ("Image".to_string(), vec!["jpg", "png", "gif"].into_iter().map(String::from).collect()),
                ("Code".to_string(), vec!["rs", "py", "js"].into_iter().map(String::from).collect()),
            ]
        }
    }

    fn get_category(&self, entry: &DecoratedEntry) -> Option<String> {
        let extension = entry.path.extension()?.to_str()?.to_lowercase();

        self.categories.iter()
            .find(|(_, exts)| exts.contains(&extension))
            .map(|(category, _)| category.clone())
    }
}

impl Plugin for SimpleCategorizerPlugin {
    fn handle_raw_request(&mut self, request: &[u8]) -> Vec<u8> {
        use lla_plugin_interface::proto::{self, plugin_message};

        // Decode the incoming protobuf message
        let proto_msg = match proto::PluginMessage::decode(request) {
            Ok(msg) => msg,
            Err(e) => return self.encode_error(&format!("Failed to decode request: {}", e)),
        };

        // Handle different message types
        let response_msg = match proto_msg.message {
            // Return plugin metadata
            Some(plugin_message::Message::GetName(_)) => {
                plugin_message::Message::NameResponse("simple-categorizer".to_string())
            }
            Some(plugin_message::Message::GetVersion(_)) => {
                plugin_message::Message::VersionResponse("0.1.0".to_string())
            }
            Some(plugin_message::Message::GetDescription(_)) => {
                plugin_message::Message::DescriptionResponse(
                    "A simple file categorizer plugin".to_string(),
                )
            }

            // Handle file decoration request
            Some(plugin_message::Message::Decorate(entry)) => {
                let mut decorated_entry = match DecoratedEntry::try_from(entry.clone()) {
                    Ok(e) => e,
                    Err(e) => return self.encode_error(&format!("Failed to convert entry: {}", e)),
                };

                // Add category to the entry's custom fields
                if let Some(category) = self.get_category(&decorated_entry) {
                    decorated_entry.custom_fields.insert("category".to_string(), category);
                }

                plugin_message::Message::DecoratedResponse(decorated_entry.into())
            }

            _ => plugin_message::Message::ErrorResponse("Invalid request type".to_string()),
        };

        // Encode and return the response
        let response = proto::PluginMessage {
            message: Some(response_msg),
        };
        let mut buf = bytes::BytesMut::with_capacity(response.encoded_len());
        response.encode(&mut buf).unwrap();
        buf.to_vec()
    }
}

// Register the plugin with the main application
lla_plugin_interface::declare_plugin!(SimpleCategorizerPlugin);
```

This example demonstrates:

1. Using Protocol Buffers for communication
2. Implementing the `Plugin` trait
3. Handling different message types
4. Processing file metadata
5. Adding custom fields to entries
6. Proper error handling
7. Using the plugin declaration macro

The plugin can be compiled as a dynamic library and loaded by the main application at runtime, with full ABI compatibility regardless of the Rust version used to compile either component.
