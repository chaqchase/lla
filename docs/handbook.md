# lla architecture and Plugin Platform handbook

This is the canonical technical guide to the `lla` workspace and Plugin
Platform v3. It is intended for four audiences:

- users installing, enabling, and running plugins;
- plugin authors building native or WebAssembly packages;
- contributors changing the host, SDK, interface, or formatters;
- maintainers building and publishing releases.

The root [README](../README.md) is the product overview. This handbook explains
how the implementation works, how to create a complete plugin, and how to
operate the plugin lifecycle safely. Focused references remain available in
[`docs/plugins`](plugins/README.md) and are linked throughout this guide.

## Contents

1. [System overview](#system-overview)
2. [Workspace map](#workspace-map)
3. [CLI execution architecture](#cli-execution-architecture)
4. [Using lla](#using-lla)
5. [Plugin Platform architecture](#plugin-platform-architecture)
6. [Plugin lifecycle](#plugin-lifecycle)
7. [Install and use plugins](#install-and-use-plugins)
8. [Create a complete native plugin](#create-a-complete-native-plugin)
9. [Convert the example to WebAssembly](#convert-the-example-to-webassembly)
10. [Manifest reference](#manifest-reference)
11. [Rust SDK reference](#rust-sdk-reference)
12. [Fields, formatting, and machine output](#fields-formatting-and-machine-output)
13. [Typed actions and output](#typed-actions-and-output)
14. [Permissions and security](#permissions-and-security)
15. [Discovery, configuration, and aliases](#discovery-configuration-and-aliases)
16. [Packaging and integrity](#packaging-and-integrity)
17. [Testing and verification](#testing-and-verification)
18. [Performance and reliability](#performance-and-reliability)
19. [Debugging and troubleshooting](#debugging-and-troubleshooting)
20. [Contributing a bundled plugin](#contributing-a-bundled-plugin)
21. [Release architecture](#release-architecture)
22. [Compatibility and migration](#compatibility-and-migration)
23. [Source-of-truth index](#source-of-truth-index)

## System overview

`lla` is a Rust command-line application that lists filesystem entries, enriches
them with metadata, optionally decorates them through plugins, sorts and
filters them, and sends them to a human or machine formatter.

```text
command line + config
        │
        ▼
argument resolution ── command dispatch
        │
        ▼
lister ── filesystem/archive traversal
        │
        ▼
metadata + filters + directory sizing
        │
        ▼
PluginManager ── discovery, validation, decoration, typed fields
        │
        ▼
sorter
        │
        ▼
formatter ── terminal view or JSON/NDJSON/CSV
```

Subcommands such as `install`, `plugin run`, `diff`, `jump`, and `upgrade`
branch from command dispatch and do not necessarily use the listing pipeline.

The default Cargo feature is `dynamic-plugins`, which enables native plugins
without embedding Wasmtime. `wasm-plugins` opts into the Component Model runtime
and implies `dynamic-plugins`. A binary compiled with `--no-default-features`
retains core listing behavior but uses the static plugin-manager stub and
reports that dynamic plugins are unavailable.

## Workspace map

The workspace shares one product version. Its major directories are:

| Directory | Crate or role | Responsibility |
| --- | --- | --- |
| `lla/` | `lla` | CLI, configuration, listing, filtering, sorting, formatting, plugin host, installer |
| `interface/` | `lla_plugin_interface` | API v3 manifest model, protobuf messages, native ABI, limits |
| `macros/` | `lla_plugin_sdk_macros` | Compile-time manifest validation and native/component exports |
| `sdk/` | `lla_plugin_sdk` | Maintained Rust authoring API, values, responses, action arguments, dispatch |
| `utils/` | `lla_plugin_utils` | Optional shared UI, configuration, formatting, cache, syntax, and trash helpers |
| `plugins/` | bundled plugin crates | Official plugins built into release archives |
| `themes/` | theme assets | Built and published separately from plugins |
| `scripts/` | build/verifier scripts | Plugin archive generation, API v3 verification, generated catalog/protobuf |
| `docs/` | documentation | User, author, architecture, migration, and release guidance |

The interface crate is the low-level compatibility boundary. Plugin authors
should normally depend only on `lla_plugin_sdk`; it re-exports the interface
types needed by implementations and the export macros needed at the crate root.

## CLI execution architecture

### Startup and configuration

At startup, `lla`:

1. loads built-in defaults;
2. loads `~/.config/lla/config.toml` if present;
3. searches from the current directory upward for the nearest `.lla.toml` and
   merges it as a project profile;
4. selects the theme;
5. parses CLI arguments using the effective configuration as defaults;
6. discovers only the plugins required for the requested operation;
7. dispatches the command.

Use these commands to inspect configuration layers:

```bash
lla config
lla config show-effective
lla config diff --default
```

CLI flags override effective configuration for the current invocation. Plugin
enable/disable commands persist changes to the global configuration.

### Listing pipeline

The normal listing path is deliberately staged:

1. **Select a lister.** Basic, recursive, fuzzy, single-file, and archive paths
   produce filesystem paths or virtual archive entries.
2. **Read metadata.** The host builds `DecoratedEntry` values with an
   `EntryMetadata` record. Symlink handling and optional directory-size
   calculation happen here.
3. **Apply filters.** Name, extension, glob, regex, size, timestamps, file type,
   dotfile, Git-ignore, and preset filters remove entries before rendering.
4. **Decorate entries.** Enabled plugins supporting the selected plugin format
   receive batches of entries and may add string and typed fields.
5. **Sort.** Name, size, and date sorters apply directory-first, reverse,
   case-sensitive, and natural-order options.
6. **Format.** A terminal formatter or machine serializer produces output.

Expensive metadata such as recursive directory sizes, extended attributes,
security context, and mount details is only calculated when requested.

### Listers

`FileLister` is the host abstraction for obtaining paths. Implementations live
under `lla/src/lister/`:

- `BasicLister`: immediate directory contents;
- `RecursiveLister`: recursive traversal with depth and safety limits;
- `FuzzyLister`: candidates for interactive fuzzy selection;
- archive support: `.zip`, `.tar`, `.tar.gz`, and `.tgz` virtual entries.

### Filters and sorters

Filters implement the `FileFilter` abstraction and can be composed. The host
supports case-sensitive/insensitive matching, extension and pattern matching,
glob and regular expressions, and metadata ranges. Sorters implement
alphabetical, size, and date ordering.

### Formatters and views

Human formatters live under `lla/src/formatter/`:

| CLI | Host formatter | Purpose |
| --- | --- | --- |
| `lla` | default | compact names and optional plugin suffixes |
| `lla -l` | long | detailed configurable columns |
| `lla -T` | table | structured configurable columns |
| `lla -t` | tree | hierarchy |
| `lla -g` | grid | terminal-width-aware grid |
| `lla -R` | recursive | grouped recursive listing |
| `lla -G` | Git | repository status view |
| `lla --timeline` | timeline | time-grouped listing |
| `lla -S` | sizemap | size bars and totals |
| `lla -F` | fuzzy | interactive selection |
| `lla --json` | JSON | streamed JSON array |
| `lla --ndjson` | NDJSON | one JSON object per entry |
| `lla --csv` | CSV | stable fixed columns |

Plugin Platform v3 currently normalizes plugin formatting to `default` and
`long`; table uses the `long` plugin representation. Other human views do not
currently invoke plugin formatting through the v3 format contract. Typed fields
already present on an entry are included in JSON/NDJSON under `plugin`.

## Using lla

This section consolidates the public workflows. Run `lla --help` or
`lla <subcommand> --help` for the exact options supported by the installed
version.

### Basic listing and views

```bash
lla                         # current directory, default view
lla /path/to/directory      # another directory
lla README.md               # one file
lla -l                      # long view
lla -T                      # table view
lla -t -d 3                 # tree view with depth 3
lla -g                      # terminal-width-aware grid
lla -R -d 3                 # recursive view
lla -G                      # Git view
lla --timeline              # group by modification period
lla -S --include-dirs       # sizemap with recursive directory sizes
lla -F                      # interactive fuzzy finder
```

View flags override `default_format` from configuration. `--include-dirs` can
be expensive because it recursively calculates directory sizes.

### Visibility and file types

```bash
lla -a                      # include dotfiles
lla -A                      # include dotfiles except . and ..
lla --no-dotfiles
lla --dotfiles-only
lla --files-only
lla --dirs-only
lla --symlinks-only
lla --no-symlinks
lla --files-only --show-symlinks
```

`--show-symlinks` lets a symlink participate when its target matches a
file/directory-only filter. `--dereference` displays target metadata while
retaining link identity. `--no-symlink-target` hides the `-> target` suffix.

### Filtering

```bash
lla -f '.rs'                        # name/extension filter
lla --size '>10M'
lla --size '5K..2G'
lla --modified '<7d'
lla --modified '2026-01-01..2026-12-31'
lla --created '<24h'
lla --preset source-files
lla --case-sensitive -f README
```

Filters compose with visibility, traversal, sort, and output options. Named
presets are defined in configuration. Repeat `--refine <expression>` to apply
additional name/path filters sequentially after the normal walk and plugin
decoration, for example `lla --refine '*.rs' --refine test`. Each expression
uses the same filter language as `--filter`, and every refinement must match.
Despite the CLI help's historical “previous listing/cache” wording, the current
implementation does not read a persisted prior listing and still walks the
filesystem normally.

### Git-ignore behavior

```bash
lla --respect-gitignore
lla --no-gitignore
```

`--respect-gitignore` honors repository, parent, global, and exclude rules.
`--no-gitignore` disables configured Git-ignore filtering for one invocation.

### Sorting

```bash
lla -s name
lla -s size -r
lla -s date
lla --sort-dirs-first --sort-natural
lla --sort-case-sensitive
```

The supported primary sort keys are `name`, `size`, and `date`. Natural sorting
orders embedded numbers as humans expect; reverse and directories-first are
orthogonal modifiers.

### Long-view metadata

```bash
lla -l --inode --links --allocated-size
lla -l --extended --context --mounts
lla -l --relative-dates
lla -l --date-format '%Y-%m-%d %H:%M'
lla -l --hide-group
lla -l --permission-format octal
```

Formatter columns can be set precisely in `config.toml`; see
[Fields, formatting, and machine output](#fields-formatting-and-machine-output).

### Icons, colors, and hyperlinks

```bash
lla --icons
lla --no-icons
lla --no-color
lla --hyperlink auto
lla --hyperlink always
lla --hyperlink never
lla theme
```

OSC 8 hyperlinks are selected automatically by default when terminal support is
detected. `always` is useful in controlled tests and compatible terminals.

### Machine output

```bash
lla --json
lla --json --pretty
lla --ndjson
lla --csv
```

JSON, NDJSON, and CSV are mutually exclusive. `--pretty` only affects JSON.
Filters, traversal, and sorting still apply; only rendering changes. JSON and
NDJSON include typed plugin fields when decoration is active.

### Archives

```bash
lla archive.zip -t
lla source.tar.gz -l
lla archive.tgz --json --pretty
lla archive.zip -l -f '.rs'
```

Archives are read as virtual directories without extraction. Supported formats
are ZIP, TAR, TAR.GZ, and TGZ. Plugin decoration follows the same view mapping:
only default, long, and table currently invoke plugin formatting. Archive paths
are virtual entries, so plugins that assume every path is directly openable on
the host filesystem must handle missing or inaccessible paths safely.

### Content search and plugin pipelines

```bash
lla --search 'TODO'
lla --search 'fn main' --search-context 4
lla --search 'secret' --search-pipe security_audit:audit
```

Content search uses `ripgrep`. A search pipeline runs a plugin action after the
search. Its syntax is `plugin:action[:arg...]`: colon-separated extra arguments
are placed first, followed by each unique matched file path in first-match
order. For example, `security_audit:audit:strict` invokes `audit` with `strict`
and then the matched paths. Multiple `--search-pipe` flags run sequentially.
The normal human/JSON/NDJSON/CSV search result is rendered first; each action
then renders its own result, so pipelines are not a single combined machine
document. No matches means no action call.

### Diff

```bash
lla diff src ../backup/src
lla diff src --git
lla diff src --git --git-ref HEAD~1
lla diff Cargo.lock ../backup/Cargo.lock
```

Directory diff reports status, left/right sizes, and deltas. File diff reports
line and size statistics plus a unified diff, with binary detection.

### Jump navigation

```bash
lla jump --setup
lla jump
lla jump --add /path/to/project
lla jump --remove /path/to/project
lla jump --list
lla jump --clear-history
```

The setup command installs shell integration so the selected directory can
change the parent shell's working directory. Bookmarks and recent history feed
the interactive selector.

### Configuration and initialization

```bash
lla init
lla config
lla config show-effective
lla config diff --default
lla config --set plugins_dir ~/.config/lla/plugins
```

`lla init` runs the configuration wizard. `config --set` modifies supported
global keys; project `.lla.toml` profiles remain normal TOML files.

### Shell completion

```bash
lla completion bash
lla completion zsh
lla completion fish
lla completion powershell
lla completion elvish
lla completion zsh --output ./_lla
lla completion zsh --path ~/.zsh/completions/_lla
```

Without either option, the command installs to its shell-specific default file.
`--path` overrides that installed file; `--output` writes the generated script
to the requested file. Release packages may also include pre-generated assets.

### Shortcuts

Plugin actions can be exposed as named `lla` shortcuts. Use the interactive
shortcut command to select a plugin/action and assign a name:

```bash
lla shortcut create
lla shortcut list
lla shortcut add audit security_audit audit --description "Audit selected paths"
lla shortcut remove audit
lla shortcut export shortcuts.toml
lla shortcut import shortcuts.toml --merge
lla audit README.md
```

The shortcut name becomes a top-level `lla` command and forwards remaining
arguments to the selected action. Shortcuts and plugin aliases live in
configuration. Import replaces the current shortcuts unless `--merge` is used.

### Upgrade

```bash
lla upgrade
lla upgrade --help
```

The upgrader reuses the official installation logic, verifies the release
`SHA256SUMS`, and atomically replaces the selected `lla` executable. Plugin
packages are managed separately through `lla install`, `lla update`, and
migration commands.

## Plugin Platform architecture

Plugin Platform v3 separates authoring, transport, execution, and packaging:

```text
plugin source
   ├── plugin.toml ───────── identity, capabilities, schemas, permissions
   ├── lla_plugin_sdk ────── high-level Plugin trait and helpers
   └── export macro
          │
          ├── native: _plugin_create_v3 + embedded manifest
          └── WASM: Component Model exports + embedded manifest
                 │
                 ▼
       protobuf PluginMessage envelope
                 │
                 ▼
       PluginManager in the lla host
          ├── discovery and precedence
          ├── checksum and manifest verification
          ├── API/runtime loading
          ├── permission grants
          ├── batching, caching, limits, and timeouts
          └── host-owned rendering
```

### Why protobuf is used internally

Both runtimes receive the same protobuf `PluginMessage` request envelope. This
keeps decoration, formatting, actions, structured errors, and typed values
identical across native and Component Model plugins. SDK users do not encode or
decode protobuf manually.

### Native ABI

A native package exports `_plugin_create_v3`. It returns a `PluginApiV3`
containing:

- the ABI version and supported host API range;
- a pointer and length for the embedded manifest;
- a request handler accepting protobuf bytes;
- a function that frees plugin-allocated response memory;
- a destructor for the plugin context.

Rust-owned values never cross the library boundary. The plugin allocates a byte
response and the host returns it through the plugin's own `free_response`
function. The SDK serializes access to mutable plugin state and converts panics
into errors.

### WebAssembly Component Model runtime

WASM plugins use Wasmtime, WASI Preview 2, and the Component Model. The
maintained WIT world exports:

```wit
world plugin {
    import host;
    export manifest: func() -> string;
    export handle: func(request: list<u8>) -> result<list<u8>, string>;
}
```

The host import interface exposes permission-gated clipboard writes and URL
opening. Scoped filesystem preopens and exact-domain HTTP are configured from
the manifest. Raw sockets and subprocess execution are not exposed.

The embedded WASM runtime is compiled only when the non-default `wasm-plugins`
feature is enabled. It is available on supported x86_64 and ARM64 builds; i686
builds reject WASM packages as unsupported. Native packages remain available
through the default `dynamic-plugins` feature. Official Linux and macOS release
binaries explicitly enable `wasm-plugins`; the official NetBSD amd64 binary
uses the default feature set and omits Wasmtime.

### Package is the trust and compatibility unit

An installed v3 package is a directory:

```text
my_plugin/
├── plugin.toml
├── libmy_plugin.so       # Linux native example
├── checksums.toml
└── README.md             # optional
```

On macOS the native entrypoint is a `.dylib`; on Windows it is a `.dll`. A WASM
package uses the `.wasm` file named by `plugin.entrypoint`.

The packaged manifest and the manifest embedded during compilation must match
exactly as parsed contracts. Changing `plugin.toml` after building requires a
rebuild and a new checksum inventory.

## Plugin lifecycle

### Source to execution

```text
author source + plugin.toml
        │ cargo build
        ▼
runtime artifact with embedded manifest
        │ package
        ▼
plugin.toml + entrypoint + checksums.toml
        │ lla install
        ▼
checksum/API/manifest/permission verification
        │ enable
        ▼
discovery → decoration/formatting or typed action
```

### Discovery and loading

The host scans search paths in precedence order. It reads `plugin.toml` before
loading code, resolves the package-local entrypoint, checks API compatibility,
and verifies package integrity. Duplicate plugin identities in lower-priority
paths are shadowed and reported by diagnostics.

For ordinary listings, the host discovers only enabled plugins and plugins
named by temporary CLI overrides or search pipelines. `list-plugins` and `use`
perform full discovery. A direct action or info request discovers only the
named plugin.

### Decoration and formatting

For a supported format, each enabled decorator receives entry batches. Its
decorated entries are merged into the host entries. The host then asks each
plugin for an optional human-readable field. Plugin names are processed in
sorted order for deterministic output.

Batch responses must preserve entry count and path order. The host falls back
to single-entry calls when a batch response is malformed or unavailable.

### Actions

Actions are independent of listing decoration. The host validates raw CLI
arguments against `plugin.toml`, converts them to typed values, calls the
registered handler, validates the returned output shape, and renders it in
human, JSON, NDJSON, or CSV form.

## Install and use plugins

### Install the official prebuilt bundle

```bash
lla install --prebuilt
```

`--prebuilt` is the default install mode. The installer resolves the latest
GitHub release for the host OS and architecture, downloads the plugin archive,
verifies its release checksum when the release publishes `SHA256SUMS`, verifies
each package, prompts for WASM grants where necessary, and lets the user choose
packages. Official releases are expected to publish `SHA256SUMS`; the installer
warns explicitly if a third-party/older release has no archive checksum.

### Install from local source

```bash
lla install --dir /absolute/path/to/my_plugin
```

The installer searches the supplied tree for plugin crates, builds selected
plugins, creates package directories and checksum inventories, verifies the
runtime contract, and records source metadata for future updates.

### Install from Git

```bash
lla install --git https://github.com/example/my-lla-plugins
```

The installer clones the repository, discovers plugin crates, builds selected
plugins, and records the source URL.

Installation does not enable newly installed plugins automatically. Run
`lla use` to persist a selection, or use `--enable-plugin <name>` for one run.

### Inspect and manage installed plugins

```bash
lla list-plugins
lla use
lla plugin info my_plugin
lla plugin permissions my_plugin
lla plugin doctor
lla update my_plugin
lla update
lla clean
```

- `list-plugins` displays discovered plugins.
- `use` is the interactive enable/disable manager.
- `plugin info` prints identity, runtime, API range, formats, fields, and
  permissions.
- `plugin permissions` prints only the permission declaration.
- `plugin doctor` verifies packages and runtime contracts.
- `update [name]` rebuilds source-installed plugins using recorded metadata.
- `clean` quarantines invalid plugin packages rather than blindly loading them.

`lla` 0.6 has no dedicated uninstall command. Disable a valid plugin with
`lla use` before removing its package directory from `plugins_dir`. `lla clean`
is a repair command for invalid packages, not an uninstaller. Prebuilt plugins
are not rebuilt by `lla update`; rerun `lla install --prebuilt` to refresh the
official bundle.

Enable or disable a plugin for one listing without changing configuration:

```bash
lla --enable-plugin my_plugin
lla --disable-plugin my_plugin
```

### Use listing fields

Enabled plugins can append their formatted summary in default and long views.
Long and table formatters can also select a declared field explicitly:

```toml
[formatters.long]
columns = ["permissions", "size", "modified", "name", "field:score"]

[formatters.table]
columns = ["name", "field:category", "field:security_score"]
```

### Run actions

```bash
lla plugin run my_plugin inspect -- README.md --limit 10
lla plugin run my_plugin inspect --output json -- README.md --limit 10
lla plugin run my_plugin inspect --output ndjson -- README.md
lla plugin run my_plugin list --output csv -- README.md  # table action only
```

The `--` separates host options from plugin action arguments. The older form
`lla plugin my_plugin inspect ...` remains deprecated during the 0.6.x series;
new documentation and manifests must use `lla plugin run`.

Interactive actions require a TTY and human output. The host rejects an
interactive action combined with `json`, `ndjson`, or `csv`.

## Create a complete native plugin

This example creates `file_score`, a plugin that:

- decorates regular files with an integer `score` and string `size_class`;
- prints compact and long human fields;
- exposes a typed `inspect` action;
- returns a typed value suitable for human and machine output.

### 1. Create the crate

```bash
cargo new --lib file-score
cd file-score
```

Use this `Cargo.toml` for an external plugin:

```toml
[package]
name = "file-score"
version = "1.0.0"
edition = "2021"
license = "MIT"

[lib]
name = "file_score"
crate-type = ["cdylib"]

[dependencies]
lla_plugin_sdk = "0.6"
```

For a plugin inside this repository, add `plugins/file_score` to the workspace
implicitly through `plugins/*` and use:

```toml
[dependencies]
lla_plugin_sdk.workspace = true
```

### 2. Add `plugin.toml`

Place `plugin.toml` beside `Cargo.toml`:

```toml
schema_version = 3

[plugin]
id = "dev.example.file-score"
name = "file_score"
version = "1.0.0"
api_min = 3
api_max = 3
runtime = "native"
entrypoint = "file_score"
description = "Score files from their filesystem metadata"
license = "MIT"
repository = "https://github.com/example/file-score"

[capabilities]
decorates_entries = true
formats = ["default", "long"]
machine_output = true

[permissions]
filesystem = ["metadata:selection", "read:user-path"]
network = []
process = false
clipboard = false
open_url = false

[[fields]]
name = "score"
type = "integer"
sortable = true
filterable = true
description = "A score derived from file size"

[[fields]]
name = "size_class"
type = "string"
sortable = true
filterable = true
description = "small, medium, or large"

[[actions]]
id = "inspect"
description = "Calculate a score for one path"
examples = [
  "lla plugin run file_score inspect -- README.md",
  "lla plugin run file_score inspect -- README.md --multiplier 2",
]
interactive = false
arguments = [
  { name = "path", type = "path", description = "File to inspect", position = 0, required = true },
  { name = "multiplier", type = "integer", description = "Score multiplier", option = "--multiplier", default = 1, min = 1, max = 10 },
]
output = { type = "value", schema = { path = "path", score = "integer", size_class = "string" } }
```

The native entrypoint is a logical library name. Packaging resolves it to the
platform filename, such as `libfile_score.so`, `libfile_score.dylib`, or
`file_score.dll`.

### 3. Implement the plugin

Replace `src/lib.rs` with:

```rust
use lla_plugin_sdk::{
    interface::proto,
    response,
    value,
    ActionArguments,
    ActionArgumentsExt,
    DecoratedEntryExt,
    Plugin,
};

#[derive(Default)]
struct FileScore;

fn score(size: u64) -> i64 {
    if size == 0 {
        0
    } else {
        size.ilog2().min(100) as i64
    }
}

fn size_class(size: u64) -> &'static str {
    match size {
        0..=1023 => "small",
        1024..=1_048_575 => "medium",
        _ => "large",
    }
}

impl Plugin for FileScore {
    fn decorate_entry(
        &mut self,
        mut entry: proto::DecoratedEntry,
    ) -> proto::DecoratedEntry {
        let Some(metadata) = entry.metadata.as_ref() else {
            return entry;
        };
        if !metadata.is_file {
            return entry;
        }

        let size = metadata.size;
        entry.insert_field(
            "score",
            value::integer(score(size)),
            score(size).to_string(),
        );
        entry.insert_field(
            "size_class",
            value::string(size_class(size)),
            size_class(size),
        );
        entry
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        entries
            .into_iter()
            .map(|entry| self.decorate_entry(entry))
            .collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        let score = entry.custom_fields.get("score")?;
        let class = entry.custom_fields.get("size_class")?;
        match format.as_str() {
            "default" => Some(format!("[score:{score}]")),
            "long" => Some(format!("score={score} class={class}")),
            _ => None,
        }
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        lla_plugin_sdk::manifest_action_infos(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/plugin.toml"
        )))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        if action != "inspect" {
            return response::error(format!("unknown action '{action}'"));
        }

        let path = match arguments.path("path") {
            Ok(Some(path)) => path,
            Ok(None) => return response::error("path is required"),
            Err(error) => return response::error(error),
        };
        let multiplier = match arguments.integer("multiplier") {
            Ok(Some(value)) => value,
            Ok(None) => 1,
            Err(error) => return response::error(error),
        };
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => return response::error(format!("{}: {error}", path.display())),
        };
        let final_score = score(metadata.len()) * multiplier;

        response::value(value::object([
            ("path".to_string(), value::path(path.to_string_lossy())),
            ("score".to_string(), value::integer(final_score)),
            (
                "size_class".to_string(),
                value::string(size_class(metadata.len())),
            ),
        ]))
    }
}

lla_plugin_sdk::export_plugin!(FileScore);
```

Important details:

- `DecoratedEntryExt::insert_field` writes both a display string and a typed
  value. This is preferred over manually keeping the two maps synchronized.
- `decorate_batch` must preserve entry count and order. Override it to share
  expensive I/O; otherwise the trait default calls `decorate_entry`.
- `registered_actions` must return exactly the action IDs declared in the
  manifest. Building the inventory from the embedded manifest prevents drift.
- the export macro requires `Default + Send + 'static`, validates
  `plugin.toml` at compile time, and embeds the exact manifest.

### 4. Build and install

```bash
cargo fmt --check
cargo test
cargo build --release
lla install --dir "$PWD"
lla plugin doctor
lla plugin info file_score
```

The source installer creates the final package and checksum inventory. Do not
copy only the `.so`, `.dylib`, or `.dll` into the plugin directory; v3 expects a
complete package.

### 5. Enable and use

Enable through the interactive manager:

```bash
lla use
```

Or enable for one invocation:

```bash
lla --enable-plugin file_score
lla -l --enable-plugin file_score
lla --json --pretty --enable-plugin file_score
```

Run the typed action:

```bash
lla plugin run file_score inspect -- README.md
lla plugin run file_score inspect --output json -- README.md --multiplier 2
```

The first `--` ends host option parsing. Tokens after it are plugin-action
arguments; positional paths and declared options may be interleaved there.

Add fields to long/table columns after persistently enabling the plugin:

```toml
[formatters.long]
columns = ["permissions", "size", "modified", "name", "field:score", "field:size_class"]
```

## Convert the example to WebAssembly

Use a Component Model plugin when portability and enforced permissions are more
important than direct native integration.

### 1. Change the SDK dependency

```toml
[lib]
name = "file_score"
crate-type = ["cdylib"]

[dependencies]
lla_plugin_sdk = { version = "0.6", features = ["component"] }
```

Inside this workspace, keep the feature and use the local path/workspace form
appropriate to the crate.

### 2. Change the manifest runtime and entrypoint

```toml
[plugin]
runtime = "wasm-component"
entrypoint = "file_score.wasm"
```

All other identity, capability, field, action, and permission declarations can
remain the same, provided the implementation can operate with the WASM host
capabilities. A WASM manifest cannot request `process = true`.

The example needs both filesystem scopes: `metadata:selection` exposes listing
entries to decoration, while `read:user-path` preopens the explicit path passed
to the `inspect` action. API v3 has no separate `metadata:user-path` scope, so
the user-path read scope is required even though this action only calls
`std::fs::metadata`.

### 3. Change the export

```rust
lla_plugin_sdk::export_component!(FileScore);
```

### 4. Build the component

```bash
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
lla install --dir "$PWD"
lla plugin doctor
```

The output is normally under
`target/wasm32-wasip2/release/file_score.wasm`. The source installer copies the
declared entrypoint into the package.

### WASM host capabilities

- filesystem access is granted through scoped WASI preopens;
- HTTP is restricted to exact declared domains;
- clipboard and URL opening use explicit host functions;
- raw sockets and subprocesses are unavailable;
- memory and execution are bounded by the host.

Other languages may generate Component Model bindings from
[`sdk/wit/lla-plugin.wit`](../sdk/wit/lla-plugin.wit), but Rust is the maintained
authoring path in the 0.6 release line.

## Manifest reference

`plugin.toml` is a compiled and packaged contract, not descriptive metadata.
The complete schema is implemented in `interface/src/manifest.rs`.

### Identity

```toml
schema_version = 3

[plugin]
id = "dev.example.my-plugin"
name = "my_plugin"
version = "1.0.0"
api_min = 3
api_max = 3
runtime = "native"
entrypoint = "my_plugin"
description = "Example plugin"
license = "MIT"
repository = "https://example.com/my-plugin"
```

| Key | Meaning |
| --- | --- |
| `schema_version` | Manifest schema; API v3 packages require `3` |
| `id` | Stable package identity used for grants; do not change it casually |
| `name` | CLI name and configuration key |
| `version` | Nonempty package version; bundled plugins match the product version |
| `api_min`, `api_max` | Inclusive host API compatibility range |
| `runtime` | `native` or `wasm-component` |
| `entrypoint` | One package-local filename/logical native name; no absolute paths or traversal |
| `description` | User-facing purpose |
| `license`, `repository` | Optional provenance fields |

IDs, names, action IDs, argument names, field names, and table column names use
ASCII letters, digits, `.`, `_`, and `-`. Values cannot contain surrounding
whitespace.

`runtime` defaults to `native`; plugin `description` defaults to empty;
capabilities, permissions, fields, and actions default empty; and action output
defaults to `none`. License and repository are optional. Plugin version is only
required to be nonempty—it is not parsed as SemVer. Action descriptions and all
argument descriptions are required and nonblank; field/table-column
descriptions may be empty. Treat these permissive parser defaults as
compatibility behavior, not as authoring recommendations.

### Capabilities

```toml
[capabilities]
decorates_entries = true
formats = ["default", "long"]
machine_output = true
```

- `decorates_entries`: the plugin adds per-entry metadata;
- `formats`: human formats the implementation supports;
- `machine_output`: declares that the plugin intends to support structured
  output. In API v3 it is metadata only; listing JSON/NDJSON field inclusion and
  action rendering are governed by the actual returned values and output schema.

For the current host, declare `default`, `long`, or both. Table rendering uses
the `long` plugin format. Unknown format names pass manifest validation but are
not selected by the current host.

### Fields

```toml
[[fields]]
name = "score"
type = "integer"
sortable = true
filterable = true
description = "A plugin-defined score"
```

Supported field types are:

- `string`
- `integer`
- `float`
- `boolean`
- `bytes` (unsigned byte count)
- `timestamp` (unsigned Unix timestamp)
- `path`

Every emitted typed value must match its declaration. If a plugin emits only a
string display value, the host attempts conversion using the declared type;
explicit typed values through `insert_field` are safer.

`sortable` and `filterable` are declarations only in API v3: current listing
sort and filter logic does not consume plugin fields. Long/table column
selection reads a field's string display value from `custom_fields`; a missing
value renders `-`. Field keys are global bare names, not plugin-namespaced, and
collision resolution is not a supported contract, so choose distinctive names.

### Actions

```toml
[[actions]]
id = "inspect"
description = "Inspect one or more paths"
examples = ["lla plugin run my_plugin inspect -- README.md --depth 2"]
interactive = false
arguments = [
  { name = "paths", type = "path", description = "Paths to inspect", position = 0, required = true, repeatable = true },
  { name = "depth", type = "integer", description = "Traversal depth", option = "--depth", default = 1, min = 0, max = 8 },
  { name = "mode", type = "string", description = "Inspection mode", option = "--mode", choices = ["fast", "full"] },
  { name = "hidden", type = "boolean", description = "Include hidden files", option = "--hidden", default = false },
]
output = { type = "value" }
```

Action argument types are `string`, `integer`, `float`, `boolean`, and `path`.
An argument must have a positional `position`, an `option`, or both. Within one
action, names, positions, and options must be unique.

Argument constraints:

- `required = true` cannot be combined with `default`;
- `repeatable = true` produces a typed list;
- `choices` and `default` must match the argument type;
- `min` and `max` apply only to integer/float arguments;
- options must begin with `--`;
- every argument needs a description.

Every manifest example for a bundled plugin must begin with the canonical
`lla plugin run <plugin> <action>` command.

### Output schemas

```toml
output = { type = "none" }
output = { type = "text" }
output = { type = "value", schema = { result = "string" } }
output = { type = "table", columns = [
  { name = "path", type = "path", description = "Matched path" },
  { name = "score", type = "integer", description = "Score" },
] }
```

The host verifies the response variant. For `value`, the `schema` map is parsed
as metadata but its keys and value types are not currently enforced. Table
output must declare at least one uniquely named column, and every row must match
the column count and declared types.

### Permissions

```toml
[permissions]
filesystem = ["read:selection"]
network = ["api.example.com"]
process = false
clipboard = false
open_url = false
```

See [Permissions and security](#permissions-and-security) for semantics.

## Rust SDK reference

### `Plugin` trait

```rust
pub trait Plugin: Default + Send + 'static {
    fn decorate_entry(&mut self, entry: DecoratedEntry) -> DecoratedEntry;
    fn decorate_batch(&mut self, entries: Vec<DecoratedEntry>, format: &str)
        -> Vec<DecoratedEntry>;
    fn format_field(&mut self, entry: DecoratedEntry, format: String)
        -> Option<String>;
    fn format_batch(&mut self, entries: Vec<DecoratedEntry>, format: &str)
        -> Vec<Option<String>>;
    fn run_action(&mut self, action: String, arguments: ActionArguments)
        -> ActionResponse;
    fn registered_actions(&mut self) -> Vec<ActionInfo>;
}
```

Every method has a safe default. Implement only the capabilities declared in
the manifest, but bundled decorators are verified to implement both single and
batch paths.

### Entry values

`DecoratedEntry` contains:

- `path`: the selected path;
- `metadata`: optional host filesystem metadata;
- `custom_fields`: display strings used by human formatters and compatibility
  paths;
- `typed_fields`: typed values used for structured output and validated field
  contracts.

Use `DecoratedEntryExt`:

```rust
entry.insert_field("score", value::integer(42), "42");
entry.promote_string_field("category");
entry.promote_integer_field("score");
entry.promote_boolean_field("safe");
entry.promote_path_field("original_path");
```

`insert_field` is preferred for new plugins. Promotion helpers are useful when
porting code that already builds string fields.

### Typed values

The `value` module constructs:

```rust
value::null();
value::string("text");
value::integer(42);
value::float(3.14);
value::boolean(true);
value::bytes(4096);
value::timestamp(1_700_000_000);
value::path("src/main.rs");
value::list([value::string("one"), value::string("two")]);
value::object([("score".to_string(), value::integer(42))]);
```

### Action arguments

`ActionArguments` is a map of host-validated typed values. Read it with
`ActionArgumentsExt`:

```rust
arguments.string("mode")?;
arguments.strings("labels")?;
arguments.integer("limit")?;
arguments.float("threshold")?;
arguments.boolean("hidden")?;
arguments.path("path")?;
arguments.paths("paths")?;
```

Scalar accessors return `Result<Option<T>, ActionError>`. Repeated accessors
return a vector. Missing optional arguments produce `None` or an empty vector;
wrong runtime types produce a structured invalid-argument error.

### Action responses

The `response` module constructs output matching manifest schemas:

```rust
response::none();
response::text("done");
response::value(value::integer(42));
response::table(
    ["path", "score"],
    [vec![value::path("README.md"), value::integer(42)]],
);
response::error("operation failed");
response::from_result(result);
```

For actionable failures, construct `ActionError` with a stable code and typed
details:

```rust
let error = lla_plugin_sdk::ActionError::new("not-found", "path does not exist")
    .with_detail("path", value::path("missing.txt"));
return response::error(error);
```

### Export macros

- `export_plugin!(Type)` generates `_plugin_create_v3`, embeds the manifest,
  serializes mutable access, bounds responses, contains panics, and provides
  plugin-owned cleanup.
- `export_component!(Type)` validates `runtime = "wasm-component"`, generates
  WIT bindings, embeds the manifest, and exports the component world.

Both macros locate `plugin.toml` through `CARGO_MANIFEST_DIR`; a missing or
invalid manifest is a compile error.

## Fields, formatting, and machine output

### Decoration versus formatting

Decoration computes reusable data. Formatting turns that data into one optional
human string:

```text
decorate_entry/batch → custom_fields + typed_fields
format_field/batch   → optional display suffix
```

Do expensive work during batch decoration, not once again in `format_field`.
Formatting should be deterministic, quick, and side-effect free.

### Supported plugin format behavior

The current host contract normalizes:

| Host view | Plugin format sent |
| --- | --- |
| default | `default` |
| long | `long` |
| table | `long` |
| grid/tree/recursive/Git/timeline/sizemap/fuzzy | none currently |

Therefore, do not declare view names beyond `default` and `long` expecting the
current host to call them. Extending cross-view plugin formatting requires a
host change to `normalize_plugin_format`, formatter integration, tests, and an
updated manifest contract.

### Long and table columns

Built-in column keys include `permissions`, `inode`, `links`, `size`,
`allocated`, `modified`, `created`, `accessed`, `user`, `group`, `xattrs`,
`context`, `mount`, `name`, `path`, and `plugins`.

Use `field:<name>` for a declared plugin field:

```toml
[formatters.table]
columns = ["name", "size", "field:score"]
```

The `plugins` column contains the combined human plugin strings. In long view,
omitting it appends plugin text to the name. In table view, omitting it emits
the combined text as a trailing row suffix rather than changing the name cell.

### JSON and NDJSON

Machine entries contain stable built-in keys and an unnamespaced `plugin`
object assembled from entry custom fields, including host-internal fields when
present. Typed values remain JSON numbers, booleans, arrays, or objects and
override a same-key display string rather than being flattened into strings.

```json
{
  "path": "README.md",
  "name": "README.md",
  "size_bytes": 1234,
  "plugin": {
    "score": 10,
    "size_class": "medium"
  }
}
```

CSV listing output has a fixed v1 set of built-in columns and does not currently
add arbitrary plugin fields. CSV **action** output requires a declared `table`
result; `none`, `text`, and `value` action results cannot be rendered as CSV.

## Typed actions and output

### Host argument processing

The host, not the plugin, parses raw arguments. It:

1. separates positional tokens and `--options`;
2. applies boolean flags, defaults, and repeated values;
3. converts values to the declared types;
4. validates required arguments, choices, and numeric ranges;
5. sends a typed map to `run_action`.

Plugins should not reparse `std::env::args`.

### Human output

- `none`: no result body;
- `text`: printed as supplied;
- `value`: rendered as a scalar or structured value;
- `table`: rendered with declared columns.

### Machine output

- JSON emits the structured result;
- NDJSON emits rows/items as newline-delimited JSON where applicable;
- CSV accepts only a declared table result and emits its columns and rows;
- structured errors include a stable error code, message, and typed details.

### Search pipelines

Listing content search can feed matched paths to plugin actions:

```bash
lla --search 'TODO' --search-pipe my_plugin:inspect
lla --search 'secret' --search-pipe security_audit:audit
```

The syntax is `plugin:action[:arg...]`. The plugin is discovered even when it
was named only by the pipeline. Design pipeline actions to accept repeated path
inputs when processing multiple matches.

## Permissions and security

### Filesystem scopes

Valid scopes are:

| Scope | Intended access |
| --- | --- |
| `metadata:selection` | metadata for selected entries |
| `metadata:tree` | metadata beneath the selected tree |
| `read:selection` | contents of selected entries |
| `read:tree` | contents beneath the selected tree |
| `read:user-path` | a path explicitly supplied by the user |
| `write:selected-destination` | a user-selected destination |
| `write:tree` | write beneath the selected tree |
| `write:user-path` | write to an explicit user path |
| `delete:selection` | delete selected entries |
| `delete:quarantine` | delete from plugin-managed quarantine |

Request the narrowest scope that supports the behavior. `delete:quarantine` is
accepted by manifest validation, but API v3 does not map it to a WASM preopen;
do not depend on it for Component Model filesystem access.

### Network and host capabilities

Network entries are host names without schemes, paths, wildcards, or ports and
are compared exactly and case-sensitively at runtime. Prefer lowercase DNS host
names such as `api.example.com`. Validation rejects leading/trailing or repeated
dots and non-ASCII-domain characters, but is not a complete DNS validator; a
manifest passing validation does not prove the host name is resolvable.

Boolean permissions declare:

- `process`: subprocess execution; forbidden for WASM;
- `clipboard`: clipboard access;
- `open_url`: opening a URL through the operating system/host.

### Native trust model

Native plugins execute in the `lla` process. Their permissions are declarations
shown to the user and checked for manifest consistency, but they are not an OS
sandbox. Install native code only from trusted sources. A native panic is
contained at the SDK call boundary when possible, but unsafe native behavior can
still compromise the process.

### WASM trust model and grants

WASM starts default-denied for host paths and capabilities. Every component also
receives a private writable `/data` directory scoped to that plugin, even when
it declares no permissions. Approved grants are stored by stable plugin ID in:

```text
<plugins_dir>/plugin-grants.toml
```

The store records the plugin version and approved permission set. An update
that requests any additional scope/domain/capability prompts again. Removing
permissions does not require broader approval. Packages with no permissions are
approved automatically.

Grants are written through a temporary file and atomically renamed. Do not use
the plugin name as a stable security identity; use an unchanging `plugin.id`.

### Runtime limits

The shared contract enforces:

- at most 512 entries per plugin batch;
- at most 16 MiB per response;
- 128 MiB of memory per WASM instance;
- a 5-second WASM timeout for decoration/formatting calls;
- a 60-second WASM timeout for actions.

WASM traps, timeouts, permission failures, and limit violations are reported as
host plugin errors rather than crashing the host. A structured error response
is distinct: it is emitted deliberately by plugin action code.

Native plugins share the batch and response-size checks, but execute
synchronously as trusted in-process code. The host cannot enforce an equivalent
memory sandbox or safely interrupt a blocked native call. Native authors should
treat the 5/60-second values as design budgets even though hard timeout
interruption is a WASM guarantee.

## Discovery, configuration, and aliases

### Search path precedence

The host searches:

1. `--plugins-dir` when supplied, otherwise configured `plugins_dir`;
2. configured `plugin_dirs` in order;
3. system locations:
   - Linux: `/usr/local/lib/lla/plugins`, `/usr/lib/lla/plugins`;
   - macOS: `/opt/homebrew/lib/lla/plugins`, `/usr/local/lib/lla/plugins`;
   - Windows: `%PROGRAMDATA%/lla/plugins`.

Earlier paths win. Duplicate path entries are removed. Use `plugin_dirs` for
read-only or package-manager-owned locations; the primary `plugins_dir` remains
the writable installation and grant location.

### Configuration example

```toml
enabled_plugins = ["file_score", "git_status"]
plugins_dir = "~/.config/lla/plugins"
plugin_dirs = ["/opt/company/lla/plugins"]

[plugin_aliases]
score = "file_score"
```

Aliases resolve user-facing names for direct actions and discovery. Stable
manifest identity and enabled configuration continue to use the real plugin
name.

### Project profiles

The nearest ancestor `.lla.toml` overlays global configuration. This allows a
project to select plugins, view columns, filters, and aliases without changing
the user's global defaults. Use `lla config show-effective` to diagnose the
merged result.

## Packaging and integrity

### `checksums.toml`

A release package contains a SHA-256 inventory:

```toml
[files]
"plugin.toml" = "<64 lowercase or uppercase hexadecimal characters>"
"libfile_score.so" = "<64 hexadecimal characters>"
```

The inventory must cover `plugin.toml` and the runtime entrypoint. It may cover
additional regular package files. Paths must be relative, remain within the
package, and refer to regular files. Symlinked entrypoints and traversal paths
are rejected.

Prebuilt installation requires a valid inventory. Source installation creates
the inventory after building and copying the mutable development files into the
immutable package directory.

### Package verification

Installation and `plugin doctor` verify:

1. manifest schema and identifiers;
2. host API compatibility;
3. package checksum coverage and content;
4. safe package-local entrypoint resolution;
5. packaged versus embedded manifest equality;
6. runtime type and platform support;
7. registered action IDs versus manifest declarations;
8. static argument/output declaration consistency;
9. single and batch decoration consistency;
10. declared field type conversion;
11. formatting responses for declared formats;
12. safe rejection of unknown actions;
13. permission/runtime constraints.

Declared actions are not invoked by `plugin doctor`: it compares the registered
action inventory with the manifest and probes rejection of an unknown action.
Typed arguments and runtime output are validated when a user actually runs an
action; table rows are checked fully, while value-schema keys are not enforced.

The functional verifier creates temporary file and directory entries. Plugins
must behave safely on generic inputs and must not assume every path matches a
special project type.

### Official archives

`scripts/build_plugins.sh` produces platform archives named like:

```text
plugins-linux-amd64.tar.gz
plugins-linux-amd64.zip
plugins-macos-arm64.tar.gz
plugins-macos-arm64.zip
```

Each plugin is packaged in its own directory. Release-level `SHA256SUMS` covers
the archives; each package-level `checksums.toml` covers its internal contract
files.

## Testing and verification

### Plugin-local tests

Keep pure scoring/parsing/business logic outside the trait implementation and
unit test it normally:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Add tests for:

- missing metadata and nonmatching paths;
- minimum/maximum sizes and timestamps;
- field types and display strings;
- every action and error path;
- batch output count/order;
- deterministic formatting;
- permission-sensitive behavior;
- platform-specific dependencies.

### SDK fixtures

The repository maintains executable examples:

- [`minimal_native`](../sdk/tests/fixtures/minimal_native/)
- [`custom_batch`](../sdk/tests/fixtures/custom_batch/)
- [`wasm_component`](../sdk/tests/fixtures/wasm_component/)

Verify them with:

```bash
cargo check --manifest-path sdk/tests/fixtures/minimal_native/Cargo.toml
cargo check --manifest-path sdk/tests/fixtures/custom_batch/Cargo.toml
rustup target add wasm32-wasip2
cargo build --manifest-path sdk/tests/fixtures/wasm_component/Cargo.toml \
  --target wasm32-wasip2
```

### Workspace verification

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo test -p lla --features wasm-plugins
cargo clippy -p lla --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy -p lla --no-default-features --all-targets -- -D warnings
```

CI additionally checks the default build natively on NetBSD and asserts that
its dependency graph does not include Wasmtime. The release workflow also
builds and smoke-tests that native configuration as `lla-netbsd-amd64`.

Build and verify every bundled package:

```bash
./scripts/build_plugins.sh --target "$(rustc -vV | sed -n 's/^host: //p')"
./scripts/verify_plugins_v3.sh dist/plugins-macos-arm64
```

Replace the example directory with the exact directory printed by the build
script, such as `dist/plugins-linux-amd64`.

### Manual end-to-end checklist

```bash
lla install --dir /path/to/plugin
lla plugin doctor
lla plugin info my_plugin
lla plugin permissions my_plugin
lla --enable-plugin my_plugin
lla -l --enable-plugin my_plugin
lla --json --pretty --enable-plugin my_plugin
lla plugin run my_plugin action -- required-argument
lla plugin run my_plugin action --output json -- required-argument
```

Also test installation into a clean temporary `--plugins-dir` to avoid passing
because of stale local artifacts.

## Performance and reliability

### Batch work

The host batches up to 512 entries. Override `decorate_batch` and `format_batch`
when one repository scan, directory walk, database lookup, or subprocess can
serve many entries. Preserve input order and return exactly one result per
input.

### Cache intentionally

The host caches decoration and formatting by path, selected format, and enabled
plugin set for the process. Plugins may add their own persistent cache when the
underlying operation is expensive, but cache keys must include every input that
affects the result and invalidation must account for file changes.

Do not rely on the host cache as durable storage. Plugin state may be recreated
on the next invocation or after an update.

### Avoid formatter side effects

`format_field` may be called after batch preparation or as a fallback. It
should render already-computed fields rather than reading files, invoking Git,
or making network calls.

### Degrade gracefully

Entry decoration failures are isolated where possible so one optional plugin
does not destroy a listing. Actions should return structured errors with stable
codes. Optional external tools should be detected and reported clearly; do not
panic when a command is missing.

### Output discipline

Return data through SDK responses. Do not print JSON/CSV directly from a plugin:
the host owns output selection, escaping, column validation, and terminal
rendering. Interactive actions are the exception because they explicitly own a
TTY interaction and cannot be combined with machine output.

## Debugging and troubleshooting

### Plugin is installed but does not appear

1. Run `lla plugin doctor`.
2. Run `lla list-plugins` and confirm the name.
3. Inspect `lla config show-effective` for `enabled_plugins`, `plugins_dir`, and
   project profile overrides.
4. Try `lla --enable-plugin <name>`.
5. Confirm the manifest declares the selected format. Table requires `long`.

### `Cannot checksum missing plugin package file`

This indicates an incomplete build/package output or stale mutable installation
state. Rebuild and reinstall from the crate root:

```bash
lla install --dir /path/to/plugin
lla plugin doctor
```

The source installer performs its own release build; a separate `cargo build`
is useful for diagnosis but is not required before installation.

Do not fabricate a checksum entry for a missing binary.

### Checksum mismatch

The package changed after checksums were generated. Reinstall from source or
download the official archive again. Never edit an installed `plugin.toml` or
entrypoint in place.

### Packaged and embedded manifests differ

Rebuild after every manifest change, then recreate the package. The macro embeds
the manifest at compile time.

### Unknown or missing action

Ensure all three surfaces agree:

1. `[[actions]].id` in `plugin.toml`;
2. `registered_actions()` output;
3. the `run_action` match arm.

Using `manifest_action_infos` for registration eliminates the first two drifting
apart.

### Field missing from a column

- declare it under `[[fields]]`;
- emit it during decoration;
- ensure its typed value matches the declared type;
- enable the plugin;
- use `field:<name>` in long/table configuration;
- confirm the plugin declares `long` for long/table output.

### WASM package is unsupported

WASM components require an x86_64 or ARM64 build compiled with
`--features wasm-plugins`. A build without that feature reports how to enable
it; an unsupported architecture reports the platform separately. NetBSD
default builds intentionally omit Wasmtime because its runtime does not support
NetBSD. Use a native package there or install an official full-featured CLI on a
supported Linux or macOS target.

### WASM permission denied

Run `lla plugin permissions <name>`, reinstall/update in a TTY to review grants,
and confirm the manifest uses the narrow scope/domain actually needed. A new
permission request after an update requires approval.

### Plugin timeout or response limit

Move repeated work into batches, reduce output, paginate action results, or
return summary data with a follow-up action for detail. Decoration must finish
within the 5-second WASM budget, actions within the 60-second WASM budget, and
responses must remain under 16 MiB. A blocked native call cannot be interrupted
safely by the host, so native plugins must enforce their own I/O timeouts.

### Old v1/v2 library is disabled

`lla` 0.6 supports API v3 only. The host detects legacy symbols without calling
their constructors. Use:

```bash
lla plugin migrate --prebuilt
lla plugin doctor
```

Third-party source must be rebuilt against `lla_plugin_sdk` 0.6; there is no
binary compatibility bridge.

### Diagnose search-path shadowing

Compare the configured primary and extra paths, then run `plugin doctor`.
Earlier paths take precedence. Remove or rename stale duplicates instead of
assuming the lower-priority package is active.

## Contributing a bundled plugin

### Directory requirements

Create `plugins/<name>/` with at least:

```text
plugins/<name>/
├── Cargo.toml
├── plugin.toml
├── README.md
└── src/lib.rs
```

The directory name, manifest plugin name, and expected package name must agree.
Use workspace versions and dependencies. New native libraries use `cdylib` and
`lla_plugin_sdk.workspace = true`.

### Implementation requirements

- use schema 3 and API range including host API 3;
- export through `export_plugin!` or `export_component!`;
- declare every emitted field, action, argument, output, and permission;
- emit typed fields for declared values;
- implement both single and batch decoration for decorators;
- register and implement every declared action;
- use canonical action examples;
- avoid legacy request adapters;
- document dependencies, actions, examples, and platform limits in README;
- add unit tests for core behavior.

The interface test suite statically audits bundled source for undeclared
clipboard, URL-opening, and process usage. This complements runtime verification
but does not replace code review.

### Regenerate the catalog

The bundled catalog is generated:

```bash
./scripts/generate_plugins.sh
```

Do not hand-edit `docs/plugins/catalog.md`. Review the generated change with the
plugin implementation.

### Validate the bundle

Run the workspace checks, build the target archive, and run the v3 verifier as
shown in [Testing and verification](#testing-and-verification).

## Release architecture

The CLI, interface, SDK macros, SDK, utilities, bundled manifests, and bundled
Cargo packages use one version and one Git tag.

### Preparation

Maintainers add release notes under `## [Unreleased]`, then run the Prepare
Release workflow or:

```bash
RELEASE_VERSION=0.6.1 .github/scripts/prepare_release.sh
```

Preparation updates workspace/internal/plugin versions, `Cargo.lock`, and the
changelog, then opens a conventional release PR in the automated workflow.

### Gates and artifacts

The release workflow validates version/tag/changelog/crates.io state, runs
quality gates, and builds:

- dynamic/native-plugin-enabled Linux and macOS CLI binaries; release builds
  explicitly enable `wasm-plugins`, so supported x86_64 and ARM64 artifacts
  include WASM while Linux i686 does not;
- a native NetBSD amd64 CLI binary built with default features, with native
  plugin loading enabled and Wasmtime omitted;
- static musl amd64/arm64 binaries with the plugin runtime;
- platform plugin `.tar.gz` and `.zip` archives;
- Linux OS packages;
- themes;
- final `SHA256SUMS`.

GNU artifacts enforce the documented glibc baseline. Musl artifacts must be
static and pass Alpine smoke tests, including plugin runtime behavior.

### Crate publication order

```text
lla_plugin_interface
        ↓
lla_plugin_sdk_macros
        ↓
lla_plugin_sdk
        ↓
lla_plugin_utils
        ↓
lla
```

The workflow waits for each dependency to become visible on crates.io before
publishing the next. GitHub release publication happens only after validation,
artifact verification, and crate publication succeed. A failed release can be
resumed with workflow dispatch and the existing tag; already-published valid
state is skipped safely.

See [`docs/maintainers/releasing.md`](maintainers/releasing.md) for the concise
operator checklist.

## Compatibility and migration

API v3 intentionally has no binary bridge from v1/v2. Legacy native libraries
are detected by symbol inspection and never initialized.

To port source:

1. depend on `lla_plugin_sdk = "0.6"`;
2. implement `Plugin` rather than a raw protobuf handler;
3. use `export_plugin!` or `export_component!`;
4. replace old metadata with schema-3 `plugin.toml`;
5. declare every field, action, typed argument, output, and permission;
6. emit typed values;
7. build, install from source, and run `plugin doctor`;
8. test both human and machine action output.

The native ABI and Component Model transport are implementation boundaries.
Compatibility promises are expressed through manifest schema, host API range,
published SDK version, and WIT world rather than Rust struct layout.

## Source-of-truth index

When documentation and implementation appear to disagree, inspect these files
and update both in the same change:

| Subject | Source of truth |
| --- | --- |
| CLI startup/discovery | `lla/src/main.rs` |
| CLI arguments and commands | `lla/src/commands/args.rs` |
| Listing pipeline | `lla/src/commands/file_utils.rs` |
| Configuration and search paths | `lla/src/config/mod.rs` |
| Lister abstraction | `lla/src/lister/mod.rs` |
| Formatter abstraction/views | `lla/src/formatter/` |
| Plugin loading/dispatch/contracts | `lla/src/plugin/mod.rs` |
| Package checksum rules | `lla/src/plugin/package.rs` |
| WASM grants | `lla/src/plugin/grants.rs` |
| WASM runtime | `lla/src/plugin/wasm_runtime.rs` |
| Source/prebuilt installation | `lla/src/installer.rs` |
| Manifest schema | `interface/src/manifest.rs` |
| Protobuf contract | `interface/src/plugin.proto` |
| Native ABI and shared limits | `interface/src/lib.rs` |
| Rust authoring API | `sdk/src/lib.rs` |
| Component Model world | `sdk/wit/lla-plugin.wit` |
| Export macros | `macros/src/lib.rs` |
| Plugin archive builder | `scripts/build_plugins.sh` |
| Plugin verifier | `scripts/verify_plugins_v3.sh` |
| Release preparation workflow | `.github/workflows/prepare-release.yml` |
| Release preparation script | `.github/scripts/prepare_release.sh` |
| Shared release helpers | `.github/scripts/release_common.sh` |
| Release workflow | `.github/workflows/release.yml` |

Focused documentation:

- [Plugin user guide](plugins/README.md)
- [Bundled catalog](plugins/catalog.md)
- [Architecture reference](plugins/architecture.md)
- [Development quick start](plugins/developing.md)
- [Manifest reference](plugins/manifest.md)
- [Migration guide](plugins/migration-v3.md)
- [Release guide](maintainers/releasing.md)
