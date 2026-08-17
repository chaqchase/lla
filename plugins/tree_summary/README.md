# Tree Summary Plugin

Adds recursive file, directory, and byte totals beside directories in the tree view.

```bash
lla --tree --depth 3 --enable-plugin tree_summary
lla plugin run tree_summary inspect -- .
```

Symlinks are not followed, preventing cycles and summaries outside the selected tree.
