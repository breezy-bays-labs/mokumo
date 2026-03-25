# Mokumo Agent Notes

- For GitHub PR reviews and disposable review worktrees, use the global Codex skill `$pr-review-hygiene`.
- The cross-repo reference note lives at `~/.codex/AGENTS.md`.
- The executable global workflow lives at `~/.codex/skills/pr-review-hygiene`.
- Invoke it explicitly when needed, for example: `Use $pr-review-hygiene to review PR #58.`
- This repo uses a shared Cargo target directory via `.cargo/config.toml`; let worktrees inherit it normally.
- Preserve any worktree the user identifies as active.
