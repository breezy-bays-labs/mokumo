//! Threshold resolution — producer-side mapping from raw measurements to
//! [`Status`](crate::Status) (`Green` / `Yellow` / `Red`).
//!
//! The module owns the operator-tunable surface that turns a raw
//! measurement (e.g. coverage delta in percentage points) into the
//! status icon the renderer paints on the sticky comment. Resolution
//! lives in the producer (Rust) so the renderer stays a dumb formatter
//! — see `decisions/mokumo/adr-scorecard-crate-shape.md` §Threshold
//! resolution lives in the producer for the architectural rationale.
//!
//! # Dependencies
//!
//! `serde + schemars + serde_json` only. The TOML parser sits behind
//! the `cli` feature so a downstream `cargo add scorecard` does not
//! pull `toml` transitively, and `cargo check -p scorecard
//! --no-default-features` keeps compiling [`ThresholdConfig`] +
//! [`schemars::schema_for!`] without a TOML dependency anywhere in the
//! transitive tree.
//!
//! # In-crate Layer-1 discipline
//!
//! `Row` and each variant are `#[non_exhaustive]`, blocking external
//! crates from struct-literal construction or non-wildcard pattern
//! matches. Inside `scorecard` both remain reachable: the rule that
//! rows are constructed via the `Row::coverage_delta_{green,yellow,red}`
//! constructors only is convention, enforced by code review. New row
//! variants gain a sibling free function in this module (one
//! `resolve_*` per `Row` variant); resolver dispatch is by call site,
//! not by trait. See the ADR for the dispatch convention rationale.
//!
//! # Doc-drift markers
//!
//! [`FALLBACK_MARKER`], [`STARTER_PREAMBLE`], and [`PATH_HINT_COMMENT`]
//! are the three byte-stable strings the renderer emits when the
//! producer ran without an operator config. They are pinned by vitest
//! snapshots so any drift between the renderer's emitted bytes and
//! these constants surfaces as a snapshot diff on PR review.

use crate::Status;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// HTML comment marker the renderer emits when the producer ran with
/// hardcoded fallback thresholds (no operator `quality.toml` was found
/// or it was empty). The renderer's fail-closed branch keys off this
/// constant so a single source of truth governs the marker bytes.
pub const FALLBACK_MARKER: &str = "<!-- fallback-thresholds:hardcoded -->";

/// Italic preamble the renderer prepends to the comment body when
/// fallback thresholds are active. The phrase "starter-wheels" is
/// deliberate: the operator can replace the defaults at any time by
/// writing a `quality.toml`, and the language signals provisional /
/// tunable rather than authoritative.
pub const STARTER_PREAMBLE: &str = "_Using starter-wheels fallback thresholds. Tune them in [`quality.toml`](QUALITY.md#threshold-tuning)._";

/// HTML comment the renderer emits immediately after [`FALLBACK_MARKER`]
/// pointing operators at the config path. Plain comment so it does not
/// render visibly in the body — the visible affordance is
/// [`STARTER_PREAMBLE`].
pub const PATH_HINT_COMMENT: &str =
    "<!-- tune at .config/scorecard/quality.toml — see QUALITY.md#threshold-tuning -->";

/// Operator-tunable threshold configuration.
///
/// Top-level shape of `.config/scorecard/quality.toml`. Generates the
/// operator-facing JSON Schema via `schemars::schema_for!` (no `cli`
/// feature required), so a downstream `cargo build -p scorecard
/// --no-default-features --bin emit-schema` succeeds. The JSON Schema
/// derives drive the `quality.config.schema.json` artifact that ajv
/// validates the committed example against.
///
/// `deny_unknown_fields` so a typo in the operator's TOML
/// (`warn_pp_dleta` etc.) fails the parse rather than being silently
/// dropped.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThresholdConfig {
    /// Per-row threshold tables. Currently a single `coverage` row;
    /// new row variants add sibling fields here in lockstep with the
    /// matching `resolve_*` free function in this module.
    pub rows: RowsConfig,
}

