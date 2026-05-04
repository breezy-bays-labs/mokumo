# Coverage Baseline

Rust workspace coverage via [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov).

## Quick Start

```bash
# JSON report (used by CI)
moon run shop:coverage

# HTML report (local dev — open coverage/rust/html/index.html)
moon run shop:coverage-report
```

Both commands run unit tests only (`--lib`). Integration and BDD tests are excluded intentionally at M0 — add `--tests` when domain logic in `crates/core/` warrants full-stack coverage.

## Baseline (2026-03-26)

Measured against `main` at commit `b22e1b4`.

### Per-Crate Summary

| Crate | Lines | Functions |
|-------|-------|-----------|
| core | 138/176 (78.4%) | 30/42 (71.4%) |
| db | 241/618 (39.0%) | 17/57 (29.8%) |
| types | 169/169 (100.0%)* | 23/23 (100.0%)* |
| api | 353/736 (48.0%) | 46/91 (50.5%) |
| **Total** | **901/1699 (53.0%)** | **116/213 (54.5%)** |

*\*types crate 100% reflects derive-macro expansion tests (ts-rs export bindings), not serialization edge-case coverage.*

### WebSocket Module Detail (`crates/mokumo-shop/src/ws/`)

| File | Lines | Notes |
|------|-------|-------|
| `manager.rs` | 70/70 (100%) | All public methods fully covered |
| `mod.rs` | 62/139 (44.6%) | Sync helpers covered; async handlers (`handle_socket`, `ws_handler`) and debug endpoints uncovered |

**Covered functions** in `ws/mod.rs`: `origin_host_port`, `is_allowed_origin`
**Uncovered functions**: `ws_handler`, `handle_socket`, `debug_connections`, `debug_broadcast` — these require WebSocket integration tests.

### Notable Gaps

| Module | Coverage | Why |
|--------|----------|-----|
| `core/customer/service.rs` | 0% | Service layer not yet exercised by unit tests (tested via integration tests excluded by `--lib`) |
| `core/sequence/mod.rs` | 0% | Trait-only module, no concrete logic to test |
| `db/sequence/repo.rs` | 0% | No unit tests yet |
| `api/lib.rs` | 0% | Server bootstrap — tested by integration tests |
| `api/customer/mod.rs` | 0% | Handler layer — tested by integration tests |
| `api/activity/mod.rs` | 0% | Handler layer — tested by integration tests |

## Threshold Policy

### M0 (Current): Advisory

Coverage numbers are tracked and reported but **not enforced as CI gates**. The baseline establishes a reference point for measuring progress. Regressions are flagged in PR reviews but do not block merges.

### M1+: Enforced

Coverage gates will be introduced with per-crate thresholds:

| Crate | Target | Rationale |
|-------|--------|-----------|
| `core` | 80%+ | Domain logic — highest value coverage |
| `types` | 90%+ | Serialization correctness is critical |
| `db` | 60%+ | Repository impls, constrained by test database setup |
| `api` | 50%+ | Handler layer, much tested via integration |

Enforcement via `cargo llvm-cov` `--fail-under-lines` flag added to the CI coverage step.

## CI Integration

The `coverage-rust` job in `.github/workflows/quality.yml` runs `moon run shop:coverage` and uploads `coverage.json` as an artifact (`rust-coverage`). This job only runs on pushes to `main` (not on PRs). Download from any main-branch CI run's Artifacts tab.

### Per-handler branch coverage (mokumo#583)

The scorecard's `Row::CoverageDelta` row carries a per-handler drill-down keyed on the HTTP routes registered through `Router::route(...)` / `Router::nest(...)` calls in each crate. The data flows through three independent stages:

1. **Branch-coverage capture** — `moon run shop:coverage-branches` runs `cargo llvm-cov nextest --lib --branch --workspace …` against a **pinned nightly toolchain**. Branch instrumentation (`-Zcoverage-options=branch`) is nightly-only on rustc as of 1.97 (tracked under [rust-lang/rust#124137](https://github.com/rust-lang/rust/issues/124137)). The pin lives in `rust-nightly-coverage-toolchain.toml` at the workspace root and is consumed by exactly two callers via `scripts/nightly-coverage-channel.sh`: this moon task and the `coverage-handlers` CI job. Every other repo task stays on the stable channel pinned by `rust-toolchain.toml`.
2. **Route-walker producer** — `cargo run -p docs-gen --bin coverage-breakouts -- --workspace-root . --coverage-json coverage-branches.json --output coverage-breakouts.json` parses the LLVM JSON, walks the AST of each crate's HTTP routes (`syn::visit::Visit`) to recover `(route, rust_path)` pairs, joins them against the demangled coverage data, and emits a producer artifact carrying both wire fields (`route`, `branch_coverage_pct`) and producer-internal diagnostics (`rust_path`, branches total/covered, unresolved-handler list).
3. **Aggregator + threshold gate** — `aggregate --coverage-breakouts-json coverage-breakouts.json` translates the producer artifact to the wire-only `Breakouts { by_crate[], handlers[] }` shape, populates the `CoverageDelta` row, and runs the worst-of handler-coverage threshold gate from `quality.toml` (`[rows.coverage_handler]`, defaults: `warn_pct_below = 60.0`, `fail_pct_below = 40.0`, `report_only = true`). The renderer drops a `<details>`-wrapped per-crate drill-down under the row.

The whole pipeline is **non-blocking by design**: the `coverage-handlers` CI job runs `continue-on-error: true`, the threshold gate's `report_only = true` default means a low-coverage handler never escalates the row past the row's own delta-driven status, and the renderer falls back to a "producer pending" note when no artifact is present. A nightly-toolchain outage shows up in the CI UI and the scorecard's drill-down section but never blocks the merge queue. **Promotion to a hard gate is two paired edits** — remove `continue-on-error: true` on `coverage-handlers` and flip `report_only = false` in `quality.toml`.

**Removal path** when branch coverage stabilizes on rustc stable: delete `rust-nightly-coverage-toolchain.toml`, `scripts/nightly-coverage-channel.sh`, the `coverage-handlers` CI job, and the `shop:coverage-branches` moon task in one PR; switch the producer to consume the existing stable `coverage.json` instead.

## Interpreting the Report

- **Lines**: percentage of executable lines hit by at least one test
- **Functions**: percentage of functions entered by at least one test
- **Branches**: LLVM branch coverage (currently 0/0 — requires `--branch` flag which is not enabled in the current configuration)
- The `--lib` flag means only `#[cfg(test)]` unit tests contribute. Integration tests (`tests/`) and BDD tests are excluded.
- Files appearing twice in raw JSON is expected when using shared `target-dir` across worktrees — the deduplicated per-crate numbers above are authoritative.

## Web E2E Coverage Notes

- Playwright BDD coverage includes shop settings LAN status states (`Active`, `Unavailable`, `Disabled`) and copy-to-clipboard behavior.
