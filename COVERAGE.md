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

## Interpreting the Report

- **Lines**: percentage of executable lines hit by at least one test
- **Functions**: percentage of functions entered by at least one test
- **Branches**: LLVM branch coverage (currently 0/0 — requires `--branch` flag which is not enabled in the current configuration)
- The `--lib` flag means only `#[cfg(test)]` unit tests contribute. Integration tests (`tests/`) and BDD tests are excluded.
- Files appearing twice in raw JSON is expected when using shared `target-dir` across worktrees — the deduplicated per-crate numbers above are authoritative.

## Web E2E Coverage Notes

- Playwright BDD coverage includes shop settings LAN status states (`Active`, `Unavailable`, `Disabled`) and copy-to-clipboard behavior.
