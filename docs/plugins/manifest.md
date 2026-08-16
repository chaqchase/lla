# Plugin manifest reference

Every API v3 package contains `plugin.toml`. The export macro embeds the exact
manifest bytes in the compiled entrypoint, and the host compares that embedded
contract with the packaged file before enabling the plugin.

## Plugin identity

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

- `id` is the stable package identity. Use a reverse-domain identifier.
- `name` is the user-facing CLI name.
- `api_min` and `api_max` declare the supported host API range.
- `runtime` is `native` or `wasm-component`.
- A native `entrypoint` is a logical library name; lla adds the platform prefix
  and suffix. A WebAssembly entrypoint names the package-local `.wasm` file.
- Entrypoints must be one package-local filename. Absolute paths and traversal
  components are rejected.

Identifiers accept ASCII letters, digits, `.`, `_`, and `-`.

## Capabilities

```toml
[capabilities]
decorates_entries = true
formats = ["default", "long"]
machine_output = true
```

`formats` names the listing formats supported by the plugin. Set
`machine_output` when its fields and action results are safe for structured
rendering.

## Listing fields

```toml
[[fields]]
name = "score"
type = "integer"
sortable = true
filterable = true
```

Field types are `string`, `integer`, `float`, `boolean`, `path`, `bytes`, and
`timestamp`. Field names must be unique.

## Actions and arguments

```toml
[[actions]]
id = "inspect"
description = "Inspect one or more paths"
examples = ["lla plugin run my_plugin inspect -- README.md --depth 2"]
interactive = false
arguments = [
  { name = "paths", type = "path", position = 0, required = true, repeatable = true },
  { name = "depth", type = "integer", option = "--depth", default = 1, min = 0, max = 8 },
  { name = "mode", type = "string", option = "--mode", choices = ["fast", "full"] },
  { name = "hidden", type = "boolean", option = "--hidden", default = false },
]
output = { type = "value" }
```

Argument types are `string`, `integer`, `float`, `boolean`, and `path`.
Arguments may declare:

- `required` and `default`
- `repeatable`
- `choices`
- numeric `min` and `max`
- a zero-based positional `position`
- a long option name in `option`

Names, positions, and option names must be unique within an action. Defaults and
choices must match the argument type and numeric constraints.

Set `interactive = true` only when an action reads from the terminal or owns its
human-facing interaction. The host rejects interactive actions without a TTY or
when `json`, `ndjson`, or `csv` output is requested.

Action output types are:

- `none`: the action has no host-rendered result.
- `text`: human-readable text.
- `value`: a typed null, scalar, list, or object.
- `table`: rows with a declared typed column schema.

## Permissions

```toml
[permissions]
filesystem = ["read:selection"]
network = ["api.example.com"]
clipboard = false
open_url = false
process = false
```

Supported filesystem scopes are:

- `metadata:selection`
- `metadata:tree`
- `read:selection`
- `read:tree`
- `read:user-path`
- `write:selected-destination`
- `write:tree`
- `write:user-path`
- `delete:selection`
- `delete:quarantine`

Network entries are exact domain names; wildcards are rejected. WebAssembly
plugins cannot request `process = true`. Native permissions are declarative,
while WebAssembly permissions are enforced by the host.

## Package integrity

Release packages contain `checksums.toml` covering `plugin.toml` and the runtime
entrypoint. Prebuilt installation and `plugin doctor` reject missing coverage,
changed files, manifest mismatches, and incompatible API ranges.
