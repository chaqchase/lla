# Navigation

`lla` provides an interactive directory jumper for saved and recent locations,
and a fuzzy view for finding and acting on files.

## Directory jumping

Set up shell integration once:

```bash
lla jump --setup
```

The setup detects Bash, Zsh, or Fish and adds a shell function to the relevant
configuration file. Override detection with `lla jump --setup --shell fish`.
Restart the terminal or source the changed shell file, then run `j` to choose a
directory and change into it.

`lla` itself prints the selected path because a child process cannot change its
parent shell's working directory; the generated `j` function performs the
actual `cd`.

### Bookmarks and history

```bash
lla jump --add ~/projects/my-app
lla jump --remove ~/projects/my-app
lla jump --list
lla jump --clear-history
```

Bookmarks appear before recent directories in the selector. Directory visits
are deduplicated in history, and paths covered by `exclude_paths` are not
recorded.

## Fuzzy view

```bash
lla --fuzzy
```

<img src="https://github.com/user-attachments/assets/ec946fd2-34d7-40b7-b951-ffd9c4009ad6" className="rounded-2xl" alt="fuzzy" />

Fuzzy view supports interactive search, multi-selection, and file actions.
Matched characters are highlighted, matches in the file name rank above
matches in its directory, and terminals at least 120 columns wide show a
preview of the highlighted file.

The finder draws on the terminal even when stdout is redirected, so
`vim $(lla -F)` and `lla -F | xargs …` work; picks are printed as plain paths.

### Query syntax

| Query | Matches |
| --- | --- |
| `cfg mod` | Paths fuzzy-matching every space-separated term. |
| `'exact` | Paths containing `exact` literally. |
| `^src` / `.rs$` | Paths starting / ending with the text. |
| `!test` | Paths not containing `test`. |

Lowercase terms ignore case; a term containing an uppercase letter is
case-sensitive.

| Key | Action |
| --- | --- |
| `Up` / `Down`, `Ctrl+K` / `Ctrl+J`, `Ctrl+P` / `Ctrl+N` | Move up / down. |
| `Ctrl+U` / `Ctrl+D`, `PageUp` / `PageDown` | Move a page up / down. |
| `Ctrl+G` / `Alt+G` (or `Ctrl+Home` / `Ctrl+End`) | Jump to start / end. |
| `Tab` / `Shift+Tab` | Toggle selection and move down / up. |
| `Ctrl+T` | Toggle the preview pane. |
| `Enter` | Return the highlighted file or selected files. |
| `Ctrl+E` | Open selected files in an editor. |
| `Ctrl+Y` | Copy selected paths to the clipboard. |
| `Ctrl+O` | Open selected files with the system opener. |
| `F2` | Rename the highlighted file; Enter confirms and Esc cancels. |
| `Ctrl+C` / `Esc` | Exit. |

Search input also supports `Ctrl+W` or `Alt+Backspace` to delete the previous
word, `Ctrl+H` for backspace, `Delete`, `Ctrl+A` or Home to move to the start,
and End to move to the end.

Plain entries in `listers.fuzzy.ignore_patterns` (such as `.git` or `target`)
match whole path components, so `.git` does not hide `.github`. Use a `glob:`
or `regex:` prefix for other matching.

### Choose the editor

The editor is selected in this order:

1. `listers.fuzzy.editor` in the lla config.
2. `$EDITOR`.
3. `$VISUAL`.
4. `nano` on macOS/Linux or `notepad` on Windows.

```toml
[listers.fuzzy]
editor = "nvim"
```

Editor arguments are supported, for example `editor = "code --wait"`.

See [Configuration](configuration.md) for `exclude_paths` and other persistent
defaults.
