# Contributing to Mokumo

> Mokumo is pre-alpha and under active development by Breezy Bays Labs. We're not actively soliciting external contributions yet, but we want the bar for "could a contributor figure this out" to be high. This document captures what's needed to work in the repo productively.

For the architectural map (what the crates do and why), read [ARCHITECTURE.md](ARCHITECTURE.md) first.

For the agent-facing daily-use commands and conventions, see [CLAUDE.md](CLAUDE.md).

For per-crate conventions, see each crate's `AGENTS.md` (currently `crates/kikan/AGENTS.md` and `crates/mokumo-shop/AGENTS.md`).

---

## Toolchain

You'll need the following installed locally:

| Tool | Purpose | Install |
|---|---|---|
| **Rust** (stable, 2024 edition) | Backend, kikan engine | `rustup install stable` |
| **Node.js 22+** | SvelteKit frontend | `mise install node@22` (or your version manager) |
| **pnpm** | JS package manager | enabled via the workspace-root `packageManager` pin in `package.json` |
| **Moon** | Polyglot orchestrator (Rust + Node) | `curl -fsSL https://moonrepo.dev/install/moon.sh \| bash` |
| **D2** | Architecture diagram renderer | `brew install d2` |
| **Hurl** | API smoke tests | `brew install hurl` |
| **lefthook** | Pre-commit hooks | `brew install lefthook` then `lefthook install` |

Optional but useful:

- `cargo-nextest` — faster Rust test runner.
- `cargo-llvm-cov` — Rust coverage.
- `cargo-mutants` — Rust mutation testing (Wave 6 adoption).
- `cargo-deny` — supply-chain audit (CI runs this).

---

## First-time setup

```bash
git clone https://github.com/breezy-bays-labs/mokumo.git
cd mokumo
pnpm install            # JS deps via pnpm workspace
moon run shop:db-prepare  # SQLx offline cache (required for Rust compilation)
moon check --all          # Full CI matrix locally; tells you if anything is broken
```

The first `moon check --all` will compile everything and run tests across both the Rust workspace and the SvelteKit project. If it passes, you're set up.

---

## Day-to-day commands

Always go through Moon. Don't invoke `cargo`/`pnpm` directly except when you're debugging a Moon task failure.

```bash
moon run web:dev              # SvelteKit dev server (Vite)
moon run web:build            # Build the static SPA (adapter-static)
moon run web:test             # Vitest unit tests
moon run web:check            # svelte-check type-check

moon run shop:dev             # Axum backend with auto-reload (depends on web:build)
moon run shop:build           # Build Rust backend
moon run shop:test            # cargo test across the Rust workspace
moon run shop:lint            # clippy
moon run shop:fmt             # cargo fmt --check
moon run shop:fmt-write       # cargo fmt
moon run shop:gen-types       # ts-rs: regenerate TypeScript bindings from Rust DTOs
moon run shop:coverage        # llvm-cov coverage (JSON, used by CI)
moon run shop:smoke           # Hurl HTTP smoke tests (requires running server)
moon run shop:deny            # cargo-deny: advisories, licenses, sources

moon check --all              # Full CI matrix
```

Underlying tools: `cargo` (Rust), `pnpm` (Svelte). Moon caches and parallelizes them. When a Moon task fails, the underlying command is in the output; you can usually run it directly to iterate faster.

---

## Branching, commits, and PRs

- **Worktrees, never branch-switching in the main checkout.** Use `git worktree add ../mokumo.worktrees/<branch> <branch>` for parallel work. Container Claude sessions: the container is the worktree — never run `git worktree add` inside `/workspace`.
- **Branch naming**: `<type>/<short-description>` — e.g. `feat/customer-tag-autocomplete`, `fix/profile-switch-rate-limit`, `docs/architecture-foundation`. **No `+` characters in branch names** (breaks GitHub Actions CI; see `feedback_no-plus-in-branch-names.md`).
- **Conventional commits** — `<type>(<scope>): <description>`. Types: `feat`, `fix`, `chore`, `refactor`, `docs`, `test`, `perf`, `ci`. Scope is the area touched (`kikan`, `mokumo-shop`, `web`, `infra`, etc.).
- **Always create a new commit; never `--amend`** unless explicitly asked. Never `--no-verify` — pre-commit hooks fail for a reason; investigate, don't bypass.
- **Never push directly to `main`.** All work merges via PR.
- **PR body**: short summary + a test-plan checklist. We don't have a PR template yet — keep it tight, link issues with `Closes #N`.

---

## Quality gates

Mokumo has stricter quality gates than typical pre-alpha projects, because Kikan's reliability story is the core of the bet. CI enforces:

### Workspace invariants (I1–I5)

