# Trash Plugin

Provides recoverable deletion with the same behavior on macOS, Linux, and Windows.
Content is moved into lla's plugin data directory with a JSON record of its original
path, deletion time, size, and restore id. If a restore target exists, a conflict-safe
`(restored N)` name is chosen instead of overwriting it.

```bash
lla plugin run trash put -- ./draft.txt ./old-directory
lla plugin run trash list
lla plugin run trash restore -- <id>

# Irreversible and intentionally requires --yes
lla plugin run trash empty -- 30 -- --yes
```

The existing `file_remover remove` action uses this same store. Its `purge` action is
the explicit permanent-deletion alternative.
