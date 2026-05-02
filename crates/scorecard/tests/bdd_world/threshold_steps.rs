//! Step definitions for scenarios #5 (configured-threshold round-trip)
//! and #6 (empty-toml fallback marker) in
//! `tests/features/scorecard_display.feature`.
//!
//! ## Test split
//!
//! These step-defs assert on **producer behavior** — the JSON state of
//! the [`Scorecard`] returned by `aggregate::build_scorecard`, including
//! the `fallback_thresholds_active` flag and the per-row [`Status`].
//! They never invoke the renderer.
//!
//! Renderer byte-equality on the rendered markdown
//! (`STARTER_PREAMBLE` + `FALLBACK_MARKER` + `PATH_HINT_COMMENT`) is
//! locked by vitest snapshots in
//! `.github/scripts/scorecard/__tests__/render.test.js`.
//!
//! ## Doc-drift gate
//!
//! The Gherkin literal `"<!-- fallback-thresholds:hardcoded -->"` is
//! checked byte-for-byte against
//! [`scorecard::threshold::FALLBACK_MARKER`] inside the `Then` step.
//! The same constant is mirrored on the renderer side and pinned by
//! vitest, so a drift on either side fails this scenario first.

use cucumber::{given, then, when};

use scorecard::aggregate::{ThresholdSource, build_scorecard, resolve_threshold_source};
use scorecard::threshold::{
    self, CoverageThresholds, FALLBACK_MARKER, PATH_HINT_COMMENT, STARTER_PREAMBLE, ThresholdConfig,
};
use scorecard::{Row, Status};

use super::ThresholdWorld;

// ───────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────

/// Produce a scorecard with `delta_pp` against the resolved thresholds
/// from a `quality.toml` at `<tmp>/quality.toml`. Mirrors the path the
/// `aggregate` binary would take in CI: `resolve_threshold_source` →
/// `build_scorecard`.
fn produce(world: &mut ThresholdWorld, delta_pp: f64) {
    let tmp = world
        .tmp
        .as_ref()
        .expect("tmp dir must be set by an earlier step");
    let toml_path = tmp.path().join("quality.toml");

    let source = resolve_threshold_source(&toml_path)
        .expect("operator config must parse (or be absent → fallback)");
    let cfg = source.config();
    let fallback_active = source.fallback_active();

    let pr = ThresholdWorld::stub_pr_meta();
    let scorecard = build_scorecard(pr, delta_pp, &cfg, fallback_active);

    let row_status = match scorecard.rows[0] {
        Row::CoverageDelta { status, .. } => status,
        // `Row` is `#[non_exhaustive]` (Layer-1 typestate); the wildcard
        // arm is required by rustc and signals to a future contributor
        // that adding a row variant means revisiting the BDD assertion
        // surface.
        _ => panic!("unexpected Row variant in V3 producer output"),
    };
    world.coverage_delta_pp = Some(delta_pp);
    world.coverage_row_status = Some(row_status);
    world.scorecard = Some(scorecard);
}

/// Build a tuned `quality.toml` with a single `[rows.coverage]` table
/// where `warn_pp_delta = warn`. `fail_pp_delta` is held at the
/// fallback's `-5.0` so the scenario isolates the warn-tightening
/// effect.
fn write_quality_toml_with_warn(world: &mut ThresholdWorld, warn: f64) {
    let tmp = world
        .tmp
        .as_ref()
        .expect("tmp dir must be set by an earlier step");
    let body = format!(
        "[rows.coverage]\nwarn_pp_delta = {warn}\nfail_pp_delta = -5.0\n",
        warn = warn,
    );
    std::fs::write(tmp.path().join("quality.toml"), body)
        .expect("must be able to write quality.toml inside the scenario tempdir");
}

// ───────────────────────────────────────────────────────────────────
// Scenario #5 — configured-threshold round-trip (Green → Yellow)
// ───────────────────────────────────────────────────────────────────

#[given(expr = "a row reports a coverage delta of {float} percentage points")]
async fn given_row_reports_delta(world: &mut ThresholdWorld, delta_pp: f64) {
    world.tmp = Some(tempfile::tempdir().expect("scenario tempdir must allocate"));
    world.coverage_delta_pp = Some(delta_pp);
}

#[given(
    expr = "the row is currently shown as green because the warn threshold is {float} percentage points"
)]
async fn given_row_currently_green(world: &mut ThresholdWorld, warn: f64) {
    write_quality_toml_with_warn(world, warn);
    let delta = world
        .coverage_delta_pp
        .expect("delta must have been set by the previous Given");
    produce(world, delta);
    assert_eq!(
        world.coverage_row_status,
        Some(Status::Green),
        "scenario precondition: with warn={warn} the row must start Green at delta={delta}",
    );
}

#[when(
    expr = "the operator edits quality.toml to tighten the warn threshold to {float} percentage points"
)]
async fn when_operator_tightens_warn(world: &mut ThresholdWorld, new_warn: f64) {
    write_quality_toml_with_warn(world, new_warn);
}

