# Views and display

`lla` can list a directory, archive, or individual file. A view changes how the
same selected entries are presented; filters and sorting can be combined with
these views unless a command documents a narrower behavior.

## Choose a view

| View | Best for |
| --- | --- |
| Default | Fast, compact directory scans. |
| Long | Permissions, ownership, dates, and filesystem metadata. |
| Tree | Directory hierarchy. |
| Table | Comparing selected columns across entries. |
| Grid | Dense directories with short names. |
| Git | Repository status and Git metadata. |
| Timeline | Grouping entries by age. |
| Sizemap | Comparing file and optional directory sizes. |
| Recursive | Walking nested directories as a flat recursive listing. |
| Fuzzy | Interactive discovery and file actions; see [Navigation](navigation.md#fuzzy-view). |

## Default view

```bash
lla
```

<img src="https://github.com/user-attachments/assets/3517c63c-f4ec-4a51-ab6d-46a0ed7918f8" className="rounded-2xl" alt="default" />

## Long view

Use long view for permissions, size, dates, ownership, and optional filesystem
metadata:

```bash
lla -l
lla -l --hide-group --relative-dates
lla -l --date-format "%Y-%m-%d %H:%M"
lla --inode --links --allocated-size
lla --extended --context --mounts
```

<img src="https://github.com/user-attachments/assets/2a8d95e4-efd2-4bff-a905-9d9a892dc794" className="rounded-2xl" alt="long" />

`--include-dirs` calculates recursive directory sizes and can be expensive on
large trees. `--dereference` uses symlink-target metadata while retaining link
identity; `--no-symlink-target` hides the rendered `-> target` suffix.

Long-view columns are configurable. Built-in column keys are `permissions`,
`inode`, `links`, `size`, `allocated`, `modified`, `created`, `accessed`, `user`,
`group`, `xattrs`, `context`, `mount`, `name`, `path`, and `plugins`. A plugin
field uses `field:<name>`.

```toml
[formatters.long]
hide_group = true
relative_dates = true
date_format = "%Y-%m-%d %H:%M"
columns = ["permissions", "size", "modified", "name", "field:git_status"]
```

## Tree view

```bash
lla -t
lla -t -d 3
```

<img src="https://github.com/user-attachments/assets/cb32bfbb-eeb1-4701-889d-f3d42c7d4896" className="rounded-2xl" alt="tree" />

## Table view

```bash
lla -T
```

<img src="https://github.com/user-attachments/assets/9f1d6d97-4074-4480-b242-a6a2eace4b38" className="rounded-2xl" alt="table" />

Table columns accept the same built-in and `field:<name>` syntax as long view:

```toml
[formatters.table]
columns = ["permissions", "size", "modified", "name", "field:git_branch"]
```

## Grid view

```bash
lla -g
lla -g --grid-ignore
```

`--grid-ignore` ignores terminal width, so output may extend beyond the screen.

<img src="https://github.com/user-attachments/assets/b81d01ea-b830-4833-8791-7b62ff9137df" className="rounded-2xl" alt="grid" />

## Git view

```bash
lla -G
```

<img src="https://github.com/user-attachments/assets/b0654b20-c37d-45c2-9fd0-f3399fce385e" className="rounded-2xl" alt="git" />

## Timeline view

```bash
lla --timeline
```

<img src="https://github.com/user-attachments/assets/06a156a7-628a-4948-b75c-a0da584c9224" className="rounded-2xl" alt="timeline" />

## Sizemap view

```bash
lla -S
lla -S --include-dirs
```

The second form includes recursively calculated directory sizes.

<img src="https://github.com/user-attachments/assets/dad703ec-ef23-460b-9b9c-b5c5d6595300" className="rounded-2xl" alt="sizemap" />

## Recursive view

```bash
lla -R
lla -R -d 3
lla -R -l
```

<img src="https://github.com/user-attachments/assets/f8fa0901-8866-4b92-a76e-3b7fd307f04e" className="rounded-2xl" alt="recursive" />

## Archive contents

Archives can be listed as virtual directories without extraction. Supported
formats are `.zip`, `.tar`, `.tar.gz`, and `.tgz`.

```bash
lla my_archive.zip -t
lla project.tar.gz -l
lla my_archive.tgz --json
lla my_archive.zip -l -f ".rs"
```

## A single file

```bash
lla README.md
lla Cargo.toml -l
lla src/main.rs --json
```

## Compare files and directories

The `diff` command compares two local paths or one path against a Git reference:

```bash
lla diff src ../backup/src
lla diff apps/api --git
lla diff src --git --git-ref HEAD~1
lla diff Cargo.lock ../backup/Cargo.lock
lla diff Cargo.lock --git --git-ref HEAD~1
```

Directory comparisons show status and size deltas. File comparisons show size
and line-count changes plus a unified diff; binary content is detected and is not
dumped to the terminal.

## Presentation modifiers

| Option | Purpose |
| --- | --- |
| `--icons`, `--no-icons` | Override icon display for one invocation. |
| `--hyperlink always\|auto\|never` | Control OSC 8 links. |
| `--no-color` | Disable color output. |
| `--permission-format <format>` | Use `symbolic`, `octal`, `binary`, `verbose`, or `compact`. |
| `--date-format <format>` | Use a Chrono strftime format for long-view dates. |

For selection and ordering, see [Filtering and search](filtering-and-search.md).
For stable script output, see [Machine output](machine-output.md).
