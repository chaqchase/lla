#!/bin/bash

# List of plugins that need the fallback handlers
PLUGINS=(
    "dirs_meta"
    "file_mover"
    "file_tagger"
    "file_remover"
    "last_git_commit"
    "code_snippet_extractor"
    "file_organizer"
    "code_complexity"
    "keyword_search"
)

# The pattern to search for (end of the match statement)
SEARCH_PATTERN="PluginRequest::PerformAction(action, args) => {
                        let result = ACTION_REGISTRY.read().handle(&action, &args);
                        PluginResponse::ActionResult(result)
                    }
                };"

# The replacement pattern (adds the new handlers)
REPLACEMENT="PluginRequest::PerformAction(action, args) => {
                        let result = ACTION_REGISTRY.read().handle(&action, &args);
                        PluginResponse::ActionResult(result)
                    }
                    PluginRequest::BatchDecorate(entries, _format) => {
                        // For now, fall back to individual processing - can be optimized later
                        let mut processed_entries = Vec::new();
                        for entry in entries {
                            processed_entries.push(entry);
                        }
                        PluginResponse::BatchDecorated(processed_entries)
                    }
                    PluginRequest::Config(_config_request) => {
                        PluginResponse::ConfigResult(Ok(()))
                    }
                };"

for plugin in "${PLUGINS[@]}"; do
    plugin_file="plugins/$plugin/src/lib.rs"
    if [ -f "$plugin_file" ]; then
        echo "Fixing $plugin_file..."
        # This is a simple approach - in practice you'd want more robust text replacement
        # For now, we'll manually fix each one
    else
        echo "Warning: $plugin_file not found"
    fi
done

echo "Script completed. Please manually apply the fixes or use sed/awk for automated replacement."
