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

## Synchronized-Docs

Two mechanisms keep code and prose in sync. Both live here so a contributor with one change in hand can find the other one they owe.

### A. AUTO-GEN marker registry

Files with `<!-- AUTO-GEN:name -->` / `<!-- /AUTO-GEN:name -->` markers have sections owned by the `docs-gen` binary (`tools/docs-gen`). **Never edit between these markers by hand** — the generator overwrites them on the next run.

After changing any source listed below, regenerate and verify before pushing:

```bash
moon run docs:gen
git diff --exit-code   # must be empty
```

The registry of every owned section lives in `tools/docs-gen/src/registry.rs`. Adding a new section is two changes: write a `render_*` function and append a `Section` entry. The marker pair must already exist in the target file.

| Marker | Source | Target |
|--------|--------|--------|
| `AUTO-GEN:msrv` | `Cargo.toml` (`workspace.package.rust-version`) | `README.md` |
| `AUTO-GEN:adr-index` | `docs/adr/*.md` (YAML frontmatter, sorted by `title`) | [`docs/adr-index.md`](docs/adr-index.md) |

CI enforces this via the `docs-drift` job: every PR regenerates all AUTO-GEN sections and fails if any target file differs from HEAD.

#### ADR `enforced-by:` contract

ADRs that opt into YAML frontmatter must declare an `enforced-by:` list-of-objects so the decision is paired with a runtime check, lint, or test:

```yaml
---
title: ADR-1: Some decision
status: approved
enforced-by:
  - kind: workflow
    ref: .github/workflows/quality.yml
    note: CI gate XYZ trips when invariant is violated
  - kind: human-judgment
    ref: code review
    note: Reviewer confirms the invariant by hand
---
```

`kind` is one of `test | lint | dep-absence | workflow | human-judgment`. The `adr-registry` CI job is **syntactic**: it fails any PR that touches a `docs/adr/*.md` file with YAML frontmatter but no `enforced-by:` key. Reference resolution (paths exist, lints exist, dependencies are absent) lives in [`tools/docs-gen/src/validate.rs`](tools/docs-gen/src/validate.rs); the `adr-validate` binary under `tools/docs-gen/src/bin/` is just a CC=1 shim that calls `validate::execute`. The validator runs from the `adr-validate` lefthook pre-push hook plus local dev shells — CI stays repo-scoped so the canonical 76-ADR vault in the private ops repo doesn't have to be checked out by runners. ADRs without YAML frontmatter (legacy format) are dormant under both gates; adoption is voluntary at the file level.

### B. Paired-files rules

When a class of code changes, a matching prose doc must change in the same PR. Rules **2** and **3** below are enforced by the `docs-paired-files` CI job (and a matching `lefthook` pre-push hook); rules **1** and **4** are semantic (no diff signal) and remain socially enforced — CI for them is tracked in [issue #781](https://github.com/breezy-bays-labs/mokumo/issues/781). The opt-out path for rules 2 + 3 is the `docs-not-applicable` PR label, intended for genuinely internal `pub` items kept public for module-graph reasons (no consumer surface).

| When this changes… | …update this in the same PR | Why |
|---|---|---|
| Trust-boundary code: auth handlers, control / data plane split, container mount config, `DeploymentMode` posture | [`SECURITY.md`](SECURITY.md) | The threat-model document and the boundary code share one truth. A code change that shifts a boundary without a doc update silently moves the trust contract. |
| New `pub` domain entity, repository trait, service, or wire-type under `crates/mokumo-*/` | [`LANGUAGE.md`](LANGUAGE.md) (vertical glossary) | The vertical glossary is the entry point for new contributors looking up shop-domain vocabulary. A new term that lacks an entry sends the reader to read the source. |
| New `pub` platform entity, repository trait, service, or wire-type under `crates/kikan/`, `crates/kikan-events/`, `crates/kikan-mail/`, `crates/kikan-scheduler/`, `crates/kikan-socket/`, `crates/kikan-spa-sveltekit/`, `crates/kikan-tauri/`, `crates/kikan-cli/`, `crates/kikan-types/` | [`crates/kikan/LANGUAGE.md`](crates/kikan/LANGUAGE.md) (platform glossary) | Same rationale, kikan-side. The kikan glossary is the file that travels with the crate post-extraction; keeping it in sync at every PR avoids a one-shot reconciliation later. |
| Architectural change touching the planes, the multi-tenant DB layout, the deployment posture, or the doc-set itself | [`CONTEXT.md`](CONTEXT.md) and (when structural) [`ARCHITECTURE.md`](ARCHITECTURE.md) | `CONTEXT.md` is the doc map; if a new doc lands or an existing one moves, the map must reflect it. `ARCHITECTURE.md` is the structural source of truth; section §11 also tracks ADRs by Y-statement. |

## Dep-graph and verdict assertions

Three CI patterns repeat across this repo and have agent-resistant forms. Use the resistant form for any new assertion.

**Workspace dep-graph assertions** — checks like "kikan does not depend on mokumo-shop" or "mokumo-server has zero transitive `tauri` dependency" must read the resolved graph, never `Cargo.toml` text. Use `cargo metadata --format-version 1` and walk `resolve.nodes`, or `cargo tree --edges=normal,build -p <crate>` for a smaller surface. Regex over `Cargo.toml` files misses transitive paths, dev-dependency leaks, and feature-conditional edges — and an agent rewriting `Cargo.toml` formatting can defeat the regex without breaking the invariant. Existing examples: `scripts/check-server-no-tauri.sh`, `scripts/check-kikan-domain-purity.sh`.

**Verdict-style aggregate gates** — a job that collects results from many upstream jobs (`needs:`) must iterate `${{ toJSON(needs) }}` via `jq`, not maintain a parallel `env:` block of `${{ needs.<name>.result }}` entries. Adding a new gate must be a single-line edit to the `needs:` array; if the assertion code also needs editing, the pattern is wrong. The verdict job in `.github/workflows/quality.yml` is the canonical form: `success` and `skipped` pass, anything else fails. The `if: always()` line is required so the verdict runs even when an upstream job fails or is cancelled.

**Cargo-binary version pinning** — versions of cargo binaries used in CI live in [`tools.toml`](tools.toml) at the repo root. Workflow `bins:` lines (which `moonrepo/setup-rust@v1` passes through to `cargo binstall`) must use the `name@version` form with the version matching `tools.toml`. The `tools-pins` CI job and the `tools-pins` pre-push hook both run [`scripts/check-tools-pins.sh`](scripts/check-tools-pins.sh) which fails on any drift in either direction (pinned-but-unreferenced, referenced-but-unpinned, or version-disagreement). Bumping a pin is two paired edits: `tools.toml` plus every matching `bins:` line. Tools not in `tools.toml` (e.g. `tauri-driver` for desktop e2e) pass through silently — only add a pin when the tool is on the quality-gate critical path.