See [ARCHITECTURE.md §8 Quality invariants](ARCHITECTURE.md#8-quality-invariants). Each is a small grep / `cargo tree` script under `scripts/check-i*.sh`. If your PR adds a new crate or rearranges deps, you may need to extend the relevant script in the same PR.

- **I1** — domain purity in `crates/kikan/src/`. The regex is wholly literal — don't write doc comments naming forbidden words even with negation.
- **I2 / I2b** — no `tauri::` symbols leaking outside `kikan-tauri` / desktop binary.
- **I3** — `mokumo-server` is Tauri-free.
- **I4** — one-way DAG; `kikan` has no incoming workspace-local edges.
- **I5** — no Cargo feature reaches Tauri into kikan.

### CRAP gate

`crap4rs` runs in CI per crate. The threshold is 25 (default). PRs that raise any function above the threshold either refactor in-session or file a follow-up issue at closeout. A "per-session CRAP delta gate" (#569) is part of Wave 6 cross-cutting work.

### BDD coverage

`.feature` files in `crates/mokumo-shop/tests/features/` and `apps/web/src/lib/components/**/*.feature` are the living spec. New behavior gets a scenario; new bug fixes get a regression scenario. `bdd-lint` runs in CI to catch stale `@wip` tags and orphaned step defs.

- Rust BDD harness: `cucumber-rs` via `moon run shop:test -- --features bdd`.
- TypeScript BDD: Playwright BDD steps for the SvelteKit frontend.

### Negative-path testing

Standard at `ops/standards/testing/negative-path.md` (private). Before writing any conditional / path-matching / range-checking code, write the boundary cases and the "almost right" rejection case before the happy path. Failing to do so is one of the most common reasons CodeRabbit and the silent-failure-hunter agent flag a PR.

### Mutation testing

- **Frontend**: Stryker was retired pre-users pending TS-side logic growth; revisited on canary/nightly cadence once that surface expands.
- **Backend**: `cargo-mutants` is available as `moon run shop:mutate` for local runs; promotion to a CI gate is tracked under epic #370. `proptest` remains the primary adversarial-input mechanism for domain crates.

### Hurl smoke

Every API endpoint gets a `tests/api/<domain>/<endpoint>.hurl` file. Error shape is `{"code": "...", "message": "...", "details": null}` — assert on `$.code`, **not** `$.error` (the field doesn't exist in our error envelope and the assertion will silently pass on a missing field).

### `cargo-deny` supply chain

After touching `Cargo.toml` or `Cargo.lock`, run `moon run shop:deny` locally before pushing. Inside cmux containers the recipe is:

```bash
env -u RUSTC_WRAPPER CARGO_HOME=/tmp/cargo-home-$$ cargo deny --manifest-path Cargo.toml check
```

(The container's sccache wrapper and shared cargo home don't play nicely with `cargo-deny`'s temp-dir behavior.)

---

## Where to put what

This is the question new contributors and agents get wrong most often. Defer to ARCHITECTURE.md §2 for the full crate map. Quick rules:

- **Shop-vertical concept** (customer, garment, quote, invoice, kanban, decoration, ...) → `crates/mokumo-shop/`.
- **Decoration-technique-specific** (screen print, embroidery, DTF, ...) → `crates/extensions/{technique}/` post-Wave-8. Until then, neutral foundations only in `mokumo-shop`.
- **Platform-level mechanism** (migration runner, backup, auth backend, control-plane handler, deployment-mode middleware, mDNS, ...) → `crates/kikan/`.
- **Wire DTO** (anything serialized to JSON for the frontend) → `crates/kikan-types/` with `#[derive(TS, Serialize, Deserialize)]`.
- **Adapter** (Tauri-shell helper, Unix socket listener, CLI subcommand, ...) → corresponding `crates/kikan-{tauri,socket,cli}/`.
- **Optional platform feature** (event bus, mailer, scheduler) → existing `kikan-{events,mail,scheduler}` SubGraft crate, registered via `BootConfig::with_subgraft(...)`.
- **Frontend route or component** → `apps/web/src/routes/` or `apps/web/src/lib/components/`.

If a piece of code "feels convenient" to put in `crates/kikan/`, pause. It probably belongs on the `Graft` trait or in `mokumo-shop`. The kikan-extraction-finalization-plan and ARCHITECTURE.md §5 describe the pattern.

---

## Architecture diagrams (D2)

Diagrams under `docs/diagrams/` are D2 source. The committed `.svg` is the rendered output that GitHub displays.

To re-render after editing a `.d2` file:

```bash
d2 docs/diagrams/<name>.d2 docs/diagrams/<name>.svg
```

CI fails if a `.d2` file changed without a corresponding `.svg` update (workflow `.github/workflows/docs-d2.yml`). Commit both files in the same change.

---

## When in doubt

- **Architecture question** → ARCHITECTURE.md.
- **Day-to-day commands and conventions** → CLAUDE.md.
- **Per-crate conventions and gotchas** → that crate's `AGENTS.md`.
- **Why a decision was made** → ADRs in `ops/decisions/mokumo/` (private; the Y-statement summary in ARCHITECTURE.md §11 covers the load-bearing ones).
- **Security concern** → SECURITY.md.

If none of those answers your question, open a discussion or DM the maintainer. We'd rather answer the question than have you guess wrong.
