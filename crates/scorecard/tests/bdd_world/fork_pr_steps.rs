//! Step definitions for the fork-PR Check Run linkage scenario in
//! `tests/features/scorecard_display.feature`.
//!
//! These step definitions assert the producer-side wire-shape invariant
//! the renderer relies on: the scorecard envelope's `all_check_runs_url`
//! is built from `pr.head_sha`, so a fork-PR payload (where `head_sha`
//! is the fork's HEAD, not the base branch) flows through with that SHA
//! intact.
//!
//! Renderer-side assertions on the rendered markdown — clickable status
//! indicators, fork-SHA-bearing URLs, base-SHA never appearing — live
//! in vitest at `.github/scripts/scorecard/__tests__/render.test.js`.

use cucumber::{given, then, when};

use scorecard::aggregate::{BddSummary, FlakyCorpus, build_scorecard};
use scorecard::threshold::ThresholdConfig;

use super::ThresholdWorld;

#[given("a pull request opened from a fork")]
async fn given_pull_request_from_fork(world: &mut ThresholdWorld) {
    let pr = ThresholdWorld::stub_fork_pr_meta();
    let cfg = ThresholdConfig::fallback();
    let scorecard = build_scorecard(
        pr,
        0.0,
        &BddSummary::default(),
        None,
        &FlakyCorpus::default(),
        None,
        &cfg,
        true,
    );
    world.scorecard = Some(scorecard);
}

#[when("CI completes and the ci-scorecard comment is posted")]
async fn when_ci_completes_and_comment_posted(_world: &mut ThresholdWorld) {
    // The Given step ran the producer in-process; this When is the
    // narrative seam the Gherkin scenario reads from. No additional
    // work — the scorecard is already in `world.scorecard`.
}

#[then("each per-gate Check Run link in the comment resolves against the fork's head commit")]
async fn then_per_gate_links_resolve_to_fork_head(world: &mut ThresholdWorld) {
    let scorecard = world
        .scorecard
        .as_ref()
        .expect("scorecard must be produced by the Given step");
    let head_sha = scorecard.pr.head_sha.clone();
    assert!(
        scorecard.all_check_runs_url.contains(&head_sha),
        "all_check_runs_url ({}) must reference the fork's head SHA ({})",
        scorecard.all_check_runs_url,
        head_sha,
    );
    let base_sha = scorecard.pr.base_sha.clone();
    assert!(
        !scorecard.all_check_runs_url.contains(&base_sha),
        "all_check_runs_url ({}) must NOT reference the base SHA ({})",
        scorecard.all_check_runs_url,
        base_sha,
    );
}

#[then("the developer can navigate from the comment directly to each gate's Check Run page")]
async fn then_navigation_affordance(world: &mut ThresholdWorld) {
    // Producer-side guarantee for the navigation affordance: the
    // envelope's `all_check_runs_url` is an absolute https:// URL.
    // The renderer wraps each row's status icon with a markdown link
    // (the two-click rule); that wrap is locked by vitest.
    let scorecard = world.scorecard.as_ref().expect("scorecard set by Given");
    assert!(
        scorecard.all_check_runs_url.starts_with("https://"),
        "all_check_runs_url must be an absolute https:// URL, got: {}",
        scorecard.all_check_runs_url,
    );
}
