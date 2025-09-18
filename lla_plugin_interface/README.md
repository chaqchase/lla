# `lla` - plugin interface

This crate provides a plugin interface for the `lla` command line tool.

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

## Advanced Plugin Capabilities

### Performance Optimizations

#### Capability Caching

The plugin system automatically caches `GetSupportedFormats` responses to avoid redundant API calls. This significantly improves performance when processing large directories with multiple plugins.

#### Batch Decoration

Plugins can implement batch decoration to process multiple entries in a single request, reducing protobuf serialization overhead:

```rust
// Handle batch decoration request
Some(plugin_message::Message::BatchDecorate(batch_request)) => {
    let mut decorated_entries = Vec::new();

    for entry in batch_request.entries {
        let mut decorated_entry = DecoratedEntry::try_from(entry)?;
        // Process entry...
        decorated_entries.push(decorated_entry.into());
    }

    plugin_message::Message::BatchDecoratedResponse(proto::BatchDecorateResponse {
        entries: decorated_entries,
    })
}
```

If your plugin doesn't support batch operations, the system automatically falls back to individual entry processing.

### Configuration and Context

Plugins receive configuration and context information during initialization through `ConfigRequest` messages:

```rust
Some(plugin_message::Message::Config(config_request)) => {
    // Access user preferences
    let theme = &config_request.theme;
    let show_icons = config_request.config.get("show_icons");
    let shortcuts = &config_request.shortcuts;

    // Adapt plugin behavior based on user preferences
    // Return success response
    plugin_message::Message::ConfigResponse(proto::ConfigResponse {
        success: true,
        error: None,
    })
}
```

Available configuration includes:

- **theme**: Current color theme
- **default_format**: User's preferred output format
- **show_icons**: Whether icons are enabled
- **shortcuts**: User-defined keyboard shortcuts

### Rich Field Types

The `DecoratedEntry` structure now supports rich field type metadata through the `field_types` map:

```rust
// Add a field with type information
decorated_entry.custom_fields.insert("file_size".to_string(), "1024".to_string());
decorated_entry.field_types.insert(
    "file_size".to_string(),
    FieldType {
        field_type: "number".to_string(),
        format: None,
        unit: Some("bytes".to_string()),
    }
);

// Add a date field
decorated_entry.custom_fields.insert("last_backup".to_string(), "1640995200".to_string());
decorated_entry.field_types.insert(
    "last_backup".to_string(),
    FieldType {
        field_type: "date".to_string(),
        format: Some("unix_timestamp".to_string()),
        unit: None,
    }
);

// Add a badge field
decorated_entry.custom_fields.insert("status".to_string(), "verified".to_string());
decorated_entry.field_types.insert(
    "status".to_string(),
    FieldType {
        field_type: "badge".to_string(),
        format: Some("green".to_string()),
        unit: None,
    }
);
```

Supported field types:

- **string**: Plain text (default)
- **number**: Numeric values with optional units
- **date**: Date/time values with format specifications
- **badge**: Status indicators with color information
- **boolean**: True/false values

### Health Monitoring

The plugin system tracks plugin health and reports diagnostic information:

- **Error tracking**: Automatic recording of plugin errors with timestamps
- **Dependency checking**: Report missing dependencies that affect plugin functionality
- **Status monitoring**: Real-time health status visible in the plugin management interface

Plugins can contribute to health monitoring by reporting missing dependencies or other status information.

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

    fn get_category_color(&self, category: &str) -> String {
        match category {
            "Document" => "blue".to_string(),
            "Image" => "green".to_string(),
            "Code" => "yellow".to_string(),
            _ => "gray".to_string(),
        }
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

                // Add category to the entry's custom fields with type information
                if let Some(category) = self.get_category(&decorated_entry) {
                    decorated_entry.custom_fields.insert("category".to_string(), category.clone());
                    decorated_entry.field_types.insert(
                        "category".to_string(),
                        FieldType {
                            field_type: "badge".to_string(),
                            format: Some(self.get_category_color(&category)),
                            unit: None,
                        }
                    );
                }

                plugin_message::Message::DecoratedResponse(decorated_entry.into())
            }

            // Handle batch decoration request for better performance
            Some(plugin_message::Message::BatchDecorate(batch_request)) => {
                let mut decorated_entries = Vec::new();

                for entry in batch_request.entries {
                    let mut decorated_entry = match DecoratedEntry::try_from(entry) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    if let Some(category) = self.get_category(&decorated_entry) {
                        decorated_entry.custom_fields.insert("category".to_string(), category.clone());
                        decorated_entry.field_types.insert(
                            "category".to_string(),
                            FieldType {
                                field_type: "badge".to_string(),
                                format: Some(self.get_category_color(&category)),
                                unit: None,
                            }
                        );
                    }

                    decorated_entries.push(decorated_entry.into());
                }

                plugin_message::Message::BatchDecoratedResponse(proto::BatchDecorateResponse {
                    entries: decorated_entries,
                })
            }

            // Handle configuration messages
            Some(plugin_message::Message::Config(config_request)) => {
                // Plugin can adapt behavior based on user preferences
                plugin_message::Message::ConfigResponse(proto::ConfigResponse {
                    success: true,
                    error: None,
                })
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
5. Adding custom fields to entries with rich type information
6. Supporting batch decoration for improved performance
7. Handling configuration messages from the host application
8. Using field type metadata for enhanced display capabilities
9. Proper error handling
10. Using the plugin declaration macro

The plugin can be compiled as a dynamic library and loaded by the main application at runtime, with full ABI compatibility regardless of the Rust version used to compile either component.

## Migration Guide

### From Basic Plugins

If you have an existing plugin that only handles basic decoration:

1. **Add batch support** (optional but recommended):
   - Implement `BatchDecorate` message handling
   - Process multiple entries in a single request for better performance

2. **Add configuration handling** (optional):
   - Implement `Config` message handling to receive user preferences
   - Adapt plugin behavior based on theme, format settings, etc.

3. **Use rich field types** (optional):
   - Add entries to the `field_types` map alongside `custom_fields`
   - Specify appropriate types ("number", "date", "badge", "boolean") for better display

4. **Update field handling**:
   - Ensure your plugin initializes the new `field_types` map when creating `DecoratedEntry` instances

### Backward Compatibility

All new features are fully backward compatible:

- Existing plugins continue to work without modification
- New message types are ignored by older plugins
- The `field_types` map defaults to empty, treating all fields as strings
- Batch operations automatically fall back to individual processing

The plugin system is designed to gracefully handle version differences and provide optimal performance while maintaining full compatibility with existing plugin implementations.
