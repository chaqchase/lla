# lla Folder Cleaner Plugin

`folder_cleaner` is a safety-first plugin for turning chaotic folders into clean,
unified directory structures. It scans a target folder, builds a reviewable plan,
lets you approve the exact actions, and quarantines cleanup candidates instead
of deleting them.

## Features

- Recursive scan with safe defaults and ignored system/project folders.
- Configurable category rules for documents, images, videos, audio, archives,
  code, design files, spreadsheets, presentations, books, installers, logs, and
  uncategorized files.
- Conservative cleanup detection for temporary files, OS metadata junk, empty
  directories, duplicate files, and old archives.
- SHA-256 duplicate detection with size limits; the oldest copy is kept.
- Preview-first workflow with saved JSON plans.
- Compact approval for large plans: apply all actions, organize only,
  quarantine only, choose individual actions, or cancel.
- Quarantine-first cleanup under `.lla-quarantine/<plan_id>/`.
- Preflight validation before any move: missing sources, stale targets, duplicate
  sources, and unsafe source/target chains stop the run before changes happen.
- Incremental run manifests with source path, target path, operation type,
  timestamp, optional hash, completion status, and restore status.
- Restore support for completed runs.
- Doctor diagnostics for partial runs, missing targets, orphaned quarantine
  files, and repairable moved files.

## Usage

```bash
# Analyze a folder without saving or moving anything
lla plugin run folder_cleaner scan -- ~/Downloads

# Preview and save a proposed plan
lla plugin run folder_cleaner preview -- ~/Downloads downloads

# Preview, interactively approve, and apply selected actions
lla plugin run folder_cleaner clean -- ~/Downloads downloads

# Apply a saved plan
lla plugin run folder_cleaner apply -- plan-20260508090000000

# Restore files moved by a previous run
lla plugin run folder_cleaner restore -- run-20260508090000000

# If you pass a plan id to restore, or a run-like id made from a plan timestamp,
# folder_cleaner resolves the applied run when there is exactly one match.

# Inspect history and render saved plans
lla plugin run folder_cleaner history
lla plugin run folder_cleaner show-plan -- plan-20260508090000000

# Diagnose run health; add --repair to restore recoverable moved files
lla plugin run folder_cleaner doctor
lla plugin run folder_cleaner doctor -- run-20260508090000000 --repair

# Inspect or empty quarantine
lla plugin run folder_cleaner quarantine-list
lla plugin run folder_cleaner quarantine-empty -- 30

# Edit common profile settings
lla plugin run folder_cleaner config-wizard
```

## Configuration

The config file is created at:

```text
~/.config/lla/plugins/folder_cleaner/config.toml
```

Important sections:

```toml
[scan]
recursive = true
max_depth = 8
include_hidden = false
follow_symlinks = false
same_filesystem = true
ignore_patterns = [".git", "node_modules", "target", ".venv", ".lla-quarantine"]

[safety]
quarantine_dir = ".lla-quarantine"
require_confirmation = true
allow_permanent_delete = false
collision_policy = "rename"

[cleanup]
level = "conservative"
duplicate_detection = true
empty_dirs = true
temp_files = true
os_junk = true
old_archives = true
duplicate_max_bytes = 268435456
old_archive_days = 30
```

Rules use extensions plus optional glob patterns:

```toml
[[rules]]
category = "documents"
destination = "Documents"
extensions = ["pdf", "docx", "txt", "md"]
filename_patterns = []
path_patterns = []
```

Built-in image rules include common modern camera and web formats such as
`jpg`, `jpeg`, `png`, `gif`, `bmp`, `svg`, `webp`, `heic`, `avif`, `tif`,
`tiff`, `raw`, `cr2`, and `nef`.

Existing config files are preserved, but the plugin merges newly supported
default rules into them on startup. User settings such as `old_archive_days`
remain unchanged.

Saved plans and run manifests live under:

```text
~/.config/lla/plugins/folder_cleaner/plans
~/.config/lla/plugins/folder_cleaner/runs
```

## Safety Model

`folder_cleaner` does not permanently delete during normal organization or
cleanup, even with aggressive cleanup defaults. Cleanup candidates are moved
into quarantine, and `restore <run_id>` can move completed actions back.
Permanent removal is limited to the explicit `quarantine-empty` action and
still requires confirmation.

If a run is interrupted or files are hard to find, use:

```bash
lla plugin run folder_cleaner doctor
lla plugin run folder_cleaner history
```

`doctor --repair` restores recoverable files whose targets still exist. Orphaned
quarantine files are reported but left in place because their original paths are
unknown.