/// Per-row threshold tables. One field per `Row` variant kind.
///
/// Adding a new row variant in `lib.rs` is paired with a new field
/// here and a sibling `resolve_*` free function below — the four
/// changes land together in the same commit.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RowsConfig {
    /// Thresholds for the `Row::CoverageDelta` variant.
    pub coverage: CoverageThresholds,
    /// Thresholds for the `Row::BddFeatureLevelSkipped` variant.
    #[serde(default = "BddFeatureSkipThresholds::default")]
    pub bdd_feature_skip: BddFeatureSkipThresholds,
    /// Thresholds for the `Row::BddScenarioLevelSkipped` variant.
    #[serde(default = "BddScenarioSkipThresholds::default")]
    pub bdd_scenario_skip: BddScenarioSkipThresholds,
    /// Thresholds for the `Row::CiWallClockDelta` variant.
    #[serde(default = "CiWallClockThresholds::default")]
    pub ci_wall_clock: CiWallClockThresholds,
    /// Thresholds for the `Row::FlakyPopulation` variant.
    #[serde(default = "FlakyPopulationThresholds::default")]
    pub flaky: FlakyPopulationThresholds,
}

/// Warn / fail thresholds for the `Row::BddFeatureLevelSkipped`
/// variant. Both fields are unsigned integer feature-file counts;
/// threshold semantics are inclusive on the worse side.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BddFeatureSkipThresholds {
    /// Skipped-feature count above (or equal) which a row reports
    /// [`Status::Yellow`].
    pub warn_skipped_features: u32,
    /// Skipped-feature count above (or equal) which a row reports
    /// [`Status::Red`]. Typically larger than `warn_skipped_features`.
    pub fail_skipped_features: u32,
}

impl Default for BddFeatureSkipThresholds {
    /// Defensible fallback: 10 WIP feature files trigger Yellow, 20
    /// trigger Red. Counts whole files (not scenarios), so the bar
    /// sits on backlog growth rather than scenario churn — operators
    /// tune via `quality.toml`.
    fn default() -> Self {
        Self {
            warn_skipped_features: 10,
            fail_skipped_features: 20,
        }
    }
}

/// Warn / fail thresholds for the `Row::BddScenarioLevelSkipped`
/// variant. Both fields are unsigned integer scenario counts; threshold
/// semantics are inclusive on the worse side.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BddScenarioSkipThresholds {
    /// Skipped-scenario count above (or equal) which a row reports
    /// [`Status::Yellow`].
    pub warn_skipped_scenarios: u32,
    /// Skipped-scenario count above (or equal) which a row reports
    /// [`Status::Red`].
    pub fail_skipped_scenarios: u32,
}

impl Default for BddScenarioSkipThresholds {
    /// Defensible fallback: 40 scenario-level skips trigger Yellow, 60
    /// trigger Red. Operators tune via `quality.toml`. The bdd-lint
    /// `--max-dead-specs` ratchet evolves separately; coupling the two
    /// would send them in opposite directions whenever either is tuned.
    fn default() -> Self {
        Self {
            warn_skipped_scenarios: 40,
            fail_skipped_scenarios: 60,
        }
    }
}

/// Warn / fail thresholds for the `Row::CiWallClockDelta` variant,
/// expressed in seconds of total-CI-wall-clock delta vs base.
///
/// Both fields are signed: a slowdown is a positive delta, so warn /
/// fail thresholds are themselves positive numbers. Inclusive boundary
/// semantics: `delta_seconds == warn_seconds_delta` resolves
/// [`Status::Yellow`]; `delta_seconds == fail_seconds_delta` resolves
/// [`Status::Red`]. See [`resolve_ci_wall_clock`] for the full table.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CiWallClockThresholds {
    /// Threshold above (or equal) which a row reports
    /// [`Status::Yellow`]. Typically positive (slowdown).
    pub warn_seconds_delta: f64,
    /// Threshold above (or equal) which a row reports [`Status::Red`].
    /// Typically positive and larger than `warn_seconds_delta`.
    pub fail_seconds_delta: f64,
}

impl Default for CiWallClockThresholds {
    /// Defensible fallback: a 60s slowdown trips Yellow, 300s trips Red.
    /// Tuned to be permissive enough that ordinary CI noise doesn't
    /// flap the verdict; operators tighten via `quality.toml`.
    fn default() -> Self {
        Self {
            warn_seconds_delta: 60.0,
            fail_seconds_delta: 300.0,
        }
    }
}

