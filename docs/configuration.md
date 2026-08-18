# Configuration

The global configuration is stored at `~/.config/lla/config.toml`. Command-line
options override configuration values for one invocation.

## Create and inspect the config

```bash
lla init
lla init --default
lla config
lla config show-effective
lla config diff --default
lla config --set default_format long
```

Use `lla config --help` and the generated default file as the canonical list of
settable keys and accepted values.

## Common listing defaults

```toml
default_sort = "name"
default_format = "long"
default_depth = 3
show_icons = true
include_dirs = false
permission_format = "symbolic"

[sort]
dirs_first = true
case_sensitive = false
natural = true

[filter]
case_sensitive = false
no_dotfiles = false
respect_gitignore = true
```

Persistent defaults currently honored by the argument parser are `default`,
`long`, `tree`, `table`, `grid`, `git`, `timeline`, and `sizemap`. Select fuzzy
or recursive view per invocation with `--fuzzy` or `--recursive`.

## Long and table columns

```toml
[formatters.long]
hide_group = true
relative_dates = true
date_format = "%Y-%m-%d %H:%M"
columns = ["permissions", "size", "modified", "user", "name"]

[formatters.table]
columns = ["permissions", "size", "modified", "name"]
```

See [Views and display](views.md#long-view) for built-in keys and plugin-field
columns.

## Recursion and fuzzy limits

```toml
[formatters.tree]
max_lines = 20000

[formatters.grid]
ignore_width = false
max_width = 200

[listers.recursive]
max_entries = 20000

[listers.fuzzy]
ignore_patterns = ["node_modules", "target", ".git", ".idea", ".vscode"]
editor = "code --wait"
```

## Exclude paths

`exclude_paths` removes unwanted paths from top-level and recursive listings.
Tilde expansion is supported. Content search and jump history also honor these
exclusions.

```toml
exclude_paths = [
  "~/Library/Mobile Documents",
  "~/Library/CloudStorage",
]
```

## Project profiles

Place `.lla.toml` inside a project to keep repository-specific settings outside
the global config:

```toml
show_icons = true
default_format = "git"

[sort]
dirs_first = true
```

`lla` walks upward from the current directory, loads the nearest `.lla.toml`,
and overlays it on the global configuration. Inspect the result with
`lla config show-effective` and its sources with `lla config diff --default`.

## Themes

```bash
lla theme
lla theme pull
lla theme install /path/to/theme.toml
lla theme install /path/to/themes/
lla theme preview one_dark
```

The interactive manager selects a theme. `pull` clones the official repository
and installs its TOML theme files, `install` accepts a local file or directory,
and `preview` renders sample output without changing the selection.

## Shortcuts

Shortcuts store plugin action invocations:

```bash
lla shortcut add find file_finder search -d "Quick file search"
lla shortcut remove find
lla shortcut list
lla shortcut create
lla shortcut export
lla shortcut import shortcuts.toml
lla find query
```

Configured shortcut names are invoked directly as the first argument to `lla`;
additional arguments are passed to the plugin action. Export and import use
TOML. Run `lla shortcut --help` for each management command's accepted argument
form.

## Plugin locations and enablement

`plugins_dir` is the writable user plugin location. `plugin_dirs` adds
read-only or package-manager-owned locations; earlier entries take precedence.

```toml
enabled_plugins = ["git_status", "file_meta"]
plugins_dir = "~/.config/lla/plugins"
plugin_dirs = ["/opt/lla/plugins"]
```

See [Installing and managing plugins](plugins/README.md) for runtime support,
package integrity, permissions, and actions.

## Shell completion

Generate a completion script for a supported shell:

```bash
lla completion zsh
```

Use `lla completion --help` for supported shells, output-file, and install
options.
