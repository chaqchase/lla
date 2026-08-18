# Filtering and search

Use filters to select directory entries before formatting them. Use content
search to find matching text inside files. Content search has a deliberately
narrower set of compatible listing filters, documented below.

## Sort entries

```bash
lla --sort name
lla --sort size --sort-reverse
lla --sort date --sort-dirs-first
lla --sort-natural
lla --sort-case-sensitive
```

`--sort` accepts `name`, `size`, or `date`. Natural sorting orders numbered names
such as `2.txt` before `10.txt`.

## Filter names and extensions

```bash
lla --filter "test"
lla --filter ".rs"
lla --filter "regex:^test.*\.rs$"
lla --filter "glob:*.{rs,toml}"
```

Name filters are case-insensitive unless `--case-sensitive` is set. Supported
compositions include:

| Form | Meaning |
| --- | --- |
| `test,spec` | Match `test` or `spec`. |
| `+test,api` | Match both `test` and `api`. |
| `test AND .rs` | Logical AND. |
| `test OR spec` | Logical OR. |
| `NOT test` | Logical NOT. |

## Filter by size or time

```bash
lla --size ">10M"
lla --size "512K..2G"
lla --size "..100K"
lla --modified "<7d"
lla --created "2023-01-01..2023-12-31"
```

Size filters accept human-readable units and open or closed ranges. Modified and
created filters accept relative durations and ISO date ranges.

## Filter by entry type and visibility

| Show only | Hide |
| --- | --- |
| `--dirs-only` | `--no-dirs` |
| `--files-only` | `--no-files` |
| `--symlinks-only` | `--no-symlinks` |
| `--dotfiles-only` | `--no-dotfiles` |

`--show-symlinks` includes symlinks whose targets match `--dirs-only` or
`--files-only`. `--all` shows dotfiles; `--almost-all` shows dotfiles except `.`
and `..`.

```bash
lla --dirs-only --dotfiles-only
lla --files-only --no-dotfiles
lla --files-only --show-symlinks
```

## Respect `.gitignore`

```bash
lla --respect-gitignore
lla --no-gitignore
```

The first option applies repository `.gitignore` and Git exclude rules. The
second overrides an enabled configuration default for one run.

```toml
[filter]
respect_gitignore = true
```

## Presets and refinements

Define a reusable preset in `~/.config/lla/config.toml`:

```toml
[filter.presets.rust_sources]
description = "Rust source files changed in the last week"
filter = ".rs"
modified = "<7d"
```

Apply it with `lla --preset rust_sources`. Repeat `--refine <filter>` to apply
additional name or path filters sequentially after the normal listing and
plugin-decoration pipeline. Every refinement must match. Despite the historical
CLI wording about a cache, the current implementation still walks the
filesystem normally.

## Search file contents

```bash
lla --search "TODO"
lla --search "TODO" --search-context 5
lla --search "regex:^func.*\("
lla --search "Error" --filter ".rs" --case-sensitive
lla --search "FIXME" --json
```

Content search uses literal matching by default. Prefix the pattern with
`regex:` to enable regular expressions. Human output includes file paths, line
numbers, themed syntax highlighting, context, and match indicators.

Search honors case sensitivity, configured hidden-file behavior,
`--no-dotfiles`, and `--almost-all`. `--dotfiles-only` is not applied.
`--filter` is applied only when it is a simple extension such as `.rs` or a
`glob:` pattern. A single preset can supply one of those compatible name
filters, but preset size, date, and refinement criteria are ignored. Multiple
preset name filters are combined into an expression that search does not map to
ripgrep. `--dirs-only` and `--no-files` return no search results because ripgrep
searches file contents. Size/date flags, refinements, complex name expressions,
and the other entry-type filters are not applied to content search.

Search currently always applies Git ignore rules, including when
`--no-gitignore` is supplied. Configured `exclude_paths` reliably match when the
search root is absolute; with a relative root such as `.`, absolute configured
exclusions may not match the walker paths. Use an absolute search root when
those exclusions matter:

```bash
lla "$(pwd)" --search "TODO"
```

Search supports JSON, NDJSON, and CSV, but their search-specific records differ
from listing records. See [Search results](machine-output.md#search-results).

## Send search matches to a plugin

```bash
lla --search "TODO" --search-pipe file_tagger:list-tags
lla --search "TODO" --search-pipe file_organizer:organize:type
```

The syntax is `plugin:action[:arg...]`. Matching file paths are appended to the
plugin action arguments. See [Plugins](plugins/README.md) for installation,
permissions, and typed actions.
