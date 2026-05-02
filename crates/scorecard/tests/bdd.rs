//! Cucumber-rs runner for `tests/features/scorecard_display.feature`.
//!
//! The runner wires the [`ThresholdWorld`] state machine in
//! `tests/bdd_world/` to the Gherkin scenarios under `tests/features/`.
//! Step-defs live in `tests/bdd_world/threshold_steps.rs` so `bdd-lint`
//! (which globs `crates/*/tests/bdd_world/**/*.rs`) discovers them.
//!
//! Producer-only assertion surface: step-defs build [`Scorecard`] values
//! through `aggregate::build_scorecard` and assert on JSON state. The
//! renderer is asserted byte-for-byte by vitest snapshots in
//! `.github/scripts/scorecard/__tests__/render.test.js`. See the
//! Gherkin comment block above the fallback-marker scenario for the
//! split rationale.

use cucumber::World as _;

#[path = "bdd_world/mod.rs"]
mod bdd_world;

const SKIP_EXEMPT_TAGS: &[&str] = &["wip", "allow.skipped", "future"];

fn is_exempt(tags: &[String]) -> bool {
    tags.iter().any(|t| SKIP_EXEMPT_TAGS.contains(&t.as_str()))
}

#[tokio::main]
async fn main() {
    bdd_world::ThresholdWorld::cucumber()
        .fail_on_skipped_with(|feature, rule, scenario| {
            !is_exempt(&feature.tags)
                && rule.is_none_or(|r| !is_exempt(&r.tags))
                && !is_exempt(&scenario.tags)
        })
        .filter_run_and_exit("tests/features", |feature, _, sc| {
            let dominated_by_wip =
                feature.tags.iter().any(|t| t == "wip") || sc.tags.iter().any(|t| t == "wip");
            !dominated_by_wip
        })
        .await;
}
