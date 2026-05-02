# Quality Framework

Mokumo is self-hosted software running critical business data. Quality discipline here is what makes the product trustworthy at all. This document explains what CI enforces, what we measure, and what we plan to add.

## The five-phase quality loop

Every code change flows through a five-phase loop. Each phase catches a different class of defect; skipping phases means those defects ship.

1. **Acceptance scenarios** — Gherkin `.feature` files describe new behavior, confirmed failing before implementation.
2. **TDD red-green-refactor** — unit tests until scenarios pass.
3. **CRAP analysis** — keep function-level complexity-risk under threshold.
4. **Mutation testing** — every surviving mutant gets a test.
5. **Architecture enforcement** — dependency direction + boundary lint.

Each phase has a tool:

| Phase | Rust | TypeScript |
|---|---|---|
| Acceptance | `cucumber-rs` + `axum-test` | `quickpickle` + `@quickpickle/playwright` |
| TDD | `cargo-nextest` | `vitest` + `vitest-browser-svelte` |
| CRAP | `crap4rs` + `cargo-llvm-cov` | `crap4ts` |
| Mutation | `cargo-mutants` (local; CI gate planned) | — (TS mutation revisited when TS logic surface grows) |
| Architecture | Cargo workspace + `mod` visibility | `dependency-cruiser` + `eslint-plugin-boundaries` |

