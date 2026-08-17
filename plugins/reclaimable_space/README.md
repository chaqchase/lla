# Reclaimable Space Plugin

Highlights generated directories, caches, and temporary files in the sizemap view and
reports how many bytes can be reclaimed. It deliberately avoids classifying ordinary
user files as disposable.

```bash
lla --sizemap --enable-plugin reclaimable_space
lla plugin run reclaimable_space inspect -- ./target
```

Results include a confidence level and reason. Review paths before deleting them; the
plugin never removes files itself.
