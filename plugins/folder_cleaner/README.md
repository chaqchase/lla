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
- Quarantine-first cleanup under `.lla-quarantine/<plan_id>/`.
- Run manifests with source path, target path, operation type, timestamp,
  optional hash, and restore status.
- Restore support for completed runs.

## Usage

```bash
# Analyze a folder without saving or moving anything
lla plugin --name folder_cleaner --action scan --args ~/Downloads

# Preview and save a proposed plan
lla plugin --name folder_cleaner --action preview --args ~/Downloads downloads

# Preview, interactively approve, and apply selected actions
lla plugin --name folder_cleaner --action clean --args ~/Downloads downloads

# Apply a saved plan
lla plugin --name folder_cleaner --action apply --args plan-20260508090000000

# Restore files moved by a previous run
lla plugin --name folder_cleaner --action restore --args run-20260508090000000

# Inspect or empty quarantine
lla plugin --name folder_cleaner --action quarantine-list
lla plugin --name folder_cleaner --action quarantine-empty --args 30

# Edit common profile settings
lla plugin --name folder_cleaner --action config-wizard
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
old_archive_days = 90
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

Saved plans and run manifests live under:

```text
~/.config/lla/plugins/folder_cleaner/plans
~/.config/lla/plugins/folder_cleaner/runs
```

## Safety Model

`folder_cleaner` does not permanently delete during normal organization or
cleanup. Cleanup candidates are moved into quarantine, and `restore <run_id>`
can move them back. Permanent removal is limited to the explicit
`quarantine-empty` action and still requires confirmation.