Phases 1–2 run during local development iterations. Phases 3 and 5 are enforced in CI on every PR. Phase 4 (mutation) is wired as a local `moon run shop:mutate` task today; promoting it to a CI gate is tracked under epic [#370](https://github.com/breezy-bays-labs/mokumo/issues/370).

## What CI enforces on every PR

When you open a pull request, these gates run:

- **Unit + integration tests** pass on all target platforms.
- **BDD scenarios** (`cargo test --test bdd --features bdd`) pass.
- **Clippy** clean with `-D warnings`.
- **Formatter** clean (`cargo fmt --check`).
- **CRAP** — no function above threshold. Refactor-heavy PRs opt into a stricter `ci:crap-delta` gate that catches sub-threshold regressions.
- **Architecture invariants** — the kikan/mokumo layering checks pass on main.
- **BDD staleness lint** — scenarios referencing renamed symbols are flagged.
- **Security** — dependency audit, dependency review, secret scanning.

## What we measure

Beyond pass/fail gates, we track metrics whose direction matters as much as the absolute value:

- Coverage (`cargo-llvm-cov`).
- Mutation kill rate (`cargo-mutants`, run locally today; CI integration planned under epic [#370](https://github.com/breezy-bays-labs/mokumo/issues/370)).
- CRAP count over threshold.
- Module size (longest function; count of modules over 300 lines — convention, not currently enforced).
- Architecture violations (must be zero).

## Threshold tuning

The sticky scorecard's per-row verdicts (Green / Yellow / Red) are produced by comparing measured deltas against thresholds. Operators tune those thresholds in [`.config/scorecard/quality.toml`](.config/scorecard/quality.toml) — no Rust source edits required.

Each row table in `quality.toml` declares a `warn_pp_delta` and a `fail_pp_delta`:

- A delta worse than `warn_pp_delta` flips the row to Yellow.
- A delta worse than `fail_pp_delta` flips it to Red.
- Thresholds are inclusive on the worse side, so a delta exactly at the threshold trips the corresponding status.

When `quality.toml` is absent or empty, the producer falls back to a hardcoded **starter-wheels** configuration: a sensible warn/fail pair that catches obvious regressions without the operator having to commit a config first. The sticky comment surfaces this state with a short italic note above the banner and an `<!-- fallback-thresholds:hardcoded -->` HTML marker after the row table — a reviewer can tell at a glance whether the verdict came from tuned thresholds or starter-wheels defaults.

A typo in `quality.toml` is a **loud failure**: the producer aborts with a non-zero exit and no scorecard artifact is written, so an operator never silently slides into fallback because of an unparseable config. The drift-check job additionally pipes the committed `quality.toml` through `ajv-cli` against the schema in `.config/scorecard/quality.config.schema.json` to catch shape-level mistakes before they reach the producer.

The committed `quality.toml`, the operator schema, and the wire schema all live under `.config/scorecard/` so an operator can see the entire surface in one directory listing. The schemas are generated from the Rust source — direct edits to either schema file fail CI on the next push.

### What V3 covers vs defers

V3 ships the threshold engine for the **coverage-delta** row variant only. Two follow-ups are intentionally deferred:

- **Absolute-coverage row variant.** A second row that compares absolute coverage against an absolute floor (rather than the delta against the base branch). Lands in V4 and inherits the same `[rows.<name>]` table shape.
- **Conditional gating between coverage-delta and absolute coverage.** When the absolute-coverage row variant lands, the warn/fail thresholds for `CoverageDelta` should apply only if absolute coverage passes its own threshold — a regression on a project that's already under its absolute floor should be reported as Red regardless of delta. This is tracked alongside V4 (see [mokumo#769](https://github.com/breezy-bays-labs/mokumo/issues/769)).

The Red branch of the threshold resolver is unit-tested in `crates/scorecard/src/threshold.rs::tests` against the fallback config; the BDD scenarios in `crates/scorecard/tests/features/scorecard_display.feature` assert the producer side of the Yellow + fallback-marker contract, and vitest snapshots in `.github/scripts/scorecard/__tests__/` lock the renderer's byte output for `STARTER_PREAMBLE`, `FALLBACK_MARKER`, and `PATH_HINT_COMMENT`. A drift on either side fails CI before merge.

### Vendored ajv refresh cadence

The drift-check job validates `quality.toml` via a vendored ajv bundle at `.github/scripts/scorecard/ajv-bundle.js`. Per the scorecard ADR, the bundle is refreshed on a quarterly cadence (Q1/Q2/Q3/Q4 calendar review) — see [`tools/update-vendored-ajv.sh`](tools/update-vendored-ajv.sh) for the regenerator script.

## What's planned

A living roadmap of quality tooling improvements, grouped by the question each answers:

**Are my tests testing real behavior?**
- `scrap4rs` — static no-op test detection. Rust equivalent of Uncle Bob's [`scrap`](https://github.com/unclebob/scrap) (originally for speclj). Catches tests with no assertions, tautological asserts, and surface-only I/O checks.
- `/review-feature` — agent skill that reviews `.feature` files for the "two developers would implement the same thing" quality bar.

**Are all my behaviors specified?**
- Public-API spec audit — reports which `pub` items are never exercised by any BDD scenario.
- Handler ↔ scenario map — for every axum route, which scenarios cover it.

**Is my code reachable and used?**
- Unused `pub` items lint — Rust-ecosystem gap; catches exports with zero inbound references from anywhere in the workspace.

**Is the system trending better or worse?**
- Metrics-PR comment bot — sticky PR comment with coverage, CRAP, mutation, module-size deltas vs. main.
- Mutation-per-scenario map — which scenarios are catching mutants, and which are effectively integration-level no-ops.

**Can I trust the quality signal itself?**
- Orphan-test lint — will flag any `[[test]]` harness not wired into CI. Tracked in [#648](https://github.com/breezy-bays-labs/mokumo/issues/648). Filed after we discovered a BDD suite silently failing on main for several sessions because no CI job ran it — see [#647](https://github.com/breezy-bays-labs/mokumo/issues/647) for the incident.

Full tracking: [Epic #370 — M0 Testing & Quality Infrastructure](https://github.com/breezy-bays-labs/mokumo/issues/370).

## Philosophy

Two principles inform how we use these tools:

1. **Measure, don't review.** AI-generated code is tested, measured, and mutation-scored the same as human code. CRAP, mutation score, module size, architecture violations — these are the review. Agent-written code that clears the bars is merged; agent-written code that doesn't is fixed until it does.

2. **Adversarial distance.** Where feasible, the agent that writes tests is distinct from the agent that makes them pass. This prevents the two from colluding (an agent tempted to weaken a test to pass a flaky implementation, or vice versa). The discipline is evolving; the goal is that every important gate has at least two independent processes defending it.

Both borrow directly from [Uncle Bob Martin's](https://x.com/unclebobmartin) recent AI-era quality writings.

## For contributors

If you're opening a PR:

- Write the `.feature` file first. Confirm the scenario fails against main. Then implement.
- Run the loop locally: `cargo nextest run`, `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo test --features bdd --test bdd`.
- CRAP over threshold? Refactor before opening the PR. If the refactor is the point of the PR, apply the `ci:crap-delta` label so the delta gate runs.
- Mutation survivors in new code? Write tests to kill them.
- Architecture violation in CI? Fix at the boundary, not by widening the exception list — the invariants encode load-bearing structural decisions.

The quality framework protects the shop owner running mokumo in their business, not the PR author. Optimize for their trust, not for the green checkmark.

## See also

- [ARCHITECTURE.md](./ARCHITECTURE.md) — system structure
- [CONTRIBUTING.md](./CONTRIBUTING.md) — how to propose changes
- [SECURITY.md](./SECURITY.md) — threat model and disclosure process
- [COVERAGE.md](./COVERAGE.md) — coverage instrumentation details
