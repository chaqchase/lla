# Security Audit Plugin

Audits listing entries and directory trees without reading file contents. It reports
world-writable entries, SUID/SGID bits, dangling or escaping symlinks, and secret-like
files whose permissions expose them to a group or other users.

```bash
lla --enable-plugin security_audit --long .
lla --enable-plugin security_audit --json --pretty .
lla plugin security_audit audit . -- --recursive
```

Machine output includes `security_risk`, `security_score`, `security_findings`,
`suspicious_symlink`, and `secret_exposed`.
