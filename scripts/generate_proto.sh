#!/bin/bash
set -e

cd "$(dirname "$0")/.."

# Build with the regenerate-protobuf feature
echo "Building with regenerate-protobuf feature..."
cargo build -p lla_plugin_interface --features regenerate-protobuf

echo "Successfully generated protobuf bindings"
