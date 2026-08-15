# lla Plugins

This is a list of all the plugins available for lla.

## Installation

You can install all prebuilt plugins at once using the default installer:

```bash
lla install
```

To force fetching the latest prebuilt archive (useful after a release):

```bash
# install pre-built plugins
lla install --prebuilt
# or install from a repo
lla install --git https://github.com/chaqchase/lla
```

If you prefer to build from source, install them like this:

```bash
git clone https://github.com/chaqchase/lla
cd lla/plugins/
cargo build --release
```

then create a directory for the plugin under the lla plugins directory and copy
both its `plugin.toml` and generated `.so`, `.dll`, or `.dylib` into that
directory. Keeping the manifest beside the entrypoint enables v2 compatibility,
permissions, and typed-field validation.

## Plugin Platform v2

Bundled plugins use the v2 package layout:

```text
plugin-name/
├── plugin.toml
├── libplugin_name.so   # .dylib on macOS, .dll on Windows
├── checksums.toml
└── README.md            # optional
```

The manifest supplies a stable plugin ID, supported host API range, runtime,
entrypoint, capabilities, permissions, and typed listing fields. The v2 ABI
keeps allocation and deallocation inside the plugin and supports batch entry
decoration. Legacy flat v1 libraries remain loadable during migration.
Release packages include SHA-256 coverage for both the manifest and native
entrypoint; installation and `plugin doctor` reject a mismatched package before
executing it.

A minimal manifest looks like this:

```toml
[plugin]
id = "dev.example.my-plugin"
name = "my_plugin"
version = "1.0.0"
api_min = 2
api_max = 2
runtime = "native"
entrypoint = "my_plugin"

[capabilities]
decorates_entries = true
formats = ["default", "long"]
actions = ["help"]
machine_output = true

[permissions]
filesystem = ["read:selection"]

[[fields]]
name = "score"
type = "integer"
sortable = true
filterable = true
```

`entrypoint` is a package-local logical name. lla adds the current platform's
library prefix and suffix. Manifest IDs, field names, duplicate declarations,
and entrypoint confinement are validated before loading.

Filesystem permissions use validated scopes such as `metadata:selection`,
`read:tree`, `write:user-path`, or `delete:quarantine`. Network entries are
domain names (or `"*"` when the endpoint is inherently dynamic). Each plugin's
private configuration/data directory is host-managed and implicit; filesystem
permissions describe access outside that private namespace. Native permissions
remain declarations because native libraries are trusted code.
Set `LLA_PLUGIN_DATA_DIR` to relocate the host-managed plugin data root; the
diagnostic command uses an isolated temporary root automatically.

Useful diagnostics:

```bash
lla plugin doctor
lla plugin info file_hash
lla plugin permissions folder_cleaner
```

lla searches the writable `plugins_dir` first, followed by configured
`plugin_dirs` and platform system locations. This allows package managers to
upgrade system plugins without modifying a user's home directory.

Repository and release verification can run the same manifest, checksum,
metadata, action, formatter, single-decoration, and batch-decoration checks:

```bash
./scripts/build_plugins.sh --target "$(rustc -vV | sed -n 's/^host: //p')"
./scripts/verify_plugins_v2.sh dist/plugins-<os>-<arch>
```

## Available Plugins

- [categorizer](https://github.com/chaqchase/lla/tree/main/plugins/categorizer): Categorizes files based on their extensions and metadata
- [code_complexity](https://github.com/chaqchase/lla/tree/main/plugins/code_complexity): Analyzes code complexity using various metrics
- [code_snippet_extractor](https://github.com/chaqchase/lla/tree/main/plugins/code_snippet_extractor): A plugin for extracting and managing code snippets
- [dirs_meta](https://github.com/chaqchase/lla/tree/main/plugins/dirs_meta): Shows directories metadata
- [duplicate_file_detector](https://github.com/chaqchase/lla/tree/main/plugins/duplicate_file_detector): A plugin for the lla that detects duplicate files.
- [file_hash](https://github.com/chaqchase/lla/tree/main/plugins/file_hash): Displays the hash of each file
- [file_meta](https://github.com/chaqchase/lla/tree/main/plugins/file_meta): Displays the file metadata of each file
- [file_tagger](https://github.com/chaqchase/lla/tree/main/plugins/file_tagger): A plugin for tagging files and filtering by tags
- [flush_dns](https://github.com/chaqchase/lla/tree/main/plugins/flush_dns): Flush DNS cache on macOS, Linux, and Windows
- [folder_cleaner](https://github.com/chaqchase/lla/tree/main/plugins/folder_cleaner): Safety-first folder organization and cleanup plugin for lla
- [git_status](https://github.com/chaqchase/lla/tree/main/plugins/git_status): Shows the git status of each file
- [google_meet](https://github.com/chaqchase/lla/tree/main/plugins/google_meet): Google Meet plugin for creating meeting rooms and managing links
- [google_search](https://github.com/chaqchase/lla/tree/main/plugins/google_search): Google search with autosuggestions, history management, and clipboard fallback
- [jwt](https://github.com/chaqchase/lla/tree/main/plugins/jwt): JWT decoder and analyzer with search and validation capabilities
- [keyword_search](https://github.com/chaqchase/lla/tree/main/plugins/keyword_search): Searches file contents for user-specified keywords
- [last_git_commit](https://github.com/chaqchase/lla/tree/main/plugins/last_git_commit): A plugin for the lla that provides the last git commit hash
- [npm](https://github.com/chaqchase/lla/tree/main/plugins/npm): NPM package search with bundlephobia integration and favorites management
- [sizeviz](https://github.com/chaqchase/lla/tree/main/plugins/sizeviz): File size visualizer plugin for lla
- [youtube](https://github.com/chaqchase/lla/tree/main/plugins/youtube): YouTube search with autosuggestions and history management
- [file_mover](https://github.com/chaqchase/lla/tree/main/plugins/file_mover): A plugin that provides an intuitive clipboard-based interface for moving files and directories.
- [file_copier](https://github.com/chaqchase/lla/tree/main/plugins/file_copier): A plugin that provides an intuitive clipboard-based interface for copying files and directories.
- [file_remover](https://github.com/chaqchase/lla/tree/main/plugins/file_remover): A plugin that provides an interactive interface for safely removing files and directories.
- [file_organizer](https://github.com/chaqchase/lla/tree/main/plugins/file_organizer): A plugin for organizing files using various strategies
- [kill_process](https://github.com/chaqchase/lla/tree/main/plugins/kill_process): Interactive process management plugin for listing and terminating system processes with cross-platform support
