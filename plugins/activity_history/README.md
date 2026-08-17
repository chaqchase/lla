# Activity History Plugin

Enriches timeline entries with their latest Git commit, author, age, and total commit
count.

```bash
lla --timeline --enable-plugin activity_history
lla plugin run activity_history inspect -- ./src/main.rs
```

Untracked paths remain undecorated so the timeline stays compact.
