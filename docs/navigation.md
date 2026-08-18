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

| Key | Action |
| --- | --- |
| `Ctrl+J` / `Ctrl+K` | Move down / up. |
| `Ctrl+N` / `Ctrl+P` | Move down / up. |
| `Ctrl+D` / `Ctrl+U` | Move half a page down / up. |
| `Ctrl+G` / `Ctrl+Shift+G` | Jump to end / start. |
| `Space` | Toggle selection. |
| `Enter` | Return the highlighted file or selected files. |
| `Ctrl+E` | Open selected files in an editor. |
| `Ctrl+Y` | Copy selected paths to the clipboard. |
| `Ctrl+O` | Open selected files with the system opener. |
| `F2` | Rename the highlighted file; Enter confirms and Esc cancels. |
| `Ctrl+C` / `Esc` | Exit. |

Search input also supports `Ctrl+W` to delete the previous word, `Ctrl+H` for
backspace, `Ctrl+A` or Home to move to the start, and End to move to the end.

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
