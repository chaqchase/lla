# Disk scan

`lla scan` shows what is using space on your disk and helps you get it back.

```bash
lla scan               # your home folder (plus /Applications on macOS)
lla scan ~/code        # just one folder
lla scan --report      # print a summary instead of the interactive view
lla scan --json        # the same data for scripts
```

In a terminal it opens an interactive view. When the output is piped, or with
`--report` or `--json`, it prints a summary instead.

## What it shows

The top of the screen shows how full the volume is and a dot map of the whole
disk, with one block of color per category. Below that, everything lla found is
grouped into categories:

| Category | What lands there |
| --- | --- |
| Apps | Installed apps and the data they keep (`/Applications`, `~/Library/Application Support`, `%LOCALAPPDATA%`, Flatpak, Snap, `/opt`) |
| Developer | Build output and dependency folders found anywhere (`node_modules`, Rust `target`, `.next`, `Pods`, `DerivedData`, ...), package caches and toolchains |
| Caches | `~/Library/Caches`, `~/.cache`, `%TEMP%`, `/var/cache` |
| Logs | `~/Library/Logs`, `/var/log`, crash dumps |
| Downloads, Documents, Photos, Videos, Music | The usual folders, plus iCloud Drive and OneDrive |
| Trash | What is already in the trash |
| Other | Everything else in the scanned folders |
| System & other | What the disk reports as used, minus everything lla found: the OS, swap, snapshots, other users, and folders lla could not read |

A build folder is only counted as such when its project file sits next to it,
so `target` needs a `Cargo.toml` and `bin` needs a `.csproj`. Bytes are counted
once: a `node_modules` inside `~/Documents` shows up under Developer and is
taken out of the Documents total.

Each item carries a label:

- **safe to clean**: rebuilt or downloaded again on its own, or already in the
  trash.
- **review first**: probably fine to remove, but take a look first (apps,
  downloads, toolchains, virtualenvs).
- **your files**: documents, photos and the like. lla never suggests these.

Anything not touched for three months is flagged as probably unused.

## Keys

| Key | Action |
| --- | --- |
| `↑` `↓` `j` `k` | Move |
| `⏎` `→` `l` | Open a category or folder |
| `esc` `←` `h` | Go back |
| `tab` | Next category |
| `space` | Mark or unmark |
| `a` | Mark everything cleanable in the list |
| `u` | Clear marks |
| `d` | Move marked items (or the current one) to the trash |
| `D` | Delete marked items for good |
| `g` | Jump to the biggest part of a folder |
| `o` | Reveal in Finder, Explorer or your file manager |
| `f` | Show only cleanable and unused items |
| `s` | Sort by size, age or name |
| `r` | Scan again |
| `?` | Help |
| `q` | Quit |

Nothing is removed without a confirmation that lists every path and the total.
`d` moves things to the trash so they can be restored; items that are already
in the trash are deleted for good, and the confirmation says so. lla refuses to
remove your home folder, the folders it groups by (like `~/Library/Caches`
itself), anything above them, and folders such as `~/.ssh` and `~/.config`.

Moving to the trash does not free space until the trash is emptied. The Trash
category shows what is waiting there.

## Permissions

Folders lla cannot read are counted and shown at the top. Their size ends up in
"System & other".

- **macOS:** give your terminal Full Disk Access in System Settings › Privacy &
  Security. Without it, macOS may ask once for access to folders like Documents
  or other apps' data, and some folders stay unreadable.
- **Linux:** system folders such as `/var/lib/docker` need `sudo lla scan`.
- **Windows:** some folders need an administrator terminal. The Recycle Bin is
  not scanned yet.

## Report and JSON

`--report` prints the categories, the largest items that are safe to clean, and
the largest ones untouched for three months. `--top N` sets how many items each
list shows (default 10).

`--json` prints:

```json
{
  "volume": { "label": "startup disk", "total": 994662584320, "used": 951000000000, "free": 43662584320 },
  "roots": ["/Users/me", "/Applications"],
  "scanned_bytes": 693000000000,
  "files": 5121672,
  "unreadable_folders": 0,
  "elapsed_ms": 36300,
  "safe_bytes": 120000000000,
  "categories": [
    {
      "key": "developer",
      "label": "Developer",
      "bytes": 181000000000,
      "safe_bytes": 102000000000,
      "stale_bytes": 9200000000,
      "item_count": 3359,
      "items": [
        {
          "path": "/Users/me/code/app/node_modules",
          "bytes": 2300000000,
          "files": 48211,
          "modified": 1762300000,
          "safety": "safe",
          "reason": "reinstalled by your package manager",
          "stale": false,
          "protected": false,
          "loose_files": false
        }
      ]
    }
  ]
}
```

Sizes are bytes actually allocated on disk, so sparse and cloud-only files count
for what they really take. Hard links are counted once, and the scan stays on
the filesystem it started on.
