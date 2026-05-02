//! `ThresholdWorld` — fixture state for the V3 threshold-engine BDD
//! scenarios. Owned exclusively by `tests/bdd.rs`; the layout matches
//! kikan/kikan-events so `bdd-lint`'s rust-glob
//! (`crates/*/tests/bdd_world/**/*.rs`) discovers the step-defs.

use cucumber::World;
use scorecard::{PrMeta, Scorecard, Status};

pub mod threshold_steps;

/// Scenario fixture for the V3 threshold-engine flow.
///
/// Each scenario edits a temp `quality.toml`, runs the producer in
/// process, and asserts on the resulting [`Scorecard`]. The fields are
/// `Option`-typed so a step-def can fail loudly when an earlier step
/// did not populate the slot it depends on.
#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct ThresholdWorld {
    /// Working directory for the operator config — dropped at the end
    /// of the scenario.
    pub tmp: Option<tempfile::TempDir>,
    /// Coverage delta in percentage points (the producer's
    /// `--coverage-delta-pp` input). Set by Given/When steps.
    pub coverage_delta_pp: Option<f64>,
    /// Most recently produced scorecard. Set by the When step that
    /// runs the producer; asserted on by Then steps.
    pub scorecard: Option<Scorecard>,
    /// Status of the row at index 0 (coverage) on the most recent
    /// produced scorecard. Cached out of [`Self::scorecard`] for
    /// terser Then steps.
    pub coverage_row_status: Option<Status>,
}

impl ThresholdWorld {
    fn new() -> Self {
        Self {
            tmp: None,
            coverage_delta_pp: None,
            scorecard: None,
            coverage_row_status: None,
        }
    }

    /// Synthetic PR metadata. The scorecard's PR fields are not under
    /// test in V3 — only the row + fallback flag are — so a fixed stub
    /// is fine.
    pub fn stub_pr_meta() -> PrMeta {
        PrMeta {
            pr_number: 768.into(),
            head_sha: "0000000000000000000000000000000000000000".into(),
            base_sha: "1111111111111111111111111111111111111111".into(),
            is_fork: false,
        }
    }
}