/// Warn / fail thresholds for the `Row::FlakyPopulation` variant.
///
/// `marker_count` is the integer count of `// FLAKY:` markers
/// across the operator-supplied source roots. Threshold semantics
/// are inclusive on the worse side.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlakyPopulationThresholds {
    /// Marker count above (or equal) which a row reports
    /// [`Status::Yellow`].
    pub warn_marker_count: u32,
    /// Marker count above (or equal) which a row reports
    /// [`Status::Red`]. Typically larger than `warn_marker_count`.
    pub fail_marker_count: u32,
}

impl Default for FlakyPopulationThresholds {
    /// Defensible fallback: 5 markers trips Yellow, 20 trips Red.
    /// Tightening these is the natural way operators discourage flaky-
    /// test accumulation; the fallback is permissive enough that an
    /// established codebase doesn't go red on day one.
    fn default() -> Self {
        Self {
            warn_marker_count: 5,
            fail_marker_count: 20,
        }
    }
}

/// Warn / fail thresholds for the `Row::CoverageDelta` variant,
/// expressed in percentage points (pp) of coverage delta vs base.
///
/// Both fields are signed: a regression is a negative delta, so warn
/// and fail thresholds for "drops" are themselves negative numbers.
/// Inclusive boundary semantics: `delta_pp == warn_pp_delta` resolves
/// to [`Status::Yellow`]; `delta_pp == fail_pp_delta` resolves to
/// [`Status::Red`]. See [`resolve_coverage_delta`] for the full table.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoverageThresholds {
    /// Threshold below (or equal) which a row reports
    /// [`Status::Yellow`]. Typically negative.
    pub warn_pp_delta: f64,
    /// Threshold below (or equal) which a row reports [`Status::Red`].
    /// Typically negative and more negative than `warn_pp_delta`.
    pub fail_pp_delta: f64,
}

impl ThresholdConfig {
    /// Defensible hardcoded fallback thresholds.
    ///
    /// Drops < 1 pp resolve [`Status::Green`] (within the noise floor
    /// for typical coverage instrumentation), drops ∈ [1, 5) pp
    /// resolve [`Status::Yellow`] (visible regression worth a soft
    /// signal), drops ≥ 5 pp resolve [`Status::Red`] (hard failure).
    /// Used when no operator `quality.toml` is present or readable.
    pub fn fallback() -> Self {
        Self {
            rows: RowsConfig {
                coverage: CoverageThresholds {
                    warn_pp_delta: -1.0,
                    fail_pp_delta: -5.0,
                },
                bdd_feature_skip: BddFeatureSkipThresholds::default(),
                bdd_scenario_skip: BddScenarioSkipThresholds::default(),
                ci_wall_clock: CiWallClockThresholds::default(),
                flaky: FlakyPopulationThresholds::default(),
            },
        }
    }
}

/// Resolve a coverage delta (in percentage points) to a [`Status`]
/// using the supplied [`CoverageThresholds`].
///
/// # Boundary semantics
///
/// | `delta_pp`                         | Result            |
/// |------------------------------------|-------------------|
/// | `delta_pp <= fail_pp_delta`        | [`Status::Red`]    |
/// | `fail_pp_delta < delta_pp <= warn_pp_delta` | [`Status::Yellow`] |
/// | `warn_pp_delta < delta_pp`         | [`Status::Green`]  |
///
/// Both threshold boundaries are inclusive on the worse side: a delta
/// exactly equal to `warn_pp_delta` is [`Status::Yellow`]; a delta
/// exactly equal to `fail_pp_delta` is [`Status::Red`]. This matches
/// operator intent ("fail at -5 pp" means -5 pp itself is failing).
///
/// # NaN handling
///
/// `NaN` participates in no IEEE 754 ordered comparison: `NaN <= x`
/// is always `false`. A `NaN` delta therefore falls through both
/// `<=` checks and resolves [`Status::Green`]. This is documented
/// rather than fixed because the producer constructs `delta_pp` from
/// a numeric CLI flag; `NaN` is not a value clap will yield. If a
/// future delta source can produce `NaN`, the caller is responsible
/// for rejecting it before calling this function.
pub fn resolve_coverage_delta(delta_pp: f64, cfg: &CoverageThresholds) -> Status {
    if delta_pp <= cfg.fail_pp_delta {
        Status::Red
    } else if delta_pp <= cfg.warn_pp_delta {
        Status::Yellow
    } else {
        Status::Green
    }
}

