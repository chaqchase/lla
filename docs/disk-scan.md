# Disk scan

`lla scan` shows what is using space on your disk and helps you get it back.

```bash
lla scan               # your home folder (plus /Applications on macOS)
lla scan ~/code        # just one folder
lla scan --report      # print a summary instead of the interactive view
lla scan --json        # the same data for scripts
lla scan --no-duplicates   # skip the duplicate search
```

In a terminal it opens an interactive view. When the output is piped, or with
`--report` or `--json`, it prints a summary instead.

## What it shows

The top of the screen shows how full the volume is and a dot map of the whole
disk, with one block of color per category. Below that, everything lla found is
grouped into categories:

| Category | What lands there |
| --- | --- |
| Apps | Installed apps and the data they keep (`/Applications`, `~/Library/Application Support`, `%LOCALAPPDATA%`, Flatpak, Snap, `/opt`), and iPhone and iPad backups, one item per device |
| Developer | Build output and dependency folders found anywhere (`node_modules`, Rust `target`, `.next`, `Pods`, `DerivedData`, ...), package caches, toolchains, Xcode and container disk images (see below) |
| Caches | `~/Library/Caches`, `~/.cache`, `%TEMP%`, `/var/cache` |
| Logs | `~/Library/Logs`, `/var/log`, crash dumps |
| Downloads, Documents, Photos, Videos, Music | The usual folders, plus iCloud Drive and OneDrive |
| Trash | What is already in the trash: `~/.Trash`, `~/.local/share/Trash`, and your Recycle Bin on each fixed drive on Windows |
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

### Developer tools

| What | Where | Label |
| --- | --- | --- |
| Xcode DerivedData, iOS / watchOS / tvOS / visionOS DeviceSupport, SwiftUI previews | `~/Library/Developer/Xcode/...` | safe to clean |
| Simulator caches | `~/Library/Developer/CoreSimulator/Caches` | safe to clean |
| Simulators | `~/Library/Developer/CoreSimulator/Devices` | review first; `xcrun simctl delete unavailable` removes old ones |
| Xcode archives | `~/Library/Developer/Xcode/Archives` | review first |
| Docker Desktop disk image | `~/Library/Containers/com.docker.docker`, `%LOCALAPPDATA%\Docker` (the WSL `.vhdx`), `/var/lib/docker` | review first; `docker system prune` frees space inside it |
| OrbStack, Colima, Lima disks | `~/Library/Group Containers/HUAQ24HBR6.dev.orbstack`, `~/.colima`, `~/.lima` | review first |
| Homebrew downloads | `~/Library/Caches/Homebrew`, `~/.cache/Homebrew`, or `$HOMEBREW_CACHE` | safe to clean, like `brew cleanup` |
| Package caches | npm, pnpm, Yarn, Bun, Cargo, Gradle, Maven, Go modules, pip, uv, NuGet, and their `%LOCALAPPDATA%` versions on Windows | safe to clean |

The Homebrew Caskroom and Cellar are installed software, not cache, and are left
alone. lla only reads these folders; it never runs `docker`, `brew` or `xcrun`
for you.

iPhone and iPad backups (`~/Library/Application Support/MobileSync/Backup` on
macOS, `%APPDATA%\Apple Computer\MobileSync\Backup` or
`%USERPROFILE%\Apple\MobileSync\Backup` on Windows) show the device name from
each backup when it can be read. Remove old ones from Finder or iTunes when you
can, so the device list there stays in sync.

## Duplicates

After the scan, lla looks for large files (1 MB and up) stored more than once in
the scanned folders. It groups them by size, then compares the first and last
64 KB, and confirms the rest with a SHA-256 of the whole file. The search runs
in the background, so the view is usable right away; the header shows how far
along it is ("checking duplicates… 1.2 GB of 4.0 GB").

Only your own files are compared. Apps and their data (`~/Library`,
`%APPDATA%`), caches, build folders, device backups, the trash, hidden folders,
macOS packages such as apps and Photos libraries, and git checkouts are
skipped: removing one file from inside them breaks the whole, and most are
offered for cleaning as a whole already. Hard links are one file with several names and are
not reported. On APFS, a file copied in Finder can be a clone that shares its
blocks with the original; lla cannot tell clones apart, so removing one may
free less than shown.

Duplicates appears below the categories. Each row is one file with how many
copies there are and what removing the extras would free; `⏎` shows the copies,
`space` marks one, and `a` marks every copy but the first. lla never lets you
mark every copy of a file: the last one cannot be marked, and if a folder you
marked would take the last copy with it, the confirmation leaves one out and
says which.

Pass `--no-duplicates` to skip the search.

## Keys

| Key | Action |
| --- | --- |
| `↑` `↓` `j` `k` | Move |
| `⏎` `→` `l` | Open a category or folder, or show a duplicate's copies |
| `esc` `←` `h` | Go back |
| `tab` | Next category |
| `space` | Mark or unmark |
| `a` | Mark everything cleanable in the list (in Duplicates, every copy but one) |
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
- **Windows:** some folders need an administrator terminal. lla finds your own
  Recycle Bin folder (`X:\$Recycle.Bin\<your SID>`) on each fixed drive and
  shows its entries under Trash with their original names; removing one also
  removes its `$I` record so Explorer does not show a broken entry. Other
  users' bins are not readable without administrator rights.

## Report and JSON

`--report` prints the categories, the largest items that are safe to clean, the
largest ones untouched for three months, and the duplicates that waste the most
space. `--top N` sets how many items each list shows (default 10).

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
          "loose_files": false,
          "label": null
        }
      ]
    }
  ],
  "duplicates": [
    {
      "bytes": 4200000000,
      "wasted_bytes": 4200000000,
      "paths": ["/Users/me/Movies/trip.mov", "/Users/me/Downloads/trip.mov"]
    }
  ]
}
```

`label` is a readable name when the folder name is just an id, like an iPhone
backup or a Recycle Bin entry. `duplicates` is left out with `--no-duplicates`;
`bytes` is the size of one copy and `wasted_bytes` what removing all but one
would free.

Sizes are bytes actually allocated on disk, so sparse and cloud-only files count
for what they really take. Hard links are counted once, and the scan stays on
the filesystem it started on.
