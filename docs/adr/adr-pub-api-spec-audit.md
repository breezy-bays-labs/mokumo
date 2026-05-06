---
title: "ADR: Public-API Spec Audit Gate (BDD coverage of pub items)"
status: approved
issue: mokumo#654
enforced-by:
  - kind: workflow
    ref: .github/workflows/quality.yml
    note: "`pub-api-spec-audit` CI job runs the producer + fail-closed gate; soft-mode landing (continue-on-error: true) until baseline corpus settles."
  - kind: lint
    ref: scripts/check-pub-api-spec-audit.sh
    note: "Fail-closed gate. Exit 2 when a new pub item lacks BDD coverage AND has no allowlist entry."
  - kind: lint
    ref: scripts/check-pub-api-spec-audit-allowlist-drift.sh
    note: "Drift gate. Fails when an allowlisted item's `tracked:` issue closes without the entry being removed."
  - kind: test
    ref: docs_gen::pub_api_audit::producer::tests
    note: "Producer pipeline integration tests cover walker × lcov join, exit-code branches, and crate-discovery edge cases."
---

# Public-API Spec Audit Gate

## Status

Approved 2026-05-06 (mokumo#654, soft-mode landing).

## Context

Epic mokumo#370 (Wave 1 hard-gate spine) commits Mokumo to a per-axis
fail-closed gate: every change must increase the codebase's "specified
behavior" coverage along at least one tracked axis. mokumo#655 covers
the per-route axis (handler × scenario map). This ADR defines the
**per-pub-item axis**: which `pub fn`/`struct`/`enum`/etc. defined in
workspace source is exercised by *any* cucumber-driven scenario?

The producer is a sibling of `coverage::route_walker` (used by mokumo#583
+ mokumo#655) and `scenario_coverage::producer` (mokumo#655). It does
NOT replace either — it's a third independent axis.

## Decision

### D1 — Use `syn` for pub-item enumeration, not `rustdoc --output-format json`

- `syn` is stable-toolchain, deterministic, fits the existing
  `route_walker` pattern, and gives source-line spans without nightly.
- `rustdoc` JSON would follow `pub use` re-exports — useful for
  external-API surface tracking, irrelevant for the question "is this
  defined item BDD-exercised?". Definition site is the right anchor.
- `cargo public-api` (binary) is targeted at SemVer breaking-change
  detection, not per-item coverage attribution. Reusing it would force
  us to also reuse its rustdoc-JSON dependency without adding signal.

### D2 — Coverage attribution: any-line-in-span ≥ 1 hit

- An item is "BDD-covered" iff at least one source line in its
  `[span_begin..=span_end]` registers ≥ 1 lcov hit from a BDD test
  binary. Permissive but matches the question this gate asks ("does
  ANY scenario hit this code?"). Tightening to "every line covered"
  would conflate this gate with branch coverage (mokumo#583's lane).

### D3 — Per-crate baseline files

- Workspace currently has 1029+ pub items across 13 crates. A single
  workspace baseline file would be unreviewable.
- Per-crate files at `.config/pub-api-spec-audit/<crate>.txt` keep
  diffs scoped to the changed crate.
- Allowlist stays workspace-wide (small file with annotation rules).

### D4 — BDD-only lcov scope

- The producer takes `--lcov <PATH>` (multi-occurrence) and merges
  every record. The CI job runs `cargo llvm-cov nextest -E
  'binary(=bdd) | binary(=api_bdd) | binary(=platform_bdd)'` against
  the 6 crates that own BDD harnesses (kikan, kikan-events,
  kikan-mail, kikan-scheduler, mokumo-shop, scorecard) and feeds each
  crate's lcov file in.
- Eight BDD test binaries today: `bdd.rs` × 5 + `api_bdd.rs` +
  `platform_bdd.rs` + scorecard `bdd.rs`. Filtering by binary name
  in nextest captures exactly these.

### D5 — Soft-mode landing posture

- First CI run consumes empty lcov (no BDD coverage capture wired
  in `quality.yml` yet — that's the next promotion step). The gate
  job is `continue-on-error: true` and NOT in the verdict's `needs:`
  during the soft window.
- Promotion to hard-gate is two paired edits when the BDD lcov
  pipeline is wired in (drop `continue-on-error`, add to verdict's
  `needs:`).

## Consequences

- New `tools/docs-gen --bin pub-api-spec-audit` binary; new
  `crates/<x>::*` baselines committed at first run; new ADR keyed on
  `enforced-by:` referring to the workflow + two scripts + producer
  test module.
- Walker has a known limitation: trait-method impls without explicit
  `pub` are NOT counted (default trait-method visibility isn't
  bare-pub). This is intentional — bare `pub` is the gate's target.
- Path to retirement when 100% of pub items have BDD coverage and the
  cost of maintaining the gate exceeds its value: drop the workflow
  job, drop the scripts, drop the producer module, drop this ADR.
  Each of those is a single small PR.

## Pairs with

- mokumo#655 (handler ↔ scenario map) — adjacent gate, both per-axis,
  both fail-closed-with-baseline. They answer different questions:
  #655 asks "is this *route* exercised by happy + 4xx scenarios?";
  #654 asks "is this *defined pub item* exercised by any scenario?".
- mokumo#583 (per-handler branch coverage) — runs against unit tests,
  not BDD. Different axis.
- mokumo#799 (`cargo public-api` breaking-change gate) — orthogonal
  gate using a different tool. Could share rustdoc-JSON infrastructure
  if that direction proves out, but doesn't today.

## Discovery items resolved during shaping

- **Baseline timing vs Wave 1 schema lock**: baseline freezes at this
  PR's merge (commit recorded in each per-crate file's header).
- **Drift gate × tracked-exclusion lint interaction**: the per-axis
  drift gate (`scripts/check-pub-api-spec-audit-allowlist-drift.sh`)
  checks `tracked: <repo>#<n>` references resolve against `gh issue`.
  The repo-wide tracked-exclusion lint (`scripts/check-tracked-exclusions.sh`)
  is unaware of these annotations because allowlist files live outside
  the source tree. Both gates are independent and don't double-fire.
- **Per-crate vs workspace allowlist**: per-crate baselines, workspace
  allowlist. See §D3.