/// Resolve a BDD skipped-feature-file count to a [`Status`] using the
/// supplied [`BddFeatureSkipThresholds`].
///
/// Boundaries are inclusive on the worse side: a count exactly equal
/// to `warn_skipped_features` is [`Status::Yellow`]; a count exactly
/// equal to `fail_skipped_features` is [`Status::Red`].
pub fn resolve_bdd_feature_skip(skipped_features: u32, cfg: &BddFeatureSkipThresholds) -> Status {
    if skipped_features >= cfg.fail_skipped_features {
        Status::Red
    } else if skipped_features >= cfg.warn_skipped_features {
        Status::Yellow
    } else {
        Status::Green
    }
}

/// Resolve a BDD scenario-level skipped count to a [`Status`] using
/// the supplied [`BddScenarioSkipThresholds`].
///
/// Boundaries are inclusive on the worse side: a count exactly equal
/// to `warn_skipped_scenarios` is [`Status::Yellow`]; a count exactly
/// equal to `fail_skipped_scenarios` is [`Status::Red`].
pub fn resolve_bdd_scenario_skip(
    skipped_scenarios: u32,
    cfg: &BddScenarioSkipThresholds,
) -> Status {
    if skipped_scenarios >= cfg.fail_skipped_scenarios {
        Status::Red
    } else if skipped_scenarios >= cfg.warn_skipped_scenarios {
        Status::Yellow
    } else {
        Status::Green
    }
}

/// Resolve a CI wall-clock delta (in seconds) to a [`Status`] using the
/// supplied [`CiWallClockThresholds`].
///
/// # Boundary semantics
///
/// | `delta_seconds`                                        | Result            |
/// |--------------------------------------------------------|-------------------|
/// | `delta_seconds >= fail_seconds_delta`                  | [`Status::Red`]    |
/// | `warn_seconds_delta <= delta_seconds < fail_seconds_delta` | [`Status::Yellow`] |
/// | `delta_seconds < warn_seconds_delta`                   | [`Status::Green`]  |
///
/// Boundaries are inclusive on the worse (positive) side. A negative
/// delta (CI sped up) resolves [`Status::Green`]. NaN handling mirrors
/// `resolve_coverage_delta`: NaN compares false against everything and
/// resolves Green; the producer rejects NaN at the input boundary.
pub fn resolve_ci_wall_clock(delta_seconds: f64, cfg: &CiWallClockThresholds) -> Status {
    if delta_seconds >= cfg.fail_seconds_delta {
        Status::Red
    } else if delta_seconds >= cfg.warn_seconds_delta {
        Status::Yellow
    } else {
        Status::Green
    }
}

/// Resolve a flaky-marker count to a [`Status`] using the supplied
/// [`FlakyPopulationThresholds`].
///
/// # Boundary semantics
///
/// | `marker_count`                                       | Result            |
/// |------------------------------------------------------|-------------------|
/// | `marker_count >= fail_marker_count`                  | [`Status::Red`]    |
/// | `warn_marker_count <= marker_count < fail_marker_count` | [`Status::Yellow`] |
/// | `marker_count < warn_marker_count`                   | [`Status::Green`]  |
///
/// Boundaries are inclusive on the worse side: a count exactly equal
/// to `warn_marker_count` is [`Status::Yellow`]; a count exactly equal
/// to `fail_marker_count` is [`Status::Red`].
pub fn resolve_flaky_population(marker_count: u32, cfg: &FlakyPopulationThresholds) -> Status {
    if marker_count >= cfg.fail_marker_count {
        Status::Red
    } else if marker_count >= cfg.warn_marker_count {
        Status::Yellow
    } else {
        Status::Green
    }
}

