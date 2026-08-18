# Machine output

`lla` provides stable JSON, NDJSON, and CSV listing modes for scripts and data
pipelines. They preserve normal path, filtering, sorting, depth, and archive
behavior; only rendering changes. JSON and NDJSON include plugin fields, while
listing CSV uses the fixed schema below.

## Formats

```bash
lla --json
lla --json --pretty
lla --ndjson
lla --csv
```

- `--json` streams one JSON array. `--pretty` only affects this mode.
- `--ndjson` emits one JSON object per line.
- `--csv` emits a header followed by data rows.
- The three output flags are mutually exclusive.

## JSON and NDJSON fields

```json
{
  "path": "src/main.rs",
  "name": "main.rs",
  "extension": "rs",
  "file_type": "file",
  "size_bytes": 1234,
  "modified": "2024-05-01T12:34:56Z",
  "created": null,
  "accessed": null,
  "mode_octal": "0644",
  "owner_user": "mohamed",
  "owner_group": "staff",
  "inode": 1234567,
  "hard_links": 1,
  "allocated_size_bytes": 4096,
  "xattrs": {"user.example": 12},
  "has_acl": false,
  "security_context": null,
  "mount_point": "/",
  "mount_source": "/dev/sda1",
  "filesystem": "ext4",
  "symlink_target": null,
  "is_hidden": false,
  "git_status": "M.",
  "plugin": {}
}
```

`extension`, timestamps other than `modified`, ownership, filesystem metadata,
the symlink target, and Git status can be `null`. `file_type` is `file`, `dir`,
`symlink`, or `other`. The `plugin` object contains enabled plugin fields.

## CSV columns

CSV uses this fixed column order:

```text
path,name,extension,file_type,size_bytes,modified,created,accessed,mode_octal,owner_user,owner_group,inode,hard_links,allocated_size_bytes,xattrs,has_acl,security_context,mount_point,mount_source,filesystem,symlink_target,is_hidden,git_status
```

Plugin actions have their own typed output modes. See
[Installing and managing plugins](plugins/README.md#run-an-action).

For selecting records before emission, see
[Filtering and search](filtering-and-search.md).

## Search results

Content search uses a separate output contract:

```bash
lla --search "TODO" --json
lla --search "TODO" --ndjson
lla --search "TODO" --csv
```

For search, both `--json` and `--ndjson` pass through ripgrep's newline-delimited
JSON event stream. It contains ripgrep event types such as `begin`, `match`,
`end`, and `summary`; it is not the listing JSON array described above.
`--pretty` does not transform this event stream.

Search CSV currently prints one line per collected match under these columns:

```text
file,line,column,kind,text
```

This search-specific renderer does not fully escape commas or quotes in file
names, so use the ripgrep JSON event stream when paths may contain those
characters.

See [Filtering and search](filtering-and-search.md#search-file-contents) for the
search filters that are currently applied.
