# ADR: Coverage Exclusions for BDD-only and Binary-entrypoint Crates

**Status**: Accepted
**Date**: 2026-04-26
**PR**: #692

## Context

Mokumo's coverage and CRAP-score quality gates rely on `cargo-llvm-cov`
emitting LCOV from a Rust workspace's unit + integration tests. Several
sets of paths in the workspace produce **0% line coverage by design**
under `cargo nextest` because their behavior is exercised by harnesses
that don't write LCOV records:

1. **Cucumber-rs BDD harnesses** — `crates/mokumo-shop/tests/api_bdd/`,
   `crates/mokumo-shop/tests/admin_uds.rs`, etc. The `cucumber` crate
   uses a custom test harness incompatible with `nextest`, and its
   scenario steps don't produce per-line coverage in the harness output.
   Handlers driven exclusively through these harnesses (e.g., kikan
   platform/control-plane, mokumo-shop's HTTP adapters and admin UDS
   handlers) therefore read as 0% covered, even though semantic
   coverage through the BDD scenarios is high.

2. **Binary entry points** — `apps/mokumo-server/`, `apps/mokumo-desktop/`,
   `crates/mokumo-shop/src/{startup,cli}.rs`. These have no unit-test
   surface; their behavior is verified by `demo-smoke`, `api-smoke`,
   `desktop-e2e` integration jobs, none of which emit LCOV.

3. **Tauri-specific crates** — `kikan-tauri`, `mokumo-desktop`. Tauri's
   runtime requires a desktop event loop; `cargo nextest` can't host
   one, so these crates can't be tested under llvm-cov at all. They
   get their own desktop-e2e job.

4. **Frontend-bundling crate** — `kikan-admin-ui`. Uses `rust-embed`
   with `#[folder = "frontend/build"]` — a *compile-time* check that
   the SvelteKit build output exists. `cargo nextest`-based coverage
   doesn't run `pnpm build` first, so the crate fails to compile in
   the coverage pipeline.

5. **Test/build helpers** — `**/tests/**`, `**/*_tests.rs`,
   `**/build.rs`. By definition either zero coverage (test fixtures
   are the test input) or zero meaningful CRAP score (build scripts).

If we let any of these flow through the coverage gate, the threshold
trips on permanently-uncovered code, gate noise drowns out real
regressions, and the team learns to ignore the gate. Excluding them is
the right answer — the question is **how** to track that exclusion.

## Decision

These exclusions are **architectural and permanent**. They are
documented here once; the config sites reference this ADR rather than
carrying ad-hoc justifications.

The exclusions live in two places:

### 1. `crap4rs.toml` — CRAP score gate

```toml
preset = "strict"

# Architectural exclusions — see docs/adr/adr-coverage-exclusions.md
exclude = [
  # BDD-only handlers (Cucumber-rs harness, no LCOV)
  "crates/kikan/src/platform/**",
  "crates/kikan/src/control_plane/**",
  "crates/mokumo-shop/src/restore_handler.rs",
  "crates/mokumo-shop/src/profile_switch.rs",
  "crates/mokumo-shop/src/routes.rs",
  "crates/mokumo-shop/src/setup.rs",
  "crates/mokumo-shop/src/settings.rs",
  "crates/mokumo-shop/src/server_info.rs",
  "crates/mokumo-shop/src/ws/handler.rs",
  "crates/mokumo-shop/src/admin/**",
  "crates/mokumo-shop/src/auth_handlers/**",
  "crates/mokumo-shop/src/demo_reset.rs",
  "crates/mokumo-shop/src/user_admin.rs",
  "crates/kikan/src/logging.rs",
  # Binary entry points
  "apps/mokumo-desktop/**",
  "apps/mokumo-server/**",
  "crates/mokumo-shop/src/startup.rs",
  "crates/mokumo-shop/src/cli.rs",
  # Integration-test client library
  "crates/kikan-cli/**",
  # Test fixtures and build scripts
  "**/tests/**",
  "**/*_tests.rs",
  "**/build.rs",
]
```

### 2. `crates/mokumo-shop/moon.yml` — coverage and clippy commands

```yaml
# All cargo invocations carry the same --exclude trio. Rationale: see
# docs/adr/adr-coverage-exclusions.md
test:
  command: cargo nextest run --profile ci --workspace --exclude mokumo-desktop --exclude kikan-tauri --exclude kikan-admin-ui
crap:
  command: |
    cargo llvm-cov nextest --workspace --exclude mokumo-desktop --exclude kikan-tauri --exclude kikan-admin-ui --lcov ...
# (and coverage-rust, coverage-html, lint-rust)
```

The CI workflow's `crap-scorecard` job mirrors the same exclude trio
inline.

## Consequences

### Positive

- **Gate signal stays meaningful.** Threshold violations represent
  real regressions on code that *can* be unit-tested.
- **Single source of truth.** Future maintainers hitting an unexpected
  exclusion can grep for the ADR reference and understand the
  rationale immediately.
- **Promotion path is explicit.** If a BDD-only handler grows a
  unit-testable helper module, that module *can* and *should*
  graduate out of the exclusion — this ADR doesn't grant blanket
  immunity to the surrounding crate.

### Negative / risks

- **Coverage of these paths is invisible to the rust matrix.** A
  regression in handler logic that the BDD harness doesn't catch
  would slip past every coverage-driven gate. Mitigation: the
  `api-smoke`, `demo-smoke`, `desktop-e2e`, and `kikan-invariants`
  jobs cover these surfaces from a different angle.
- **Drift risk.** New crates added to `apps/` or new BDD-only
  handlers added to `mokumo-shop` won't be auto-excluded; someone has
  to update both `crap4rs.toml` and `moon.yml`. Quarterly grep audit
  catches missing references.

## Out of scope

- **Cucumber `@wip` and `@future` tags** in `tools/bdd-lint`. Each
  tagged scenario should have its own tracking issue, not an ADR
  reference. Audit owned by #696.
- **Test exclusions added by individual PRs** (e.g., a temporarily-
  flaky test). Those follow the issue-tracking path, not the ADR
  path.

## When this ADR should be revisited

- Cucumber-rs gains nextest compatibility or LCOV emission → most
  BDD-only paths can re-enter the coverage gate.
- A `kikan-admin-ui` build-artifact pre-step lands in the coverage
  workflow → that crate can drop out of the `--exclude` list.

## References

- **Surfaced during:** PR #692 (CRAP scorecard wiring, 2026-04-26) —
  the new `crap-scorecard` job inherited the exclusion trio without
  inline justification, prompting this ADR.
- **Related ADR:** `docs/adr/adr-bdd-given-step-repo-direct.md` — why
  BDD owns these handlers' verification.
- **CRAP delta gate:** `scripts/check-crap-delta.sh` (slated for
  retirement after the scorecard stabilizes).
- **Companion issues:** #569 (per-session CRAP delta gates), #650
  (metrics-delta PR comment bot), #696 (`@wip` / `@future` tag
  audit).