/// Parse an operator `quality.toml` document into a [`ThresholdConfig`].
///
/// Behind `cli` feature so the lib stays deps-zero. The drift workflow
/// runs `cargo run -p scorecard --bin emit-schema` (no `cli` needed)
/// for schema regeneration; the aggregate binary runs with
/// `--features cli` and pulls `toml` transitively only there.
#[cfg(feature = "cli")]
pub fn parse_quality_toml(input: &str) -> Result<ThresholdConfig, toml::de::Error> {
    toml::from_str(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fallback_coverage() -> CoverageThresholds {
        ThresholdConfig::fallback().rows.coverage
    }

    // ── Boundary table per CLAUDE.md item 16 ─────────────────────────

    #[test]
    fn fallback_thresholds_match_documented_values() {
        let cfg = fallback_coverage();
        assert_eq!(cfg.warn_pp_delta, -1.0);
        assert_eq!(cfg.fail_pp_delta, -5.0);
    }

    #[test]
    fn delta_at_warn_threshold_resolves_yellow() {
        // Inclusive boundary on the warn side: -1.0 == warn → Yellow.
        assert_eq!(
            resolve_coverage_delta(-1.0, &fallback_coverage()),
            Status::Yellow
        );
    }

    #[test]
    fn delta_at_fail_threshold_resolves_red() {
        // Inclusive boundary on the fail side: -5.0 == fail → Red.
        assert_eq!(
            resolve_coverage_delta(-5.0, &fallback_coverage()),
            Status::Red
        );
    }

    #[test]
    fn delta_just_above_warn_resolves_green() {
        // The "almost wrong" case (CLAUDE.md item 16): a regression
        // smaller than the warn threshold is still Green.
        assert_eq!(
            resolve_coverage_delta(-0.999, &fallback_coverage()),
            Status::Green
        );
    }

    #[test]
    fn delta_between_warn_and_fail_resolves_yellow() {
        assert_eq!(
            resolve_coverage_delta(-2.5, &fallback_coverage()),
            Status::Yellow
        );
    }

    #[test]
    fn delta_just_above_fail_resolves_yellow() {
        // Fail boundary is inclusive on the fail side; immediately
        // less negative than fail must still be Yellow, not Red.
        assert_eq!(
            resolve_coverage_delta(-4.999, &fallback_coverage()),
            Status::Yellow
        );
    }

    #[test]
    fn synthetic_red_below_fail_threshold_resolves_red() {
        // Documents the Red branch behavior with a synthetic delta that
        // falls below `fail_pp_delta`. The coverage-delta row is the
        // only row variant that exercises the Red branch today, so a
        // unit test here is the canonical assertion until other row
        // variants land their own resolvers.
        assert_eq!(
            resolve_coverage_delta(-7.5, &fallback_coverage()),
            Status::Red
        );
    }

    #[test]
    fn zero_delta_resolves_green() {
        assert_eq!(
            resolve_coverage_delta(0.0, &fallback_coverage()),
            Status::Green
        );
    }

    #[test]
    fn positive_delta_resolves_green() {
        assert_eq!(
            resolve_coverage_delta(2.5, &fallback_coverage()),
            Status::Green
        );
    }

    #[test]
    fn nan_delta_resolves_green_documented_behavior() {
        // IEEE 754: NaN compares false against everything, including
        // itself. Both `<=` checks fall through, landing in Green.
        // This is documented behavior, not a feature: the producer
        // constructs `delta_pp` from a clap-parsed numeric flag, which
        // never yields NaN.
        assert_eq!(
            resolve_coverage_delta(f64::NAN, &fallback_coverage()),
            Status::Green
        );
    }

    #[test]
    fn negative_infinity_delta_resolves_red() {
        // Sanity check: extreme regression is unambiguously Red.
        assert_eq!(
            resolve_coverage_delta(f64::NEG_INFINITY, &fallback_coverage()),
            Status::Red
        );
    }

    #[test]
    fn positive_infinity_delta_resolves_green() {
        assert_eq!(
            resolve_coverage_delta(f64::INFINITY, &fallback_coverage()),
            Status::Green
        );
    }

    // ── BDD feature-skip resolver boundary table ─────────────────────

    fn fallback_bdd_feature_skip() -> BddFeatureSkipThresholds {
        ThresholdConfig::fallback().rows.bdd_feature_skip
    }

    #[test]
    fn bdd_feature_skip_fallback_values_match_documented_defaults() {
        let cfg = fallback_bdd_feature_skip();
        assert_eq!(cfg.warn_skipped_features, 10);
        assert_eq!(cfg.fail_skipped_features, 20);
    }

    #[test]
    fn bdd_feature_skip_zero_resolves_green() {
        assert_eq!(
            resolve_bdd_feature_skip(0, &fallback_bdd_feature_skip()),
            Status::Green
        );
    }

    #[test]
    fn bdd_feature_skip_just_below_warn_resolves_green() {
        assert_eq!(
            resolve_bdd_feature_skip(9, &fallback_bdd_feature_skip()),
            Status::Green
        );
    }

    #[test]
    fn bdd_feature_skip_at_warn_threshold_resolves_yellow() {
        assert_eq!(
            resolve_bdd_feature_skip(10, &fallback_bdd_feature_skip()),
            Status::Yellow
        );
    }

    #[test]
    fn bdd_feature_skip_just_below_fail_resolves_yellow() {
        assert_eq!(
            resolve_bdd_feature_skip(19, &fallback_bdd_feature_skip()),
            Status::Yellow
        );
    }

    #[test]
    fn bdd_feature_skip_at_fail_threshold_resolves_red() {
        assert_eq!(
            resolve_bdd_feature_skip(20, &fallback_bdd_feature_skip()),
            Status::Red
        );
    }

    // ── BDD scenario-skip resolver boundary table ────────────────────

    fn fallback_bdd_scenario_skip() -> BddScenarioSkipThresholds {
        ThresholdConfig::fallback().rows.bdd_scenario_skip
    }

    #[test]
    fn bdd_scenario_skip_fallback_values_match_documented_defaults() {
        let cfg = fallback_bdd_scenario_skip();
        assert_eq!(cfg.warn_skipped_scenarios, 40);
        assert_eq!(cfg.fail_skipped_scenarios, 60);
    }

    #[test]
    fn bdd_scenario_skip_zero_resolves_green() {
        assert_eq!(
            resolve_bdd_scenario_skip(0, &fallback_bdd_scenario_skip()),
            Status::Green
        );
    }

    #[test]
    fn bdd_scenario_skip_just_below_warn_resolves_green() {
        assert_eq!(
            resolve_bdd_scenario_skip(39, &fallback_bdd_scenario_skip()),
            Status::Green
        );
    }

    #[test]
    fn bdd_scenario_skip_at_warn_threshold_resolves_yellow() {
        assert_eq!(
            resolve_bdd_scenario_skip(40, &fallback_bdd_scenario_skip()),
            Status::Yellow
        );
    }

    #[test]
    fn bdd_scenario_skip_just_below_fail_resolves_yellow() {
        assert_eq!(
            resolve_bdd_scenario_skip(59, &fallback_bdd_scenario_skip()),
            Status::Yellow
        );
    }

    #[test]
    fn bdd_scenario_skip_at_fail_threshold_resolves_red() {
        assert_eq!(
            resolve_bdd_scenario_skip(60, &fallback_bdd_scenario_skip()),
            Status::Red
        );
    }

    // ── CI wall-clock resolver boundary table ────────────────────────

    fn fallback_ci_wall_clock() -> CiWallClockThresholds {
        ThresholdConfig::fallback().rows.ci_wall_clock
    }

    #[test]
    fn ci_wall_clock_fallback_values_match_documented_defaults() {
        let cfg = fallback_ci_wall_clock();
        assert_eq!(cfg.warn_seconds_delta, 60.0);
        assert_eq!(cfg.fail_seconds_delta, 300.0);
    }

    #[test]
    fn ci_wall_clock_negative_delta_resolves_green() {
        // CI sped up — Green unconditionally.
        assert_eq!(
            resolve_ci_wall_clock(-30.0, &fallback_ci_wall_clock()),
            Status::Green
        );
    }

    #[test]
    fn ci_wall_clock_zero_delta_resolves_green() {
        assert_eq!(
            resolve_ci_wall_clock(0.0, &fallback_ci_wall_clock()),
            Status::Green
        );
    }

    #[test]
    fn ci_wall_clock_just_below_warn_resolves_green() {
        // CLAUDE.md item 16 — the "almost wrong" case.
        assert_eq!(
            resolve_ci_wall_clock(59.9, &fallback_ci_wall_clock()),
            Status::Green
        );
    }

    #[test]
    fn ci_wall_clock_at_warn_threshold_resolves_yellow() {
        assert_eq!(
            resolve_ci_wall_clock(60.0, &fallback_ci_wall_clock()),
            Status::Yellow
        );
    }

    #[test]
    fn ci_wall_clock_just_below_fail_resolves_yellow() {
        assert_eq!(
            resolve_ci_wall_clock(299.9, &fallback_ci_wall_clock()),
            Status::Yellow
        );
    }

    #[test]
    fn ci_wall_clock_at_fail_threshold_resolves_red() {
        assert_eq!(
            resolve_ci_wall_clock(300.0, &fallback_ci_wall_clock()),
            Status::Red
        );
    }

    #[test]
    fn ci_wall_clock_above_fail_threshold_resolves_red() {
        assert_eq!(
            resolve_ci_wall_clock(900.0, &fallback_ci_wall_clock()),
            Status::Red
        );
    }

    // ── Flaky-population resolver boundary table ─────────────────────

    fn fallback_flaky() -> FlakyPopulationThresholds {
        ThresholdConfig::fallback().rows.flaky
    }

    #[test]
    fn flaky_fallback_values_match_documented_defaults() {
        let cfg = fallback_flaky();
        assert_eq!(cfg.warn_marker_count, 5);
        assert_eq!(cfg.fail_marker_count, 20);
    }

    #[test]
    fn flaky_zero_resolves_green() {
        assert_eq!(
            resolve_flaky_population(0, &fallback_flaky()),
            Status::Green
        );
    }

    #[test]
    fn flaky_just_below_warn_resolves_green() {
        assert_eq!(
            resolve_flaky_population(4, &fallback_flaky()),
            Status::Green
        );
    }

    #[test]
    fn flaky_at_warn_threshold_resolves_yellow() {
        assert_eq!(
            resolve_flaky_population(5, &fallback_flaky()),
            Status::Yellow
        );
    }

    #[test]
    fn flaky_just_below_fail_resolves_yellow() {
        assert_eq!(
            resolve_flaky_population(19, &fallback_flaky()),
            Status::Yellow
        );
    }

    #[test]
    fn flaky_at_fail_threshold_resolves_red() {
        assert_eq!(resolve_flaky_population(20, &fallback_flaky()), Status::Red);
    }

    #[test]
    fn flaky_above_fail_threshold_resolves_red() {
        assert_eq!(resolve_flaky_population(50, &fallback_flaky()), Status::Red);
    }

    // ── Configured-thresholds round-trip ─────────────────────────────

    #[test]
    fn tightened_warn_flips_status_at_smaller_drop() {
        // A drop of -0.8 lands Green against the fallback warn (-1.0)
        // and Yellow against a tightened warn (-0.5) — the round-trip
        // contract operators rely on when they tune `quality.toml`.
        let configured = CoverageThresholds {
            warn_pp_delta: -0.5,
            fail_pp_delta: -5.0,
        };
        assert_eq!(
            resolve_coverage_delta(-0.8, &fallback_coverage()),
            Status::Green
        );
        assert_eq!(resolve_coverage_delta(-0.8, &configured), Status::Yellow);
    }

    // ── Doc-drift constants ──────────────────────────────────────────

    #[test]
    fn fallback_marker_is_html_comment_form() {
        assert!(FALLBACK_MARKER.starts_with("<!--"));
        assert!(FALLBACK_MARKER.ends_with("-->"));
    }

    #[test]
    fn path_hint_comment_is_html_comment_form() {
        assert!(PATH_HINT_COMMENT.starts_with("<!--"));
        assert!(PATH_HINT_COMMENT.ends_with("-->"));
    }

    #[test]
    fn starter_preamble_is_italic_markdown() {
        // Single-underscore italic. Surfaces a regression if someone
        // converts to asterisks (CommonMark allows both, but the
        // surrounding renderer prose conventions use underscores).
        assert!(STARTER_PREAMBLE.starts_with('_'));
        assert!(STARTER_PREAMBLE.ends_with('_'));
    }

    #[test]
    fn starter_preamble_links_quality_toml_anchor() {
        // Link target must resolve to the docs section
        // STARTER_PREAMBLE points operators at.
        assert!(STARTER_PREAMBLE.contains("QUALITY.md#threshold-tuning"));
        assert!(STARTER_PREAMBLE.contains("`quality.toml`"));
    }

    // ── Schema generation under deps-zero (CAO-1 acceptance gate) ────

    #[test]
    fn threshold_config_schema_compiles_without_cli_feature() {
        // Compiles under `--no-default-features` because schemars +
        // serde + serde_json are unconditional deps. If this test
        // fails to compile, the deps-zero invariant has regressed and
        // the cli-feature gate has leaked into the lib path.
        let _schema = schemars::schema_for!(ThresholdConfig);
    }

    #[test]
    fn threshold_config_round_trips_through_serde_json() {
        // A round-trip through serde_json (always-available dep)
        // exercises the Serialize + Deserialize derives without
        // pulling toml. Confirms the JSON shape stays stable for the
        // operator-schema artifact ajv validates against.
        let cfg = ThresholdConfig::fallback();
        let json = serde_json::to_string(&cfg).expect("serialize fallback config");
        let parsed: ThresholdConfig =
            serde_json::from_str(&json).expect("round-trip fallback config");
        assert_eq!(parsed.rows.coverage.warn_pp_delta, -1.0);
        assert_eq!(parsed.rows.coverage.fail_pp_delta, -5.0);
    }

    #[test]
    fn deny_unknown_fields_rejects_typo_at_root() {
        // Operator typo at the root must fail-loud rather than be
        // silently dropped.
        let bad =
            r#"{"rows": {"coverage": {"warn_pp_delta": -1.0, "fail_pp_delta": -5.0}}, "rowz": {}}"#;
        let err = serde_json::from_str::<ThresholdConfig>(bad).unwrap_err();
        assert!(err.to_string().contains("unknown field"), "got: {err}");
    }

    #[test]
    fn deny_unknown_fields_rejects_typo_in_coverage_thresholds() {
        let bad = r#"{"rows": {"coverage": {"warn_pp_delta": -1.0, "fail_pp_delta": -5.0, "fail_pp_dleta": -7.0}}}"#;
        let err = serde_json::from_str::<ThresholdConfig>(bad).unwrap_err();
        assert!(err.to_string().contains("unknown field"), "got: {err}");
    }

    // ── TOML parser (cli feature only) ───────────────────────────────

    #[cfg(feature = "cli")]
    #[test]
    fn parse_quality_toml_round_trips_fallback_values() {
        let input = r#"
[rows.coverage]
warn_pp_delta = -1.0
fail_pp_delta = -5.0
"#;
        let cfg = parse_quality_toml(input).expect("parse");
        assert_eq!(cfg.rows.coverage.warn_pp_delta, -1.0);
        assert_eq!(cfg.rows.coverage.fail_pp_delta, -5.0);
    }

    #[cfg(feature = "cli")]
    #[test]
    fn parse_quality_toml_rejects_unknown_field() {
        let input = r#"
[rows.coverage]
warn_pp_delta = -1.0
fail_pp_delta = -5.0
fail_pp_dleta = -7.0
"#;
        let err = parse_quality_toml(input).unwrap_err();
        assert!(err.to_string().contains("unknown field"), "got: {err}");
    }

    #[cfg(feature = "cli")]
    #[test]
    fn parse_quality_toml_rejects_string_for_numeric_field() {
        let input = r#"
[rows.coverage]
warn_pp_delta = "tight"
fail_pp_delta = -5.0
"#;
        assert!(parse_quality_toml(input).is_err());
    }
}