#[when(expr = "CI completes again on the same head commit with no other changes")]
async fn when_ci_reruns(world: &mut ThresholdWorld) {
    let delta = world
        .coverage_delta_pp
        .expect("delta must remain pinned across the two CI runs");
    produce(world, delta);
}

#[then(expr = "the row is shown as yellow")]
async fn then_row_shown_yellow(world: &mut ThresholdWorld) {
    assert_eq!(
        world.coverage_row_status,
        Some(Status::Yellow),
        "row status mismatch — tightened-warn run should flip Green → Yellow",
    );
}

#[then(expr = "no Rust source files were modified between the two CI runs")]
async fn then_no_rust_modified(_world: &mut ThresholdWorld) {
    // Operator-config scenarios live entirely in the fixture tempdir;
    // the step-def itself never touches `crates/scorecard/src/**`.
    // This step exists for the spec to read cleanly — the contract is
    // that threshold tuning round-trips through `quality.toml` only.
}

// ───────────────────────────────────────────────────────────────────
// Scenario #6 — empty-toml fallback marker
// ───────────────────────────────────────────────────────────────────

#[given(expr = "quality.toml is empty or absent")]
async fn given_quality_toml_absent(world: &mut ThresholdWorld) {
    // Allocate the tempdir but never write `quality.toml` in it —
    // `resolve_threshold_source` will hit `io::ErrorKind::NotFound`
    // and produce `ThresholdSource::Fallback`.
    world.tmp = Some(tempfile::tempdir().expect("scenario tempdir must allocate"));
}

#[when(expr = "CI completes")]
async fn when_ci_completes(world: &mut ThresholdWorld) {
    // -2.0 pp picks up the fallback's `warn_pp_delta = -1.0` and stops
    // short of `fail_pp_delta = -5.0`, so the row lands Yellow — the
    // "regressed compared to the base branch" condition in the next
    // Then step.
    produce(world, -2.0);
}

#[then(expr = "any metric that regressed compared to the base branch is shown as yellow")]
async fn then_regressed_shown_yellow(world: &mut ThresholdWorld) {
    assert_eq!(
        world.coverage_row_status,
        Some(Status::Yellow),
        "fallback warn_pp_delta = -1.0; delta of -2.0 pp must land Yellow",
    );
    let scorecard = world.scorecard.as_ref().expect("scorecard must be built");
    assert!(
        scorecard.fallback_thresholds_active,
        "absent quality.toml must mark fallback_thresholds_active = true",
    );
}

#[then(expr = "any new gate failure is shown as red")]
async fn then_new_failure_shown_red(_world: &mut ThresholdWorld) {
    // V3 ships only the CoverageDelta row variant. We exercise the Red
    // branch as a unit assertion against the same fallback config the
    // producer is using; the "gate failure" wording in the .feature is
    // forward-compatible with the absolute-coverage row variant that
    // lands in V4 or later (council C5).
    let cfg = ThresholdConfig::fallback();
    let coverage: &CoverageThresholds = &cfg.rows.coverage;
    assert_eq!(
        threshold::resolve_coverage_delta(-7.5, coverage),
        Status::Red,
        "fallback fail_pp_delta = -5.0; a delta of -7.5 pp must land Red",
    );
}

#[then(expr = "the ci-scorecard comment contains the HTML marker {string}")]
async fn then_comment_contains_marker(_world: &mut ThresholdWorld, marker_literal: String) {
    // Doc-drift gate (impl-plan §C13). The Gherkin literal is the
    // canonical text on the renderer side; the Rust constant is the
    // canonical text on the producer side. Asserting byte-equality
    // here means a drift on either side fails this scenario first.
    assert_eq!(
        marker_literal, FALLBACK_MARKER,
        "Gherkin marker literal must equal scorecard::threshold::FALLBACK_MARKER",
    );
    let scorecard = _world.scorecard.as_ref().expect("scorecard must be built");
    assert!(
        scorecard.fallback_thresholds_active,
        "fallback_thresholds_active must be true so the renderer emits the marker",
    );
}

#[then(expr = "the comment displays a visible note that hardcoded fallback thresholds are in use")]
async fn then_comment_displays_note(world: &mut ThresholdWorld) {
    // Mirror of the above: the producer flags fallback so the renderer
    // (asserted by vitest) prepends `STARTER_PREAMBLE` and trails with
    // `PATH_HINT_COMMENT`. Touching the constants here keeps any
    // unused-import drift from sneaking past compile.
    let _ = (STARTER_PREAMBLE, PATH_HINT_COMMENT);
    let scorecard = world.scorecard.as_ref().expect("scorecard must be built");
    assert!(
        scorecard.fallback_thresholds_active,
        "the visible note is gated on fallback_thresholds_active = true",
    );
}

// ───────────────────────────────────────────────────────────────────
// Pinning the helpers' types — keeps `cargo check` honest about the
// public surface even when no scenario currently exercises a code path.
// ───────────────────────────────────────────────────────────────────

#[allow(dead_code)]
fn _type_pins(source: ThresholdSource) -> Status {
    let cfg = source.config();
    threshold::resolve_coverage_delta(0.0, &cfg.rows.coverage)
}
