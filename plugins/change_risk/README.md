# Change Risk Plugin

Adds a compact risk score to Git view using file size, structural complexity, Git
churn, commit count, and current worktree state.

```bash
lla --git --enable-plugin change_risk
lla plugin run change_risk inspect -- ./src/main.rs
```

The score is a prioritization hint, not a correctness or security verdict.
