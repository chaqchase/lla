# Preview Plugin

Renders bounded terminal previews without extracting archives or modifying files.

- Text and Markdown use `bat` when available, with a safe built-in text fallback.
- Images use `chafa` when available, with a metadata fallback.
- ZIP and tar-family archives are listed via `unzip`/`tar`; uncompressed ZIP and tar
  files also have built-in listing fallbacks.

```bash
lla plugin preview show README.md
lla plugin preview show src/main.rs -- --lines 80
lla plugin preview show screenshot.png
lla plugin preview show release.tar.gz
lla plugin preview backends
```
