# Project Context Plugin

Detects the nearest Rust, Node, Python, or Go project and exposes its root, dependency
lock state, generated artifacts, toolchain versions, and Git branch/working-tree health.
Multi-ecosystem repositories are reported as combined project types.

```bash
lla --enable-plugin project_context --long .
lla --enable-plugin project_context --json --pretty .
lla plugin project_context inspect .
lla plugin project_context refresh
```

The cache avoids running toolchain and Git probes once per displayed entry; use
`refresh` before inspecting state changed by another process.
