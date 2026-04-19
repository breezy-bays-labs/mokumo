# Mokumo Agent Notes

- Use `$ops-conventions` whenever work here needs pipeline notes, board state, decision records, closeout logs, or other private ops artifacts in `/Users/cmbays/github/ops`.
- Use `$pr-review-hygiene` for GitHub PR reviews and disposable review worktrees.
- Cross-repo reference note: `~/.codex/AGENTS.md`.
- Global skills: `~/.codex/skills/ops-conventions`, `~/.codex/skills/pr-review-hygiene`.
- This repo uses a shared Cargo target directory via `.cargo/config.toml`; let worktrees inherit it normally.
- Preserve any worktree the user identifies as active.

## Repo Context

- Architecture: Moon monorepo with `apps/web` (SvelteKit), `apps/mokumo-server` (headless Axum binary), `apps/mokumo-desktop` (Tauri), and Rust crates under `crates/` (`kikan`, `mokumo-shop`, `kikan-cli`, `kikan-socket`, etc.). The Moon `shop:` project points at `crates/mokumo-shop`.
- Testing: prefer repo tasks over ad hoc commands. `moon check --all` is the broadest validation path; `moon run web:test` covers frontend tests; `moon run shop:test` covers backend unit + integration tests; `moon run shop:test-bdd shop:test-bdd-api` covers both shop and HTTP BDD harnesses. BDD suites live under `crates/kikan/tests/` and `crates/mokumo-shop/tests/`; Playwright BDD coverage lives under `apps/web/tests`.
- Quality context: `COVERAGE.md` documents `cargo-llvm-cov`; `tools/bdd-lint` enforces BDD spec and step-definition hygiene.
- Safety: do not push directly to `main`, do not modify `.github/workflows/*` unless the task clearly requires CI changes, and keep private operational state in `ops`, not this repo.
