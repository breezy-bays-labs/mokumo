//! `aggregate` entry-point logic, split out of `bin/aggregate.rs` so the
//! argument parser, scorecard builder, schema validator, and file writer
//! are reachable as plain library functions and exercised by unit tests
//! that `cargo llvm-cov nextest --workspace` can attribute coverage to.
//!
//! The bin target is a one-line wrapper around [`run`]; everything testable
//! lives here. Gated behind the `cli` feature so the lib's deps-zero
//! invariant (serde + schemars + serde_json only) holds under a default
//! `cargo build`.

#![doc(hidden)]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use jsonschema::JSONSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::threshold::{
    self, BddFeatureSkipThresholds, BddScenarioSkipThresholds, CiWallClockThresholds,
    CoverageThresholds, FlakyPopulationThresholds, ThresholdConfig,
};
use crate::{
    BddFeatureBreakout, BddScenarioBreakout, Breakouts, GateRun, PrMeta, Row, RowCommon, Scorecard,
    Status, TagCount,
};

/// Embedded copy of `.config/scorecard/schema.json`. Embedding (vs. a
/// `--schema <path>` CLI flag) keeps the binary cwd-portable: any CI
/// runner or local invocation gets the same schema the source built
/// against. The drift-check integration test (`tests/schema_drift.rs`)
/// guarantees byte-identity between this string and the committed file.
const COMMITTED_SCHEMA: &str = include_str!("../../../.config/scorecard/schema.json");

#[derive(Debug, Parser)]
#[command(name = "aggregate", about = "Sticky scorecard.json producer.")]
struct Cli {
    /// Path to a JSON file matching the `PrMeta` shape:
    ///   { "pr_number": u64, "head_sha": "...", "base_sha": "...", "is_fork": bool }
    #[arg(long)]
    pr_meta: PathBuf,

    /// Coverage delta vs. base, in percentage points (signed). The
    /// producer feeds this through [`threshold::resolve_coverage_delta`]
    /// against the resolved [`ThresholdConfig`] to mint the row's
    /// status. `allow_hyphen_values` so a regression like `-2.5` is
    /// not mis-parsed as a short flag.
    ///
    /// Rejected at parse time: `NaN`, `inf`, `-inf`. `f64::from_str`
    /// accepts those literals, but a non-finite delta has no defensible
    /// position in the warn/fail ordering and would silently resolve
    /// Green under the default comparison rules. Loud-fail at the CLI
    /// boundary keeps the verdict honest.
    #[arg(long, allow_hyphen_values = true, value_parser = parse_finite_f64)]
    coverage_delta_pp: f64,

    /// Path to the operator-tuned `quality.toml`. When the file is
    /// absent the producer falls back to the hardcoded
    /// [`ThresholdConfig::fallback`] thresholds and marks
    /// `fallback_thresholds_active = true` on the artifact. When the
    /// file is present but cannot be parsed, the producer fails with
    /// a non-zero exit so an operator typo never silently slides into
    /// fallback mode and produces a different verdict than intended.
    #[arg(long, default_value = ".config/scorecard/quality.toml")]
    quality_toml: PathBuf,

    /// Roots to walk for BDD `.feature` files. Repeat the flag for
    /// multiple roots. When the flag is omitted the producer emits the
    /// two BDD rows with `0 skipped / 0 total` rather than producer-
    /// pending stubs — both rows are wired even on a corpus-less run.
    #[arg(long = "bdd-features-root", value_name = "DIR")]
    bdd_features_roots: Vec<PathBuf>,

    /// Path to the CI wall-clock JSON artifact produced by the workflow
    /// `total_seconds` aggregation step. Shape:
    /// `{ "total_seconds": f64, "base_total_seconds": Option<f64> }`.
    /// When the flag is omitted the producer emits a Green
    /// `CiWallClockDelta` row with delta_seconds=0 — the row is wired
    /// even when the base SHA's data is unavailable.
    #[arg(long, value_name = "PATH")]
    ci_wall_clock_json: Option<PathBuf>,

    /// Roots to walk for `// FLAKY:` markers. Repeat the flag for
    /// multiple roots. When the flag is omitted the producer emits a
    /// Green `FlakyPopulation` row with `flaky_marker_count: 0` — the
    /// row is wired even on a corpus-less run.
    #[arg(long = "flaky-source-root", value_name = "DIR")]
    flaky_source_roots: Vec<PathBuf>,

    /// Path to a JSON artifact `{ "retry_count": u32 }` capturing the
    /// number of nextest retry events on the head SHA's run. Optional;
    /// when absent `nextest_retry_events: 0` is emitted.
    #[arg(long, value_name = "PATH")]
    nextest_retry_json: Option<PathBuf>,

    /// Path to a newline-delimited file listing the paths changed
    /// between base and head. The producer derives a Mermaid
    /// `graph LR` visualization grouped by crate / app for the
    /// `ChangedScopeDiagram` row. Absent flag emits an empty-scope
    /// row (Green, `(no diff)`).
    #[arg(long, value_name = "PATH")]
    changed_files: Option<PathBuf>,

    /// Path to write the resulting scorecard.json artifact. Parent
    /// directories are created if missing.
    #[arg(long)]
    out: PathBuf,
}

/// `clap` value-parser that accepts an `f64` and rejects non-finite
/// values (`NaN`, `+inf`, `-inf`).
///
/// `f64::from_str` itself accepts those literals, so a producer
/// invocation like `--coverage-delta-pp NaN` would otherwise satisfy
/// clap's default parser and silently resolve Green under the
/// `delta_pp <= warn_pp_delta` comparison. Rejecting at the boundary
/// converts an invalid input into a non-zero exit with a clear message
/// instead of a silent verdict.
fn parse_finite_f64(raw: &str) -> Result<f64, String> {
    let parsed: f64 = raw
        .parse()
        .map_err(|e| format!("not a valid floating-point number: {e}"))?;
    if !parsed.is_finite() {
        return Err(format!(
            "must be a finite number (NaN and infinity are not allowed), got {raw}",
        ));
    }
    Ok(parsed)
}

/// Format a coverage delta (in percentage points) for display.
///
/// Positive deltas carry an explicit `+` sign so a glance at the row
/// makes the direction unambiguous; negative deltas pick up the sign
/// from `f64`'s default formatting. One decimal place keeps the row
/// table from drifting columns when the delta crosses thresholds.
pub fn format_delta_text(delta_pp: f64) -> String {
    if delta_pp >= 0.0 {
        format!("+{delta_pp:.1} pp")
    } else {
        format!("{delta_pp:.1} pp")
    }
}

/// Render the inline failure detail for a Red coverage row.
///
/// The renderer wraps this string as the body of a markdown blockquote
/// keyed by the row label, so the prose reads as a complete sentence
/// after the label-colon prefix the renderer adds. Both numbers are
/// reported in absolute magnitude (operators read "6.0 pp drop" more
/// fluently than "-6.0 pp delta").
fn coverage_failure_detail(delta_pp: f64, fail_pp_delta: f64) -> String {
    // "at or below" matches the resolver's inclusive boundary: a delta
    // exactly at `fail_pp_delta` lands Red.
    format!(
        "Coverage dropped {drop:.1} pp — at or below the {fail:.1} pp fail threshold.",
        drop = -delta_pp,
        fail = -fail_pp_delta,
    )
}

/// Read the resolved [`Status`] off any [`Row`] variant.
///
/// Every variant carries a `status` field; the `or`-pattern keeps the
/// rollup definition co-located instead of scattered across call sites.
fn row_status(row: &Row) -> Status {
    match row {
        Row::CoverageDelta { status, .. }
        | Row::CrapDelta { status, .. }
        | Row::MutationSurvivors { status, .. }
        | Row::BddFeatureLevelSkipped { status, .. }
        | Row::BddScenarioLevelSkipped { status, .. }
        | Row::GateRuns { status, .. }
        | Row::FlakyPopulation { status, .. }
        | Row::CiWallClockDelta { status, .. }
        | Row::HandlerCoverageAxis { status, .. }
        | Row::ChangedScopeDiagram { status, .. } => *status,
    }
}

/// Build a coverage row from the raw delta + the thresholds in effect.
fn build_coverage_row(delta_pp: f64, thresholds: &CoverageThresholds) -> Row {
    let common = RowCommon {
        id: "coverage".into(),
        label: "Coverage".into(),
        anchor: "coverage".into(),
    };
    let delta_text = format_delta_text(delta_pp);
    // V4 ships an empty `Breakouts` default — `by_crate[]` populates
    // when per-crate coverage signal lands; per-handler-branch
    // coverage waits on the producer (mokumo#583, currently
    // re-architecting). The renderer surfaces a `(per-handler producer
    // pending — see #583)` note inline when `handlers` is empty.
    let breakouts = Breakouts::default();
    match threshold::resolve_coverage_delta(delta_pp, thresholds) {
        Status::Green => Row::coverage_delta_green(common, delta_pp, delta_text, breakouts),
        Status::Yellow => Row::coverage_delta_yellow(common, delta_pp, delta_text, breakouts),
        Status::Red => Row::coverage_delta_red(
            common,
            delta_pp,
            delta_text,
            breakouts,
            coverage_failure_detail(delta_pp, thresholds.fail_pp_delta),
        ),
    }
}

// ── Layer-3 stub fallback ──────────────────────────────────────────────
//
// Per the V4 closure model, the v0 row inventory includes variants
// whose producers have not yet shipped. Rather than file a sub-issue
// per blocked row (the orchestration debt the parent issue closure-
// model section retired), V4 emits each producer-blocked row as a
// graceful Green "stub" with `delta_text` pinned to the
// [`PENDING_TEXT_PREFIX`] sentinel + the upstream producer's issue
// reference. The renderer surfaces the row inline; GitHub's automatic
// linking turns refs like `crap4rs#111` into clickable links inside
// the sticky comment without any extra renderer logic.
//
// Each producer-blocked row carries a stable, dedicated constant for
// its producer reference so a future row-population follow-up PR can
// grep this module for the matching constant and replace the stub
// helper with a real producer.

/// Renderer-detectable prefix that marks a row as a producer-pending
/// stub. The renderer keys off this prefix to surface the
/// `(producer pending — see #N)` cell + GitHub-autolink the issue
/// reference. Mirrored by [`render.js::PENDING_DELTA_PREFIX`] (vitest
/// snapshot pins byte-equality across the boundary).
pub const PENDING_TEXT_PREFIX: &str = "(producer pending — see ";

/// Closing parenthesis for the stub sentinel.
pub const PENDING_TEXT_SUFFIX: &str = ")";

/// Producer reference for the [`Row::CrapDelta`] stub. Replaced when
/// crap4rs#111 (`--format scorecard-row`) ships and the aggregator
/// consumes its output.
const CRAP_DELTA_PENDING_REF: &str = "crap4rs#111";

/// Producer reference for the [`Row::MutationSurvivors`] stub.
/// Replaced when mokumo#748 wires `cargo-mutants --in-diff` into the
/// CI pipeline.
const MUTATION_SURVIVORS_PENDING_REF: &str = "mokumo#748";

/// Producer reference for the [`Row::HandlerCoverageAxis`] stub.
/// Replaced when the BDD-coverage map (mokumo#654 + #655) is built.
const HANDLER_COVERAGE_AXIS_PENDING_REF: &str = "mokumo#654, mokumo#655";

/// Producer reference for the [`Row::GateRuns`] stub. V4 ships the
/// schema variant; V5 (mokumo#770) populates per-gate Check Runs.
const GATE_RUNS_PENDING_REF: &str = "mokumo#770";

/// Format the sentinel `delta_text` for a producer-pending row.
fn pending_delta_text(producer_ref: &str) -> String {
    format!("{PENDING_TEXT_PREFIX}{producer_ref}{PENDING_TEXT_SUFFIX}")
}

/// Mint a stub `Row::CrapDelta` row pinned to the upstream producer
/// (`crap4rs#111`). Status is Green so the row does not poison the
/// `overall_status` rollup.
fn stub_crap_delta_pending() -> Row {
    let common = RowCommon {
        id: "crap_delta".into(),
        label: "CRAP Δ".into(),
        anchor: "crap-delta".into(),
    };
    Row::crap_delta_green(common, 15, 0, pending_delta_text(CRAP_DELTA_PENDING_REF))
}

/// Mint a stub `Row::MutationSurvivors` row pinned to mokumo#748.
fn stub_mutation_survivors_pending() -> Row {
    let common = RowCommon {
        id: "mutation_survivors".into(),
        label: "Mutation survivors".into(),
        anchor: "mutation-survivors".into(),
    };
    Row::mutation_survivors_green(
        common,
        0,
        Vec::new(),
        pending_delta_text(MUTATION_SURVIVORS_PENDING_REF),
    )
}

/// Mint a stub `Row::HandlerCoverageAxis` row pinned to mokumo#654 +
/// mokumo#655.
fn stub_handler_coverage_axis_pending() -> Row {
    let common = RowCommon {
        id: "handler_coverage_axis".into(),
        label: "Handler axes".into(),
        anchor: "handler-coverage-axis".into(),
    };
    Row::handler_coverage_axis_green(
        common,
        Vec::new(),
        pending_delta_text(HANDLER_COVERAGE_AXIS_PENDING_REF),
    )
}

/// Mint a stub `Row::GateRuns` row pinned to mokumo#770. V5 (#770)
/// replaces this with real per-gate Check Run references.
fn stub_gate_runs_pending() -> Row {
    let common = RowCommon {
        id: "gate_runs".into(),
        label: "Gates".into(),
        anchor: "gate-runs".into(),
    };
    Row::gate_runs_green(
        common,
        Vec::<GateRun>::new(),
        pending_delta_text(GATE_RUNS_PENDING_REF),
    )
}

// ── BDD scenario / skip producer ───────────────────────────────────────
//
// V4 (#769) §4 wired rows. The producer walks operator-supplied
// `.feature` directory roots, parses each file's tag stack and scenario
// keywords, and emits a feature-level + scenario-level pair of rows
// (see [`build_bdd_feature_skip_row`] / [`build_bdd_scenario_skip_row`]).
// The split lets reviewers tell *backlog* growth (whole feature files
// gated `@wip`) from *hygiene* growth (individual scenarios skipped)
// without conflating the two.

/// Tags that mark a scenario or feature as skipped from execution.
/// Matches the cucumber-rs convention used across the workspace.
const BDD_SKIP_TAGS: &[&str] = &["@wip", "@future", "@ignore", "@skip"];

/// Tag prefix that marks a scenario or feature as tracked-but-deferred.
/// Tag payloads after the colon (`@tracked:mokumo#123`) act as upstream
/// issue references the renderer can autolink.
const BDD_TRACKED_TAG_PREFIX: &str = "@tracked:";

/// Aggregated BDD corpus statistics computed from one or more
/// `.feature` files. Splits the count into feature-level (whole files
/// gated `@wip`) and scenario-level (individual scenarios with their
/// own skip tag, *not* inheriting from a feature-level tag).
#[derive(Debug, Default, Clone)]
pub struct BddSummary {
    /// Total `.feature` files across the corpus.
    pub total_features: u32,
    /// `.feature` files bearing at least one feature-level skip tag.
    pub skipped_features: u32,
    /// Workspace-wide tag breakdown across feature-level skip tags.
    /// Counts increment once per file (not once per scenario inside).
    pub feature_by_tag: std::collections::BTreeMap<String, u32>,
    /// Per-crate breakdown of feature-file counts. Sorted by
    /// `crate_name` for deterministic artifacts.
    pub feature_breakouts: Vec<BddFeatureBreakout>,
    /// Total scenarios across the corpus (`Scenario:` +
    /// `Scenario Outline:` + `Example:`).
    pub total_scenarios: u32,
    /// Scenarios whose own tag set carries a skip tag — does NOT count
    /// scenarios inheriting a skip tag from a feature-level tag.
    pub skipped_scenarios: u32,
    /// Workspace-wide tag breakdown across scenario-level skip tags.
    /// Counts increment once per scenario.
    pub scenario_by_tag: std::collections::BTreeMap<String, u32>,
    /// Per-crate breakdown of scenario counts. Sorted by `crate_name`
    /// for deterministic artifacts.
    pub scenario_breakouts: Vec<BddScenarioBreakout>,
}

/// `true` when a tag literal counts as a skip tag.
fn is_bdd_skip_tag(tag: &str) -> bool {
    BDD_SKIP_TAGS.contains(&tag) || tag.starts_with(BDD_TRACKED_TAG_PREFIX)
}

/// `true` when a slice of tag literals contains at least one skip tag.
fn any_skip_tag(tags: &[String]) -> bool {
    tags.iter().any(|t| is_bdd_skip_tag(t))
}

#[derive(Debug, Default)]
struct ParsedFeature {
    /// Scenarios in the file (`Scenario:` + `Scenario Outline:` +
    /// `Example:`).
    total_scenarios: u32,
    /// Scenarios whose own pending tag set carries a skip tag. Only
    /// populated when the file is NOT feature-level skipped — when
    /// the whole feature is gated, every scenario is already counted
    /// at the feature level.
    scenario_skipped: u32,
    /// `true` when the feature line carries at least one skip tag, in
    /// which case all scenarios in the file inherit the skip.
    feature_level_skipped: bool,
    /// The feature-level tags the parser attached to the `Feature:`
    /// line — used to populate `feature_by_tag` in the summary.
    feature_tags: Vec<String>,
    /// Per-tag breakdown across the file's *scenario-level* skip tags.
    /// Counts only fire when the scenario was scenario-level skipped
    /// (so the breakdown stays a strict view of the scenario count).
    scenario_by_tag: std::collections::BTreeMap<String, u32>,
}

/// Classification of a single `.feature` line into the
/// dispatch buckets `parse_feature` cares about.
#[derive(Debug, PartialEq, Eq)]
enum FeatureLineKind {
    /// Empty line or `#`-style comment — skipped.
    Skip,
    /// One or more `@token` tags on the same line.
    Tags,
    /// `Feature:` or `Rule:` — moves the parser past the
    /// preamble and locks feature-level tags.
    FeatureOrRule,
    /// `Scenario:` / `Scenario Outline:` / `Example:` —
    /// counts toward the scenario total.
    ScenarioStart,
    /// `Background:` — drains pending tags so they don't
    /// bleed into the next scenario.
    Background,
    /// Steps, table rows, doc-strings, etc. — ignored.
    Other,
}

/// Classify a `.feature` line *after trimming leading
/// whitespace*. Pure function on borrowed input — no
/// state, no allocations.
fn classify_feature_line(line: &str) -> FeatureLineKind {
    if line.is_empty() || line.starts_with('#') {
        FeatureLineKind::Skip
    } else if line.starts_with('@') {
        FeatureLineKind::Tags
    } else if line.starts_with("Feature:") || line.starts_with("Rule:") {
        FeatureLineKind::FeatureOrRule
    } else if line.starts_with("Scenario:")
        || line.starts_with("Scenario Outline:")
        || line.starts_with("Example:")
    {
        FeatureLineKind::ScenarioStart
    } else if line.starts_with("Background:") {
        FeatureLineKind::Background
    } else {
        FeatureLineKind::Other
    }
}

/// Extract every `@token` from a tag line. Tokens that do
/// not start with `@` (whitespace, parameter strings) are
/// discarded.
fn extract_tags(line: &str) -> Vec<String> {
    line.split_whitespace()
        .filter(|t| t.starts_with('@'))
        .map(str::to_string)
        .collect()
}

/// Tally a single scenario's tag set into the running
/// `ParsedFeature`. Scenario-level skip tags only count when the file
/// is NOT feature-level skipped — when the whole feature is gated,
/// the scenario is already accounted for at the feature level and
/// double-counting it would inflate the hygiene signal.
fn tally_scenario_tags(parsed: &mut ParsedFeature, pending: &mut Vec<String>) {
    parsed.total_scenarios += 1;
    let scenario_tags = std::mem::take(pending);
    if parsed.feature_level_skipped {
        return;
    }
    if !any_skip_tag(&scenario_tags) {
        return;
    }
    parsed.scenario_skipped += 1;
    for tag in scenario_tags {
        if is_bdd_skip_tag(&tag) {
            *parsed.scenario_by_tag.entry(tag).or_insert(0) += 1;
        }
    }
}

/// Parse a `.feature` file body. Splits the file's signal into
/// feature-level (whole file gated) and scenario-level (individual
/// scenarios with their own skip tags) so the producer can emit two
/// independent rows.
///
/// Not a full Gherkin parser — good enough for counting + tagging.
/// Dispatch lives in [`classify_feature_line`]; tag extraction in
/// [`extract_tags`]; per-scenario tally in [`tally_scenario_tags`].
fn parse_feature(contents: &str) -> ParsedFeature {
    let mut pending: Vec<String> = Vec::new();
    let mut feature_seen = false;
    let mut parsed = ParsedFeature::default();

    for raw in contents.lines() {
        let line = raw.trim();
        match classify_feature_line(line) {
            FeatureLineKind::Skip | FeatureLineKind::Other => {}
            FeatureLineKind::Tags => pending.extend(extract_tags(line)),
            FeatureLineKind::FeatureOrRule => {
                if feature_seen {
                    pending.clear();
                } else {
                    parsed.feature_tags = std::mem::take(&mut pending);
                    parsed.feature_level_skipped = any_skip_tag(&parsed.feature_tags);
                    feature_seen = true;
                }
            }
            FeatureLineKind::ScenarioStart => {
                tally_scenario_tags(&mut parsed, &mut pending);
            }
            // Background lines drain pending tags so a stray `@` line
            // followed by `Background:` does not bleed into the next
            // scenario.
            FeatureLineKind::Background => pending.clear(),
        }
    }

    parsed
}

/// Derive a crate / app name from a workspace-relative path. Returns
/// `Some(name)` for `crates/<name>/...` or `apps/<name>/...` paths;
/// returns `None` for paths outside those two trees (root files, docs,
/// `.github/`, …). Callers decide whether the un-recognised slice is
/// noise to drop or a fallback bucket to mint.
fn crate_name_from_path(path: &Path) -> Option<String> {
    let parts: Vec<_> = path.components().collect();
    for (i, c) in parts.iter().enumerate() {
        let std::path::Component::Normal(s) = c else {
            continue;
        };
        let s = s.to_string_lossy();
        if (s == "crates" || s == "apps")
            && i + 1 < parts.len()
            && let std::path::Component::Normal(name) = &parts[i + 1]
        {
            return Some(name.to_string_lossy().into_owned());
        }
    }
    None
}

/// `true` when `path` is a regular file ending in `.feature`.
fn is_feature_file(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("feature")
}

/// Walk one or more existing roots for files matching `predicate` and
/// return the matching paths (sorted; deterministic).
///
/// Missing roots are silently skipped — that's the contract callers
/// rely on when they pass a list of optional / not-yet-created roots.
fn walk_files_matching<F>(roots: &[PathBuf], predicate: F) -> Vec<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            if predicate(entry.path()) {
                out.push(entry.path().to_path_buf());
            }
        }
    }
    out.sort();
    out
}

/// Read `path` and return its UTF-8 contents, mapping I/O errors to a
/// human-readable producer message.
fn read_source_file(path: &Path, label: &'static str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| {
        format!(
            "aggregate: failed to read {label} file {}: {e}",
            path.display()
        )
    })
}

/// Per-crate accumulator used while folding parsed feature files into a
/// [`BddSummary`]. Tracks the feature-file count and the scenario count
/// independently so the producer can emit two breakouts per crate.
#[derive(Debug, Default)]
struct BddCrateAcc {
    feature_total: u32,
    feature_skipped: u32,
    feature_tag_counts: std::collections::BTreeMap<String, u32>,
    scenario_total: u32,
    scenario_skipped: u32,
    scenario_tag_counts: std::collections::BTreeMap<String, u32>,
}

impl BddCrateAcc {
    fn merge(&mut self, parsed: &ParsedFeature) {
        self.feature_total += 1;
        self.scenario_total += parsed.total_scenarios;
        if parsed.feature_level_skipped {
            self.feature_skipped += 1;
            for tag in &parsed.feature_tags {
                if is_bdd_skip_tag(tag) {
                    *self.feature_tag_counts.entry(tag.clone()).or_insert(0) += 1;
                }
            }
        } else {
            self.scenario_skipped += parsed.scenario_skipped;
            for (tag, n) in &parsed.scenario_by_tag {
                *self.scenario_tag_counts.entry(tag.clone()).or_insert(0) += n;
            }
        }
    }

    fn to_feature_breakout(&self, crate_name: String) -> BddFeatureBreakout {
        BddFeatureBreakout {
            crate_name,
            feature_total: self.feature_total,
            feature_skipped: self.feature_skipped,
            by_tag: tag_counts_to_vec(&self.feature_tag_counts),
        }
    }

    fn to_scenario_breakout(&self, crate_name: String) -> BddScenarioBreakout {
        BddScenarioBreakout {
            crate_name,
            scenario_total: self.scenario_total,
            scenario_skipped: self.scenario_skipped,
            by_tag: tag_counts_to_vec(&self.scenario_tag_counts),
        }
    }
}

/// Convert a deterministic `BTreeMap<tag, count>` view into the wire
/// `Vec<TagCount>` shape.
fn tag_counts_to_vec(map: &std::collections::BTreeMap<String, u32>) -> Vec<TagCount> {
    map.iter()
        .map(|(tag, count)| TagCount {
            tag: tag.clone(),
            count: *count,
        })
        .collect()
}

/// Fold a single parsed feature into per-crate accumulators and into
/// the workspace-wide `BddSummary`. Mutates `summary` in place — the
/// only field it does NOT touch is the per-crate breakouts (those are
/// derived from `per_crate` after the walk completes).
fn merge_parsed_feature(
    per_crate: &mut std::collections::BTreeMap<String, BddCrateAcc>,
    summary: &mut BddSummary,
    crate_name: String,
    parsed: ParsedFeature,
) {
    summary.total_features += 1;
    summary.total_scenarios += parsed.total_scenarios;
    if parsed.feature_level_skipped {
        summary.skipped_features += 1;
        for tag in &parsed.feature_tags {
            if is_bdd_skip_tag(tag) {
                *summary.feature_by_tag.entry(tag.clone()).or_insert(0) += 1;
            }
        }
    } else {
        summary.skipped_scenarios += parsed.scenario_skipped;
        for (tag, n) in &parsed.scenario_by_tag {
            *summary.scenario_by_tag.entry(tag.clone()).or_insert(0) += n;
        }
    }
    per_crate.entry(crate_name).or_default().merge(&parsed);
}

/// Walk one or more roots for `.feature` files and aggregate the BDD
/// corpus into a [`BddSummary`].
///
/// Returns an error when a discovered file cannot be read; missing
/// roots are silently skipped (an empty `--bdd-features-root` set
/// produces an empty summary).
pub fn discover_bdd_corpus(roots: &[PathBuf]) -> Result<BddSummary, String> {
    let mut per_crate: std::collections::BTreeMap<String, BddCrateAcc> = Default::default();
    let mut summary = BddSummary::default();

    for path in walk_files_matching(roots, is_feature_file) {
        let contents = read_source_file(&path, "feature")?;
        let parsed = parse_feature(&contents);
        // `.feature` files always live under `crates/<name>/...` or
        // `apps/<name>/...` in this workspace. A path that escapes
        // both trees is a hand-rolled test fixture or an authoring
        // mistake — bucket those under a stable label so the breakout
        // stays deterministic instead of dropping them silently.
        let crate_name = crate_name_from_path(&path).unwrap_or_else(|| "unknown".to_string());
        merge_parsed_feature(&mut per_crate, &mut summary, crate_name, parsed);
    }

    summary.feature_breakouts = per_crate
        .iter()
        .map(|(name, acc)| acc.to_feature_breakout(name.clone()))
        .collect();
    summary.scenario_breakouts = per_crate
        .iter()
        .map(|(name, acc)| acc.to_scenario_breakout(name.clone()))
        .collect();

    Ok(summary)
}

/// Render the failure detail for a Red BDD feature-skip row.
fn bdd_feature_failure_detail(skipped_features: u32, fail_threshold: u32) -> String {
    format!(
        "BDD feature-level WIP count is {skipped_features} — at or above the {fail_threshold} fail threshold."
    )
}

/// Render the failure detail for a Red BDD scenario-skip row.
fn bdd_scenario_failure_detail(skipped_scenarios: u32, fail_threshold: u32) -> String {
    format!(
        "BDD scenario-level skip count is {skipped_scenarios} — at or above the {fail_threshold} fail threshold."
    )
}

/// Format the delta-text shown alongside the BDD feature-skip row.
fn bdd_feature_delta_text(total_features: u32, skipped_features: u32) -> String {
    format!("{skipped_features} WIP / {total_features} features")
}

/// Format the delta-text shown alongside the BDD scenario-skip row.
fn bdd_scenario_delta_text(total_scenarios: u32, skipped_scenarios: u32) -> String {
    format!("{skipped_scenarios} skipped / {total_scenarios} scenarios")
}

/// Convert the workspace-wide `feature_by_tag` map into the wire
/// `Vec<TagCount>` ordering (sorted by tag, deterministic).
fn workspace_tag_counts(map: &std::collections::BTreeMap<String, u32>) -> Vec<TagCount> {
    tag_counts_to_vec(map)
}

/// Build a wired `Row::BddFeatureLevelSkipped` from a corpus summary +
/// thresholds.
pub fn build_bdd_feature_skip_row(
    summary: &BddSummary,
    thresholds: &BddFeatureSkipThresholds,
) -> Row {
    let common = RowCommon {
        id: "bdd_feature_skip".into(),
        label: "WIP feature files".into(),
        anchor: "bdd-feature-skip".into(),
    };
    let delta_text = bdd_feature_delta_text(summary.total_features, summary.skipped_features);
    let by_tag = workspace_tag_counts(&summary.feature_by_tag);
    match threshold::resolve_bdd_feature_skip(summary.skipped_features, thresholds) {
        Status::Green => Row::bdd_feature_level_skipped_green(
            common,
            summary.total_features,
            summary.skipped_features,
            by_tag,
            summary.feature_breakouts.clone(),
            delta_text,
        ),
        Status::Yellow => Row::bdd_feature_level_skipped_yellow(
            common,
            summary.total_features,
            summary.skipped_features,
            by_tag,
            summary.feature_breakouts.clone(),
            delta_text,
        ),
        Status::Red => Row::bdd_feature_level_skipped_red(
            common,
            summary.total_features,
            summary.skipped_features,
            by_tag,
            summary.feature_breakouts.clone(),
            delta_text,
            bdd_feature_failure_detail(summary.skipped_features, thresholds.fail_skipped_features),
        ),
    }
}

/// Build a wired `Row::BddScenarioLevelSkipped` from a corpus summary +
/// thresholds.
pub fn build_bdd_scenario_skip_row(
    summary: &BddSummary,
    thresholds: &BddScenarioSkipThresholds,
) -> Row {
    let common = RowCommon {
        id: "bdd_scenario_skip".into(),
        label: "WIP scenarios".into(),
        anchor: "bdd-scenario-skip".into(),
    };
    let delta_text = bdd_scenario_delta_text(summary.total_scenarios, summary.skipped_scenarios);
    let by_tag = workspace_tag_counts(&summary.scenario_by_tag);
    match threshold::resolve_bdd_scenario_skip(summary.skipped_scenarios, thresholds) {
        Status::Green => Row::bdd_scenario_level_skipped_green(
            common,
            summary.total_scenarios,
            summary.skipped_scenarios,
            by_tag,
            summary.scenario_breakouts.clone(),
            delta_text,
        ),
        Status::Yellow => Row::bdd_scenario_level_skipped_yellow(
            common,
            summary.total_scenarios,
            summary.skipped_scenarios,
            by_tag,
            summary.scenario_breakouts.clone(),
            delta_text,
        ),
        Status::Red => Row::bdd_scenario_level_skipped_red(
            common,
            summary.total_scenarios,
            summary.skipped_scenarios,
            by_tag,
            summary.scenario_breakouts.clone(),
            delta_text,
            bdd_scenario_failure_detail(
                summary.skipped_scenarios,
                thresholds.fail_skipped_scenarios,
            ),
        ),
    }
}

// ── CI wall-clock producer ─────────────────────────────────────────────
//
// V4 (#769) §4 wired row. The producer reads a JSON artifact emitted by
// the workflow's per-job duration aggregation step, computes the delta
// against the base SHA's most recent run (if available), and feeds the
// result through `threshold::resolve_ci_wall_clock`.

/// Wire shape of `--ci-wall-clock-json`. `base_total_seconds` is
/// `Option<f64>` because the base SHA's wall-clock is not always
/// available (first PR on a branch, base run never recorded an
/// artifact, fork PRs without artifact-read permission). Absence
/// resolves Green with `delta_seconds: 0` rather than failing closed
/// — the row is informational on a no-base-data run.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CiWallClockJson {
    /// Total CI wall-clock for the head SHA, in seconds.
    pub total_seconds: f64,
    /// Total CI wall-clock for the base SHA, in seconds. `None` when
    /// the base run's artifact is absent.
    #[serde(default)]
    pub base_total_seconds: Option<f64>,
}

/// Render the inline failure detail for a Red CI wall-clock row.
fn ci_wall_clock_failure_detail(delta_seconds: f64, fail_threshold: f64) -> String {
    format!(
        "CI wall-clock grew by {delta_seconds:.0}s — at or above the {fail_threshold:.0}s fail threshold."
    )
}

/// `+` for non-negative deltas, empty string for negatives.
/// Mirrors the convention `format_delta_text` uses.
fn delta_sign_prefix(delta: f64) -> &'static str {
    if delta >= 0.0 { "+" } else { "" }
}

/// Format the `delta_text` for a wired CI wall-clock row.
fn ci_wall_clock_delta_text(json: &CiWallClockJson, delta_seconds: f64) -> String {
    let total = json.total_seconds;
    let Some(_base) = json.base_total_seconds else {
        return format!("{total:.0}s total / (no base)");
    };
    let sign = delta_sign_prefix(delta_seconds);
    format!("{total:.0}s total / {sign}{delta_seconds:.0}s vs base")
}

/// Build a wired `Row::CiWallClockDelta` from the JSON artifact +
/// thresholds.
pub fn build_ci_wall_clock_row(json: &CiWallClockJson, thresholds: &CiWallClockThresholds) -> Row {
    let common = RowCommon {
        id: "ci_wall_clock".into(),
        label: "CI wall-clock".into(),
        anchor: "ci-wall-clock".into(),
    };
    // No base data → delta is zero. The row reports total CI seconds
    // unconditionally so operators see the absolute value even before
    // base comparison is available.
    let delta_seconds = match json.base_total_seconds {
        Some(base) => json.total_seconds - base,
        None => 0.0,
    };
    let delta_text = ci_wall_clock_delta_text(json, delta_seconds);
    match threshold::resolve_ci_wall_clock(delta_seconds, thresholds) {
        Status::Green => {
            Row::ci_wall_clock_delta_green(common, json.total_seconds, delta_seconds, delta_text)
        }
        Status::Yellow => {
            Row::ci_wall_clock_delta_yellow(common, json.total_seconds, delta_seconds, delta_text)
        }
        Status::Red => Row::ci_wall_clock_delta_red(
            common,
            json.total_seconds,
            delta_seconds,
            delta_text,
            ci_wall_clock_failure_detail(delta_seconds, thresholds.fail_seconds_delta),
        ),
    }
}

/// Validate that `parsed` carries finite floating-point values for
/// the wall-clock shape. Caller maps the error string to its own CLI
/// message prefix.
fn validate_ci_wall_clock_finite(parsed: &CiWallClockJson, path: &Path) -> Result<(), String> {
    if !parsed.total_seconds.is_finite() {
        return Err(format!(
            "aggregate: --ci-wall-clock-json {} total_seconds must be finite, got {}",
            path.display(),
            parsed.total_seconds
        ));
    }
    if let Some(base) = parsed.base_total_seconds
        && !base.is_finite()
    {
        return Err(format!(
            "aggregate: --ci-wall-clock-json {} base_total_seconds must be finite, got {base}",
            path.display(),
        ));
    }
    Ok(())
}

/// Read the `--ci-wall-clock-json` file if present, returning `None`
/// when the flag was omitted. Reports a clear error when the file is
/// supplied but cannot be read or does not match the wire shape.
pub fn read_ci_wall_clock_json(path: Option<&Path>) -> Result<Option<CiWallClockJson>, String> {
    let Some(path) = path else { return Ok(None) };
    let bytes = fs::read(path).map_err(|e| {
        format!(
            "aggregate: cannot read --ci-wall-clock-json {}: {e}",
            path.display()
        )
    })?;
    let parsed: CiWallClockJson = serde_json::from_slice(&bytes).map_err(|e| {
        format!(
            "aggregate: --ci-wall-clock-json {} is not a valid CiWallClockJson: {e}",
            path.display()
        )
    })?;
    validate_ci_wall_clock_finite(&parsed, path)?;
    Ok(Some(parsed))
}

// ── Flaky-population producer ──────────────────────────────────────────
//
// V4 (#769) §4 wired row. The producer scans operator-supplied source
// roots for `// FLAKY:` markers (a repo-wide convention documented in
// QUALITY.md, landed in C8) and optionally consumes a JSON artifact
// reporting nextest retry events on the head SHA. Threshold resolver
// in `threshold::resolve_flaky_population` mints status from the marker
// count.

/// Source-file extensions the flaky-marker scanner inspects. Limited
/// to text-source code files we expect `// FLAKY:` to appear in;
/// extending the list is additive.
const FLAKY_SCAN_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "mjs", "svelte"];

/// Marker substring the producer counts in source files. Documented in
/// QUALITY.md (C8) so contributors know to tag a flaky test with a
/// trailing `// FLAKY: <reason>` comment as the canonical signal.
const FLAKY_MARKER: &str = "// FLAKY:";

/// Wire shape of `--nextest-retry-json`. Captures only the head SHA's
/// retry-event count today; richer per-test breakouts are a future
/// extension paid for when a producer can populate them.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NextestRetryJson {
    /// Number of retry events recorded on the head SHA's run. Captured
    /// from nextest's output (or `0` on best-effort runs).
    pub retry_count: u32,
}

/// Aggregated flaky-population corpus. Pure-data input to
/// [`build_flaky_population_row`].
#[derive(Debug, Default, Clone)]
pub struct FlakyCorpus {
    /// Total `// FLAKY:` markers across the scanned source roots.
    pub marker_count: u32,
    /// Retry events from the optional `--nextest-retry-json` input
    /// (`0` when absent).
    pub retry_events: u32,
}

/// `true` when `path`'s extension is one of [`FLAKY_SCAN_EXTENSIONS`].
fn is_flaky_scannable(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| FLAKY_SCAN_EXTENSIONS.contains(&e))
}

/// `true` when `line` is a real `// FLAKY:` marker — i.e. starts with
/// the marker after trimming leading whitespace, with the marker not
/// preceded by another `/` (which would make it `/// FLAKY:`, a
/// rustdoc reference, not a real marker).
///
/// Rejects:
/// - Doc-comment references such as `/// `// FLAKY:` source markers...`
/// - Test-fixture string literals such as `"// FLAKY: ignored"`
/// - The constant declaration `const FLAKY_MARKER: &str = "// FLAKY:"`
///
/// Accepts only lines whose first non-whitespace characters are the
/// literal marker.
fn is_flaky_marker_line(line: &str) -> bool {
    line.trim_start().starts_with(FLAKY_MARKER)
}

/// Count the number of real `// FLAKY:` markers in `contents`.
fn count_flaky_markers(contents: &str) -> u32 {
    u32::try_from(contents.lines().filter(|l| is_flaky_marker_line(l)).count()).unwrap_or(u32::MAX)
}

/// Walk one or more roots for source files and count `// FLAKY:`
/// markers. Reports a clear error when a discovered file cannot be
/// read; missing roots are silently skipped.
pub fn discover_flaky_corpus(
    source_roots: &[PathBuf],
    retry_json: Option<&NextestRetryJson>,
) -> Result<FlakyCorpus, String> {
    let mut marker_count = 0u32;
    for path in walk_files_matching(source_roots, is_flaky_scannable) {
        let contents = read_source_file(&path, "source")?;
        marker_count = marker_count.saturating_add(count_flaky_markers(&contents));
    }
    Ok(FlakyCorpus {
        marker_count,
        retry_events: retry_json.map(|r| r.retry_count).unwrap_or(0),
    })
}

/// Read the `--nextest-retry-json` file if present, returning `None`
/// when the flag was omitted.
pub fn read_nextest_retry_json(path: Option<&Path>) -> Result<Option<NextestRetryJson>, String> {
    let Some(path) = path else { return Ok(None) };
    let bytes = fs::read(path).map_err(|e| {
        format!(
            "aggregate: cannot read --nextest-retry-json {}: {e}",
            path.display()
        )
    })?;
    serde_json::from_slice::<NextestRetryJson>(&bytes)
        .map(Some)
        .map_err(|e| {
            format!(
                "aggregate: --nextest-retry-json {} is not a valid NextestRetryJson: {e}",
                path.display()
            )
        })
}

/// Render the inline failure detail for a Red flaky-population row.
fn flaky_failure_detail(marker_count: u32, fail_threshold: u32) -> String {
    format!(
        "FLAKY marker count is {marker_count} — at or above the {fail_threshold} fail threshold."
    )
}

/// Format the `delta_text` for a wired flaky-population row.
fn flaky_delta_text(corpus: &FlakyCorpus) -> String {
    if corpus.retry_events == 0 {
        format!("{} markers", corpus.marker_count)
    } else {
        format!(
            "{} markers / {} retries",
            corpus.marker_count, corpus.retry_events
        )
    }
}

/// Build a wired `Row::FlakyPopulation` from a corpus + thresholds.
pub fn build_flaky_population_row(
    corpus: &FlakyCorpus,
    thresholds: &FlakyPopulationThresholds,
) -> Row {
    let common = RowCommon {
        id: "flaky_population".into(),
        label: "Flaky markers".into(),
        anchor: "flaky-population".into(),
    };
    let delta_text = flaky_delta_text(corpus);
    match threshold::resolve_flaky_population(corpus.marker_count, thresholds) {
        Status::Green => Row::flaky_population_green(
            common,
            corpus.marker_count,
            corpus.retry_events,
            delta_text,
        ),
        Status::Yellow => Row::flaky_population_yellow(
            common,
            corpus.marker_count,
            corpus.retry_events,
            delta_text,
        ),
        Status::Red => Row::flaky_population_red(
            common,
            corpus.marker_count,
            corpus.retry_events,
            delta_text,
            flaky_failure_detail(corpus.marker_count, thresholds.fail_marker_count),
        ),
    }
}

// ── Changed-scope diagram producer ─────────────────────────────────────
//
// V4 (#769) §4 wired row. The producer takes a newline-delimited list
// of paths changed between base and head (typically generated by
// `git diff --name-only origin/main...HEAD` in the CI workflow),
// groups them by crate / app, and emits a Mermaid `graph LR` snippet
// where each touched crate/app is a node connected to a virtual
// "changed scope" hub. V4 ships the simple grouping; richer DAG
// integration (depcruise + cargo-metadata) is a follow-up against
// #650 once the surface is exercised.

/// Maximum number of nodes the Mermaid diagram renders before
/// truncating. Real PRs rarely touch >200 distinct crates/apps; the
/// guard keeps a runaway PR from generating a comment GitHub refuses
/// to render.
const CHANGED_SCOPE_NODE_LIMIT: usize = 200;

/// Aggregated changed-file list. Pure-data input to
/// [`build_changed_scope_row`].
#[derive(Debug, Default, Clone)]
pub struct ChangedScope {
    /// Crate / app names touched by the PR, sorted ascending. Each
    /// name appears once even when the PR touches multiple files in
    /// it.
    pub touched: Vec<String>,
    /// `true` when the underlying input listed more changed files than
    /// [`CHANGED_SCOPE_NODE_LIMIT`] distinct crates; the Mermaid body
    /// includes a truncation footer when set.
    pub truncated: bool,
}

/// Project a list of changed paths (one per line, untrimmed) into a
/// [`ChangedScope`]. Pure function — exposed for unit tests so the
/// projection can be exercised without touching the filesystem.
///
/// Paths outside `crates/` and `apps/` (root files, docs, `.github/`,
/// …) are dropped — the diagram's purpose is to show *which crates*
/// the PR touches, and bucketing every workflow tweak into an
/// `unknown` node would actively mislead reviewers.
fn project_changed_scope(body: &str) -> ChangedScope {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(name) = crate_name_from_path(Path::new(trimmed)) {
            seen.insert(name);
        }
    }
    let truncated = seen.len() > CHANGED_SCOPE_NODE_LIMIT;
    let touched: Vec<String> = seen.into_iter().take(CHANGED_SCOPE_NODE_LIMIT).collect();
    ChangedScope { touched, truncated }
}

/// Read a newline-delimited changed-files list from `path`, group
/// paths by crate / app, and return a [`ChangedScope`].
///
/// Returns `None` when `path` is `None` (caller treats this as
/// `(no diff)`). Reports a clear error when the file is supplied but
/// cannot be read.
pub fn read_changed_scope(path: Option<&Path>) -> Result<Option<ChangedScope>, String> {
    let Some(path) = path else { return Ok(None) };
    let body = fs::read_to_string(path).map_err(|e| {
        format!(
            "aggregate: cannot read --changed-files {}: {e}",
            path.display()
        )
    })?;
    Ok(Some(project_changed_scope(&body)))
}

/// Render the Mermaid `graph LR` body for a [`ChangedScope`].
///
/// Empty scope yields `graph LR\n  empty[(no diff)]` so the row
/// always carries a non-empty Mermaid body — operators see the row
/// shape even when the PR does not change any tracked file.
fn mermaid_empty_scope() -> String {
    "graph LR\n  empty[\"(no diff)\"]".to_string()
}

fn mermaid_truncation_footer() -> String {
    format!(
        "  changed --> truncated[\"(diagram truncated at {CHANGED_SCOPE_NODE_LIMIT} nodes — see workflow logs)\"]\n",
    )
}

fn render_changed_scope_mermaid(scope: &ChangedScope) -> String {
    if scope.touched.is_empty() {
        return mermaid_empty_scope();
    }
    let mut out = String::from("graph LR\n  changed[\"Changed scope\"]\n");
    for (i, name) in scope.touched.iter().enumerate() {
        // Mermaid node ids must be alphanumeric / underscore. Keep the
        // pretty crate name in the label and use a deterministic id.
        out.push_str(&format!("  changed --> n{i}[\"{name}\"]\n"));
    }
    if scope.truncated {
        out.push_str(&mermaid_truncation_footer());
    }
    // Trailing newline trimmed for the wire shape — the renderer wraps
    // the body in a fenced block, no trailing whitespace needed.
    out.trim_end().to_string()
}

/// Format the `delta_text` for a wired changed-scope row.
fn changed_scope_delta_text(scope: &ChangedScope) -> String {
    if scope.touched.is_empty() {
        "(no diff)".to_string()
    } else if scope.truncated {
        format!(
            "{} crates touched (truncated at {})",
            scope.touched.len(),
            CHANGED_SCOPE_NODE_LIMIT
        )
    } else {
        format!("{} crates touched", scope.touched.len())
    }
}

/// Build a wired `Row::ChangedScopeDiagram` from a [`ChangedScope`].
///
/// V4 surfaces the row as informational — Status is always Green. A
/// future tightening could resolve Yellow/Red on dependency-DAG fan-
/// out (depcruise + cargo-metadata follow-ups), but that requires
/// inputs the producer does not yet consume.
pub fn build_changed_scope_row(scope: &ChangedScope) -> Row {
    let common = RowCommon {
        id: "changed_scope".into(),
        label: "Changed scope".into(),
        anchor: "changed-scope".into(),
    };
    let mermaid_md = render_changed_scope_mermaid(scope);
    let node_count = u32::try_from(scope.touched.len()).unwrap_or(u32::MAX);
    let delta_text = changed_scope_delta_text(scope);
    Row::changed_scope_diagram_green(common, mermaid_md, node_count, delta_text)
}

/// Build the scorecard artifact from parsed PR metadata, raw
/// measurements, and the resolved threshold config.
///
/// Pure function: no I/O, no panics, deterministic. `fallback_active`
/// records whether the supplied [`ThresholdConfig`] came from
/// [`ThresholdConfig::fallback`] (no operator config) so the renderer
/// can surface the starter-wheels affordance.
///
/// The argument list grows with each new wired producer — V4 lands
/// four real producers, pushing the count past clippy's seven-arg
/// soft cap. Bundling them into a `BuilderInputs` struct is a future
/// ergonomics pass; for now the explicit signature keeps each input
/// visible at the call site, which is what reviewers want when
/// diagnosing a wrong-row-data issue.
#[allow(clippy::too_many_arguments)]
pub fn build_scorecard(
    pr: PrMeta,
    coverage_delta_pp: f64,
    bdd_summary: &BddSummary,
    ci_wall_clock: Option<&CiWallClockJson>,
    flaky_corpus: &FlakyCorpus,
    changed_scope: Option<&ChangedScope>,
    thresholds: &ThresholdConfig,
    fallback_active: bool,
) -> Scorecard {
    let coverage = build_coverage_row(coverage_delta_pp, &thresholds.rows.coverage);
    let bdd_feature = build_bdd_feature_skip_row(bdd_summary, &thresholds.rows.bdd_feature_skip);
    let bdd_scenario = build_bdd_scenario_skip_row(bdd_summary, &thresholds.rows.bdd_scenario_skip);
    // Absent CI wall-clock JSON: emit Green row with zero values. The
    // row is informational until the workflow step starts uploading the
    // artifact (C8); operators see the slot occupied either way.
    let ci_wall_clock_default = CiWallClockJson {
        total_seconds: 0.0,
        base_total_seconds: None,
    };
    let ci_wall_clock_input = ci_wall_clock.unwrap_or(&ci_wall_clock_default);
    let ci_wall_clock_row =
        build_ci_wall_clock_row(ci_wall_clock_input, &thresholds.rows.ci_wall_clock);
    let flaky = build_flaky_population_row(flaky_corpus, &thresholds.rows.flaky);
    let changed_scope_default = ChangedScope::default();
    let changed_scope_input = changed_scope.unwrap_or(&changed_scope_default);
    let changed_scope_row = build_changed_scope_row(changed_scope_input);

    // Producer-blocked rows ship as Green stubs pinned to their
    // upstream producer references. The renderer detects the
    // [`PENDING_TEXT_PREFIX`] sentinel and inlines the cell with the
    // GitHub-autolinked issue reference. Replacing a stub is a small
    // one-PR follow-up against #650 — see the issue's closure model.
    let rows = vec![
        coverage,
        bdd_feature,
        bdd_scenario,
        ci_wall_clock_row,
        flaky,
        changed_scope_row,
        stub_crap_delta_pending(),
        stub_mutation_survivors_pending(),
        stub_handler_coverage_axis_pending(),
        stub_gate_runs_pending(),
    ];

    // Single-source overall_status rollup. Worst-of across all rows
    // (Red > Yellow > Green); stub rows are Green by construction so
    // they cannot mask a wired row's verdict.
    let overall_status = rows
        .iter()
        .map(row_status)
        .fold(Status::Green, Status::worst_of);

    let head_sha = pr.head_sha.clone();
    let all_check_runs_url =
        format!("https://github.com/breezy-bays-labs/mokumo/commit/{head_sha}/checks");

    Scorecard {
        schema_version: SCHEMA_VERSION,
        pr,
        overall_status,
        rows,
        top_failures: Vec::new(),
        all_check_runs_url,
        fallback_thresholds_active: fallback_active,
    }
}

/// Wire `schema_version` emitted by the producer.
///
/// Bumped to `2` in V4 (#769) when the v0 row inventory landed in full
/// — eight net-new variants joined the existing `CoverageDelta`. The
/// renderer's degradation-notice path triggers when a renderer pinned
/// to an earlier version sees a newer artifact.
///
/// The migration playbook lives in
/// `decisions/mokumo/adr-scorecard-crate-shape.md`; bumps are an
/// additive-rejection event paired with the matching renderer-side
/// catch-up.
pub const SCHEMA_VERSION: u32 = 2;

/// Outcome of resolving operator thresholds from a `--quality-toml` path.
///
/// The pair `(config, fallback_active)` flows directly into
/// [`build_scorecard`]; the renderer keys off `fallback_active` to
/// surface the starter-wheels affordance. Surfacing the source as a
/// distinct enum (rather than a bare bool) makes intent legible at the
/// call site and keeps the fallback semantics consistent across
/// callers (CLI today, BDD step-defs in a later slice).
#[derive(Debug)]
pub enum ThresholdSource {
    /// The operator config at `path` was read and parsed successfully.
    Configured {
        config: ThresholdConfig,
        path: PathBuf,
    },
    /// The operator config at `path` was not present on disk; the
    /// producer fell back to [`ThresholdConfig::fallback`].
    Fallback { path: PathBuf },
}

impl ThresholdSource {
    /// Borrow the resolved [`ThresholdConfig`]. Configured sources
    /// return their parsed config; fallback sources mint
    /// [`ThresholdConfig::fallback`] on demand.
    pub fn config(&self) -> ThresholdConfig {
        match self {
            ThresholdSource::Configured { config, .. } => config.clone(),
            ThresholdSource::Fallback { .. } => ThresholdConfig::fallback(),
        }
    }

    /// `true` when the producer is using the hardcoded fallback
    /// thresholds rather than an operator-tuned config. Flows into the
    /// `fallback_thresholds_active` artifact field.
    pub fn fallback_active(&self) -> bool {
        matches!(self, ThresholdSource::Fallback { .. })
    }
}

/// Resolve the [`ThresholdConfig`] for a producer run from the
/// `--quality-toml` path.
///
/// Four outcomes:
/// - File present, non-empty, parses → [`ThresholdSource::Configured`].
/// - File absent (any [`std::io::ErrorKind::NotFound`] from
///   `fs::read`) → [`ThresholdSource::Fallback`]. Operators who never
///   write a `quality.toml` and just want the starter-wheel verdict
///   get a green CI run, not an error.
/// - File present but empty (zero bytes, or only whitespace) →
///   [`ThresholdSource::Fallback`]. The schema rejects an empty file
///   because `[rows.coverage]` is required; treating empty as absent
///   keeps the operator-facing contract aligned with the rendered
///   surface ("absent or empty falls back to hardcoded thresholds").
/// - File present but unreadable, invalid UTF-8, or unparseable as TOML
///   → `Err(...)` with a message naming the path and the underlying
///   cause. Fail-loud so a typo never silently degrades to fallback.
pub fn resolve_threshold_source(path: &Path) -> Result<ThresholdSource, String> {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(ThresholdSource::Fallback {
            path: path.to_path_buf(),
        }),
        Ok(text) => {
            let config = threshold::parse_quality_toml(&text).map_err(|e| {
                format!(
                    "aggregate: --quality-toml {} failed to parse: {e}",
                    path.display()
                )
            })?;
            validate_threshold_config(&config).map_err(|e| {
                format!(
                    "aggregate: --quality-toml {} has invalid thresholds: {e}",
                    path.display()
                )
            })?;
            Ok(ThresholdSource::Configured {
                config,
                path: path.to_path_buf(),
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ThresholdSource::Fallback {
            path: path.to_path_buf(),
        }),
        Err(e) => Err(format!(
            "aggregate: --quality-toml {} could not be read: {e}",
            path.display()
        )),
    }
}

/// Reject operator-tuned threshold configs that would silently break
/// the verdict surface even though they parse cleanly.
///
/// TOML's `nan` / `inf` / `-inf` literals all deserialize into `f64`
/// without complaint, but a non-finite warn or fail threshold makes
/// the comparison rules in [`threshold::resolve_coverage_delta`]
/// nonsensical: NaN comparisons are always false (everything resolves
/// Green) and infinity collapses one of the two transitions.
///
/// Likewise, `fail_pp_delta` greater than `warn_pp_delta` is logically
/// inverted: the resolver's `delta_pp <= warn_pp_delta` first arm
/// would catch the Red case before the Yellow check ran, making
/// Yellow unreachable and turning ordinary regressions Red.
///
/// Both cases produce a verdict the operator did not ask for. Loud-fail
/// at config-load time keeps the producer honest.
fn validate_threshold_config(config: &ThresholdConfig) -> Result<(), String> {
    let coverage = &config.rows.coverage;
    if !coverage.warn_pp_delta.is_finite() {
        return Err(format!(
            "rows.coverage.warn_pp_delta must be finite, got {}",
            coverage.warn_pp_delta
        ));
    }
    if !coverage.fail_pp_delta.is_finite() {
        return Err(format!(
            "rows.coverage.fail_pp_delta must be finite, got {}",
            coverage.fail_pp_delta
        ));
    }
    if coverage.fail_pp_delta > coverage.warn_pp_delta {
        return Err(format!(
            "rows.coverage.fail_pp_delta ({}) must be <= warn_pp_delta ({}); \
             with fail above warn, Yellow is unreachable and ordinary regressions land Red",
            coverage.fail_pp_delta, coverage.warn_pp_delta
        ));
    }

    // Integer-count thresholds: every resolver here treats the row as
    // worse when the measured count is *higher*, so `fail` must be at
    // or above `warn`. An inverted pair makes Yellow unreachable —
    // a measured value crossing `fail` resolves Red before the Yellow
    // arm runs. Loud-fail at config-load time so an operator typo
    // never silently shifts the verdict.
    let bf = &config.rows.bdd_feature_skip;
    if bf.fail_skipped_features < bf.warn_skipped_features {
        return Err(format!(
            "rows.bdd_feature_skip.fail_skipped_features ({}) must be >= warn_skipped_features ({}); \
             with fail below warn, Yellow is unreachable",
            bf.fail_skipped_features, bf.warn_skipped_features
        ));
    }
    let bs = &config.rows.bdd_scenario_skip;
    if bs.fail_skipped_scenarios < bs.warn_skipped_scenarios {
        return Err(format!(
            "rows.bdd_scenario_skip.fail_skipped_scenarios ({}) must be >= warn_skipped_scenarios ({}); \
             with fail below warn, Yellow is unreachable",
            bs.fail_skipped_scenarios, bs.warn_skipped_scenarios
        ));
    }
    let fl = &config.rows.flaky;
    if fl.fail_marker_count < fl.warn_marker_count {
        return Err(format!(
            "rows.flaky.fail_marker_count ({}) must be >= warn_marker_count ({}); \
             with fail below warn, Yellow is unreachable",
            fl.fail_marker_count, fl.warn_marker_count
        ));
    }

    // CI wall-clock thresholds are signed seconds. A positive delta is
    // a slowdown, so the resolver flags Red when measured >= fail and
    // Yellow when measured >= warn — same monotonicity rule.
    let ci = &config.rows.ci_wall_clock;
    if !ci.warn_seconds_delta.is_finite() {
        return Err(format!(
            "rows.ci_wall_clock.warn_seconds_delta must be finite, got {}",
            ci.warn_seconds_delta
        ));
    }
    if !ci.fail_seconds_delta.is_finite() {
        return Err(format!(
            "rows.ci_wall_clock.fail_seconds_delta must be finite, got {}",
            ci.fail_seconds_delta
        ));
    }
    if ci.fail_seconds_delta < ci.warn_seconds_delta {
        return Err(format!(
            "rows.ci_wall_clock.fail_seconds_delta ({}) must be >= warn_seconds_delta ({}); \
             with fail below warn, Yellow is unreachable",
            ci.fail_seconds_delta, ci.warn_seconds_delta
        ));
    }

    Ok(())
}

/// Read + parse `--pr-meta`. Returns a clear error message on missing
/// file / invalid JSON / shape mismatch.
pub fn read_pr_meta(path: &Path) -> Result<PrMeta, String> {
    let bytes = fs::read(path)
        .map_err(|e| format!("aggregate: cannot read --pr-meta {}: {e}", path.display()))?;
    serde_json::from_slice::<PrMeta>(&bytes).map_err(|e| {
        format!(
            "aggregate: --pr-meta {} is not a valid PrMeta JSON: {e}",
            path.display()
        )
    })
}

/// Validate the serialized scorecard against the committed schema.
/// Layer-2 defense-in-depth — drift between the Rust source and the
/// committed schema fails the run before the artifact ever leaves the
/// producer.
pub fn validate_against_schema(value: &Value) -> Result<(), String> {
    let schema_value: Value = serde_json::from_str(COMMITTED_SCHEMA)
        .map_err(|e| format!("aggregate: embedded schema is not valid JSON: {e}"))?;
    let compiled = JSONSchema::compile(&schema_value)
        .map_err(|e| format!("aggregate: failed to compile committed schema: {e}"))?;
    let result = compiled.validate(value);
    if let Err(errors) = result {
        let messages: Vec<String> = errors
            .map(|e| format!("  at {}: {e}", e.instance_path))
            .collect();
        return Err(format!(
            "aggregate: scorecard output failed schema validation:\n{}",
            messages.join("\n")
        ));
    }
    Ok(())
}

/// Ensure the parent directory of `path` exists. No-op when the path has
/// no parent or the parent is the current directory.
fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    fs::create_dir_all(parent).map_err(|e| {
        format!(
            "aggregate: failed to create parent dir {}: {e}",
            parent.display()
        )
    })
}

/// Serialize the scorecard to a string after passing the schema check.
fn render_scorecard(scorecard: &Scorecard) -> Result<String, String> {
    let value = serde_json::to_value(scorecard)
        .map_err(|e| format!("aggregate: failed to serialize scorecard: {e}"))?;
    validate_against_schema(&value)?;
    let mut pretty = serde_json::to_string_pretty(&value)
        .map_err(|e| format!("aggregate: failed to render scorecard: {e}"))?;
    pretty.push('\n');
    Ok(pretty)
}

/// Serialize the scorecard to `--out` atomically (write to a temp
/// sibling + rename), creating parent dirs as needed, after passing
/// the schema check.
///
/// Atomicity matters because the artifact is consumed by the renderer
/// out-of-process; a partial write from an interrupted run (CI cancel,
/// disk full, signal) would otherwise leave the renderer parsing
/// truncated JSON and posting a confusing fail-closed comment.
pub fn write_scorecard(scorecard: &Scorecard, out_path: &Path) -> Result<(), String> {
    let content = render_scorecard(scorecard)?;
    ensure_parent_dir(out_path)?;
    let tmp_path = tmp_sibling(out_path);
    fs::write(&tmp_path, content)
        .map_err(|e| format!("aggregate: failed to write {}: {e}", tmp_path.display()))?;
    fs::rename(&tmp_path, out_path).map_err(|e| {
        // Best-effort tmp cleanup; ignore errors (the rename failure is
        // the actionable signal).
        let _ = fs::remove_file(&tmp_path);
        format!(
            "aggregate: failed to rename {} -> {}: {e}",
            tmp_path.display(),
            out_path.display()
        )
    })
}

/// Compute the temp-file sibling path used by [`write_scorecard`]. We
/// keep the same parent dir so `rename` stays on the same filesystem
/// (cross-device renames silently fall back to copy+delete on some
/// kernels — atomicity is lost). Suffix is `.tmp` plus the process id
/// so two parallel `aggregate` invocations writing different `--out`
/// paths sharing a parent don't collide.
fn tmp_sibling(out_path: &Path) -> PathBuf {
    let pid = std::process::id();
    let mut tmp = out_path.as_os_str().to_owned();
    tmp.push(format!(".tmp.{pid}"));
    PathBuf::from(tmp)
}

/// Parse CLI args from raw OS args. Returns the parsed [`Cli`] or an
/// [`ExitCode`] for clap-rendered usage / help / version output.
fn parse_cli(args: impl IntoIterator<Item = OsString>) -> Result<Cli, ExitCode> {
    match Cli::try_parse_from(std::iter::once(OsString::from("aggregate")).chain(args)) {
        Ok(c) => Ok(c),
        Err(e) => {
            // clap renders `--help`/`--version`/usage errors via Display.
            eprint!("{e}");
            // `--help` and `--version` are successes per GNU convention;
            // every other arg-failure is exit 2.
            Err(if e.use_stderr() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}

/// Bundle of producer inputs gathered from the CLI flags. Returned
/// by [`gather_producer_inputs`] so [`run`] stays a thin orchestrator
/// (parse → gather → build → write) instead of an eight-arm error
/// dispatcher.
struct ProducerInputs {
    pr: PrMeta,
    bdd: BddSummary,
    ci_wall_clock: Option<CiWallClockJson>,
    flaky: FlakyCorpus,
    changed_scope: Option<ChangedScope>,
    threshold_source: ThresholdSource,
}

/// Composite error coupling a human-readable message with the exit
/// code the CLI should return. The exit code distinguishes
/// usage-failures (exit 2 — bad `--pr-meta` etc.) from runtime
/// failures (exit 1 — I/O / parse / schema).
type CliError = (String, u8);

fn usage_err(msg: String) -> CliError {
    (msg, 2)
}

fn runtime_err(msg: String) -> CliError {
    (msg, 1)
}

/// Read every flag-driven producer input. Each step short-circuits
/// to a `(message, exit_code)` pair — `run` prints the message and
/// returns the exit code without further branching.
fn gather_producer_inputs(cli: &Cli) -> Result<ProducerInputs, CliError> {
    let pr = read_pr_meta(&cli.pr_meta).map_err(usage_err)?;
    let threshold_source = resolve_threshold_source(&cli.quality_toml).map_err(runtime_err)?;
    let bdd = discover_bdd_corpus(&cli.bdd_features_roots).map_err(runtime_err)?;
    let ci_wall_clock =
        read_ci_wall_clock_json(cli.ci_wall_clock_json.as_deref()).map_err(runtime_err)?;
    let nextest_retry =
        read_nextest_retry_json(cli.nextest_retry_json.as_deref()).map_err(runtime_err)?;
    let flaky = discover_flaky_corpus(&cli.flaky_source_roots, nextest_retry.as_ref())
        .map_err(runtime_err)?;
    let changed_scope = read_changed_scope(cli.changed_files.as_deref()).map_err(runtime_err)?;
    Ok(ProducerInputs {
        pr,
        bdd,
        ci_wall_clock,
        flaky,
        changed_scope,
        threshold_source,
    })
}

/// Drive the CLI from raw OS args. Extracted for testability.
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let cli = match parse_cli(args) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let inputs = match gather_producer_inputs(&cli) {
        Ok(i) => i,
        Err((msg, code)) => {
            eprintln!("{msg}");
            return ExitCode::from(code);
        }
    };
    let scorecard = build_scorecard(
        inputs.pr,
        cli.coverage_delta_pp,
        &inputs.bdd,
        inputs.ci_wall_clock.as_ref(),
        &inputs.flaky,
        inputs.changed_scope.as_ref(),
        &inputs.threshold_source.config(),
        inputs.threshold_source.fallback_active(),
    );
    if let Err(msg) = write_scorecard(&scorecard, &cli.out) {
        eprintln!("{msg}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests assert exact deserialised literals, not float arithmetic results"
)]
mod tests {
    use super::*;

    fn pr_meta() -> PrMeta {
        PrMeta {
            pr_number: 763.into(),
            head_sha: "abc123".into(),
            base_sha: "def456".into(),
            is_fork: false,
        }
    }

    fn fallback() -> ThresholdConfig {
        ThresholdConfig::fallback()
    }

    fn build_with_delta(delta_pp: f64) -> Scorecard {
        build_scorecard(
            pr_meta(),
            delta_pp,
            &BddSummary::default(),
            None,
            &FlakyCorpus::default(),
            None,
            &fallback(),
            true,
        )
    }

    #[test]
    fn build_scorecard_emits_coverage_row_first() {
        // V4 emits the coverage row + five wired rows
        // (BddFeatureLevelSkipped, BddScenarioLevelSkipped,
        // CiWallClockDelta, FlakyPopulation, ChangedScopeDiagram) +
        // four producer-pending stubs (CrapDelta, MutationSurvivors,
        // HandlerCoverageAxis, GateRuns) — ten rows total.
        let sc = build_with_delta(0.3);
        assert_eq!(sc.rows.len(), 10);
        let Row::CoverageDelta {
            status,
            delta_pp,
            delta_text,
            ..
        } = &sc.rows[0]
        else {
            panic!("expected CoverageDelta as the first row")
        };
        assert_eq!(*status, Status::Green);
        assert_eq!(*delta_pp, 0.3);
        assert_eq!(delta_text, "+0.3 pp");
    }

    #[test]
    fn build_scorecard_emits_bdd_feature_skip_row_after_coverage() {
        // BDD feature-skip is the second row — wired in C9.
        // Empty summary lands Green.
        let sc = build_with_delta(0.3);
        let Row::BddFeatureLevelSkipped {
            status,
            total_features,
            skipped_features,
            ..
        } = &sc.rows[1]
        else {
            panic!("expected BddFeatureLevelSkipped as the second row")
        };
        assert_eq!(*status, Status::Green);
        assert_eq!(*total_features, 0);
        assert_eq!(*skipped_features, 0);
    }

    #[test]
    fn build_scorecard_emits_bdd_scenario_skip_row_third() {
        // BDD scenario-skip is the third row — paired with the
        // feature-skip row to surface both backlog and hygiene signals.
        let sc = build_with_delta(0.3);
        let Row::BddScenarioLevelSkipped {
            status,
            total_scenarios,
            skipped_scenarios,
            ..
        } = &sc.rows[2]
        else {
            panic!("expected BddScenarioLevelSkipped as the third row")
        };
        assert_eq!(*status, Status::Green);
        assert_eq!(*total_scenarios, 0);
        assert_eq!(*skipped_scenarios, 0);
    }

    #[test]
    fn build_scorecard_overall_status_rolls_up_worst_of_rows() {
        // Stub rows are Green by construction so they cannot mask the
        // wired CoverageDelta verdict — the rollup mirrors the
        // CoverageDelta row's status across all three branches.
        assert_eq!(build_with_delta(0.5).overall_status, Status::Green);
        assert_eq!(build_with_delta(-2.5).overall_status, Status::Yellow);
        assert_eq!(build_with_delta(-6.0).overall_status, Status::Red);
    }

    #[test]
    fn build_scorecard_emits_producer_pending_stubs() {
        let sc = build_with_delta(0.3);
        let pending: Vec<_> = sc
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::CrapDelta { delta_text, .. }
                | Row::MutationSurvivors { delta_text, .. }
                | Row::HandlerCoverageAxis { delta_text, .. }
                | Row::GateRuns { delta_text, .. } => Some(delta_text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(pending.len(), 4);
        for text in pending {
            assert!(
                text.starts_with(PENDING_TEXT_PREFIX),
                "stub row delta_text must start with PENDING_TEXT_PREFIX, got: {text}"
            );
        }
    }

    #[test]
    fn build_scorecard_marks_fallback_thresholds_active() {
        // The `fallback_active` argument round-trips into the produced
        // artifact's `fallback_thresholds_active` field — that's the
        // flag the renderer keys off when it decides whether to surface
        // the starter-wheels preamble + HTML markers.
        let sc = build_with_delta(-2.5);
        assert!(sc.fallback_thresholds_active);
    }

    #[test]
    fn build_scorecard_records_fallback_active_false_when_passed() {
        // Independent test of the parameter — `false` flows through
        // the same path so a contributor cannot accidentally hardwire
        // the field to `true` and pass the previous test by coincidence.
        let sc = build_scorecard(
            pr_meta(),
            -2.5,
            &BddSummary::default(),
            None,
            &FlakyCorpus::default(),
            None,
            &fallback(),
            false,
        );
        assert!(!sc.fallback_thresholds_active);
    }

    #[test]
    fn build_scorecard_red_row_carries_failure_detail() {
        let sc = build_with_delta(-7.5);
        let Row::CoverageDelta {
            status,
            failure_detail_md,
            ..
        } = &sc.rows[0]
        else {
            panic!("expected CoverageDelta")
        };
        assert_eq!(*status, Status::Red);
        let detail = failure_detail_md
            .as_ref()
            .expect("Red rows carry failure_detail_md by Layer-1 invariant");
        assert!(detail.contains("7.5 pp"), "got: {detail}");
        assert!(detail.contains("5.0 pp"), "got: {detail}");
    }

    #[test]
    fn build_scorecard_url_uses_https_and_head_sha() {
        let sc = build_with_delta(0.0);
        assert!(sc.all_check_runs_url.starts_with("https://"));
        assert!(sc.all_check_runs_url.contains("abc123"));
    }

    #[test]
    fn build_scorecard_validates_against_committed_schema_for_all_three_branches() {
        // Layer 2 defense: every branch the producer can mint must
        // pass schema validation. Catches a future field addition that
        // forgets to add `failure_detail_md` to a Red branch.
        for delta in [0.5_f64, -2.5, -7.5] {
            let sc = build_with_delta(delta);
            let value = serde_json::to_value(&sc).expect("serialize");
            validate_against_schema(&value)
                .unwrap_or_else(|e| panic!("schema validation failed for delta={delta}: {e}"));
        }
    }

    #[test]
    fn validate_rejects_invalid_overall_status() {
        let sc = build_with_delta(0.0);
        let mut value = serde_json::to_value(&sc).expect("serialize");
        value["overall_status"] = serde_json::json!("Magenta");
        let err = validate_against_schema(&value).unwrap_err();
        assert!(err.contains("schema validation"), "got: {err}");
    }

    #[test]
    fn format_delta_text_signs_match_direction() {
        assert_eq!(format_delta_text(0.3), "+0.3 pp");
        assert_eq!(format_delta_text(-2.5), "-2.5 pp");
        assert_eq!(format_delta_text(0.0), "+0.0 pp");
        assert_eq!(format_delta_text(-7.5), "-7.5 pp");
    }

    #[test]
    fn coverage_failure_detail_reports_absolute_drop_and_threshold() {
        let detail = coverage_failure_detail(-6.2, -5.0);
        assert!(detail.contains("6.2 pp"), "got: {detail}");
        assert!(detail.contains("5.0 pp"), "got: {detail}");
        // The wording must match the resolver's inclusive boundary —
        // a delta exactly at `fail_pp_delta` lands Red, so the detail
        // says "at or below" rather than "below".
        assert!(detail.contains("at or below"), "got: {detail}");
    }

    // ── --quality-toml resolution ──────────────────────────────────

    #[test]
    fn resolve_threshold_source_returns_fallback_for_missing_file() {
        let dir = tempdir();
        let missing = dir.path.join("does-not-exist.toml");
        let source = resolve_threshold_source(&missing).expect("absent file is fallback");
        assert!(source.fallback_active());
        let cfg = source.config();
        assert_eq!(cfg.rows.coverage.warn_pp_delta, -1.0);
        assert_eq!(cfg.rows.coverage.fail_pp_delta, -5.0);
    }

    #[test]
    fn resolve_threshold_source_returns_fallback_for_empty_file() {
        // Operator surface contract: "absent or empty falls back to
        // hardcoded thresholds". Without this branch, `parse_quality_toml`
        // rejects empty input because `[rows.coverage]` is required, and
        // an operator who creates the file but hasn't filled it in yet
        // gets a loud parse failure instead of the starter-wheels verdict.
        let dir = tempdir();
        let path = dir.path.join("empty.toml");
        fs::write(&path, "").expect("write empty");
        let source = resolve_threshold_source(&path).expect("empty file is fallback");
        assert!(source.fallback_active());
        let cfg = source.config();
        assert_eq!(cfg.rows.coverage.warn_pp_delta, -1.0);
        assert_eq!(cfg.rows.coverage.fail_pp_delta, -5.0);
    }

    #[test]
    fn resolve_threshold_source_returns_fallback_for_whitespace_only_file() {
        // Same contract as above — a file with only blank lines /
        // comments-without-tables / spaces is operationally empty.
        let dir = tempdir();
        let path = dir.path.join("whitespace.toml");
        fs::write(&path, "\n   \n\t\n").expect("write whitespace");
        let source = resolve_threshold_source(&path).expect("whitespace file is fallback");
        assert!(source.fallback_active());
    }

    #[test]
    fn resolve_threshold_source_parses_well_formed_toml() {
        let dir = tempdir();
        let path = dir.path.join("quality.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -0.5\nfail_pp_delta = -3.0\n",
        )
        .expect("write");
        let source = resolve_threshold_source(&path).expect("parse");
        assert!(!source.fallback_active());
        let cfg = source.config();
        assert_eq!(cfg.rows.coverage.warn_pp_delta, -0.5);
        assert_eq!(cfg.rows.coverage.fail_pp_delta, -3.0);
    }

    #[test]
    fn resolve_threshold_source_rejects_inverted_thresholds() {
        // fail above warn would make Yellow unreachable; loud-fail.
        let dir = tempdir();
        let path = dir.path.join("inverted.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -5.0\nfail_pp_delta = -1.0\n",
        )
        .expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(err.contains("inverted.toml"), "got: {err}");
        assert!(err.contains("Yellow is unreachable"), "got: {err}");
    }

    #[test]
    fn resolve_threshold_source_rejects_non_finite_warn() {
        let dir = tempdir();
        let path = dir.path.join("nan.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = nan\nfail_pp_delta = -5.0\n",
        )
        .expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(err.contains("warn_pp_delta must be finite"), "got: {err}");
    }

    #[test]
    fn resolve_threshold_source_rejects_non_finite_fail() {
        let dir = tempdir();
        let path = dir.path.join("inf.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -1.0\nfail_pp_delta = -inf\n",
        )
        .expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(err.contains("fail_pp_delta must be finite"), "got: {err}");
    }

    #[test]
    fn resolve_threshold_source_rejects_inverted_bdd_feature_skip() {
        let dir = tempdir();
        let path = dir.path.join("bdd-feat-inv.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -1.0\nfail_pp_delta = -5.0\n\
             [rows.bdd_feature_skip]\nwarn_skipped_features = 15\nfail_skipped_features = 5\n",
        )
        .expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(
            err.contains("rows.bdd_feature_skip.fail_skipped_features"),
            "got: {err}"
        );
        assert!(err.contains("Yellow is unreachable"), "got: {err}");
    }

    #[test]
    fn resolve_threshold_source_rejects_inverted_bdd_scenario_skip() {
        let dir = tempdir();
        let path = dir.path.join("bdd-scen-inv.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -1.0\nfail_pp_delta = -5.0\n\
             [rows.bdd_scenario_skip]\nwarn_skipped_scenarios = 50\nfail_skipped_scenarios = 30\n",
        )
        .expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(
            err.contains("rows.bdd_scenario_skip.fail_skipped_scenarios"),
            "got: {err}"
        );
        assert!(err.contains("Yellow is unreachable"), "got: {err}");
    }

    #[test]
    fn resolve_threshold_source_rejects_inverted_flaky() {
        let dir = tempdir();
        let path = dir.path.join("flaky-inv.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -1.0\nfail_pp_delta = -5.0\n\
             [rows.flaky]\nwarn_marker_count = 30\nfail_marker_count = 10\n",
        )
        .expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(err.contains("rows.flaky.fail_marker_count"), "got: {err}");
        assert!(err.contains("Yellow is unreachable"), "got: {err}");
    }

    #[test]
    fn resolve_threshold_source_rejects_inverted_ci_wall_clock() {
        let dir = tempdir();
        let path = dir.path.join("ci-inv.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -1.0\nfail_pp_delta = -5.0\n\
             [rows.ci_wall_clock]\nwarn_seconds_delta = 300.0\nfail_seconds_delta = 60.0\n",
        )
        .expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(
            err.contains("rows.ci_wall_clock.fail_seconds_delta"),
            "got: {err}"
        );
        assert!(err.contains("Yellow is unreachable"), "got: {err}");
    }

    #[test]
    fn resolve_threshold_source_rejects_non_finite_ci_wall_clock_warn() {
        let dir = tempdir();
        let path = dir.path.join("ci-nan.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -1.0\nfail_pp_delta = -5.0\n\
             [rows.ci_wall_clock]\nwarn_seconds_delta = nan\nfail_seconds_delta = 300.0\n",
        )
        .expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(
            err.contains("rows.ci_wall_clock.warn_seconds_delta must be finite"),
            "got: {err}"
        );
    }

    #[test]
    fn resolve_threshold_source_rejects_non_finite_ci_wall_clock_fail() {
        let dir = tempdir();
        let path = dir.path.join("ci-inf.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -1.0\nfail_pp_delta = -5.0\n\
             [rows.ci_wall_clock]\nwarn_seconds_delta = 60.0\nfail_seconds_delta = inf\n",
        )
        .expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(
            err.contains("rows.ci_wall_clock.fail_seconds_delta must be finite"),
            "got: {err}"
        );
    }

    #[test]
    fn resolve_threshold_source_accepts_equal_warn_and_fail() {
        // `fail == warn` is allowed: the resolver's `delta_pp <= warn`
        // arm fires first, so a delta exactly at the threshold lands
        // Yellow, never Red. That's a degenerate but legal config —
        // it collapses Yellow into a single tripwire point and skips
        // straight to Red below it. Operators may want this for tight
        // gates; reject only the strict-inversion case.
        let dir = tempdir();
        let path = dir.path.join("equal.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -2.0\nfail_pp_delta = -2.0\n",
        )
        .expect("write");
        let source = resolve_threshold_source(&path).expect("equal thresholds are legal");
        assert!(!source.fallback_active());
    }

    #[test]
    fn resolve_threshold_source_errors_on_malformed_toml() {
        let dir = tempdir();
        let path = dir.path.join("bad.toml");
        fs::write(&path, "[rows.coverage]\nwarn_pp_delta = \"tight\"\n").expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(err.contains("bad.toml"), "got: {err}");
        assert!(err.contains("--quality-toml"), "got: {err}");
        assert!(err.contains("failed to parse"), "got: {err}");
    }

    #[test]
    fn resolve_threshold_source_errors_on_unknown_field() {
        let dir = tempdir();
        let path = dir.path.join("typo.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -1.0\nfail_pp_delta = -5.0\nfail_pp_dleta = -7.0\n",
        )
        .expect("write");
        let err = resolve_threshold_source(&path).unwrap_err();
        assert!(err.contains("unknown field"), "got: {err}");
    }

    #[test]
    fn configured_thresholds_flip_status_at_smaller_drop() {
        // Tightened-warn round trip: with `warn_pp_delta = -0.5`, a
        // drop of -0.8 lands Yellow even though it would land Green
        // under the fallback's -1.0 warn threshold.
        let dir = tempdir();
        let path = dir.path.join("tight.toml");
        fs::write(
            &path,
            "[rows.coverage]\nwarn_pp_delta = -0.5\nfail_pp_delta = -5.0\n",
        )
        .expect("write");
        let source = resolve_threshold_source(&path).expect("parse");
        let sc = build_scorecard(
            pr_meta(),
            -0.8,
            &BddSummary::default(),
            None,
            &FlakyCorpus::default(),
            None,
            &source.config(),
            source.fallback_active(),
        );
        assert_eq!(sc.overall_status, Status::Yellow);
        assert!(!sc.fallback_thresholds_active);
    }

    #[test]
    fn fallback_path_yields_yellow_at_two_point_five_drop() {
        // Absent-file round trip: a drop of -2.5 against the fallback
        // thresholds (warn = -1.0, fail = -5.0) lands Yellow and flags
        // the artifact as fallback-active.
        let dir = tempdir();
        let missing = dir.path.join("absent.toml");
        let source = resolve_threshold_source(&missing).expect("fallback");
        let sc = build_scorecard(
            pr_meta(),
            -2.5,
            &BddSummary::default(),
            None,
            &FlakyCorpus::default(),
            None,
            &source.config(),
            source.fallback_active(),
        );
        assert_eq!(sc.overall_status, Status::Yellow);
        assert!(sc.fallback_thresholds_active);
    }

    #[test]
    fn run_emits_configured_path_artifact_when_quality_toml_present() {
        let dir = tempdir();
        let pr_path = dir.path.join("pr.json");
        let toml_path = dir.path.join("quality.toml");
        let out_path = dir.path.join("scorecard.json");
        fs::write(
            &pr_path,
            r#"{"pr_number":1,"head_sha":"x","base_sha":"y","is_fork":false}"#,
        )
        .unwrap();
        fs::write(
            &toml_path,
            "[rows.coverage]\nwarn_pp_delta = -0.5\nfail_pp_delta = -5.0\n",
        )
        .unwrap();
        let code = run([
            OsString::from("--pr-meta"),
            OsString::from(pr_path.as_os_str()),
            OsString::from("--coverage-delta-pp"),
            OsString::from("-0.8"),
            OsString::from("--quality-toml"),
            OsString::from(toml_path.as_os_str()),
            OsString::from("--out"),
            OsString::from(out_path.as_os_str()),
        ]);
        assert_eq!(code, ExitCode::SUCCESS);
        let parsed: Value = serde_json::from_str(&fs::read_to_string(&out_path).unwrap()).unwrap();
        assert_eq!(parsed["overall_status"], "Yellow");
        assert_eq!(parsed["fallback_thresholds_active"], false);
    }

    #[test]
    fn run_emits_fallback_artifact_when_quality_toml_absent() {
        let dir = tempdir();
        let pr_path = dir.path.join("pr.json");
        let out_path = dir.path.join("scorecard.json");
        let missing_toml = dir.path.join("absent.toml");
        fs::write(
            &pr_path,
            r#"{"pr_number":1,"head_sha":"x","base_sha":"y","is_fork":false}"#,
        )
        .unwrap();
        let code = run([
            OsString::from("--pr-meta"),
            OsString::from(pr_path.as_os_str()),
            OsString::from("--coverage-delta-pp"),
            OsString::from("-2.5"),
            OsString::from("--quality-toml"),
            OsString::from(missing_toml.as_os_str()),
            OsString::from("--out"),
            OsString::from(out_path.as_os_str()),
        ]);
        assert_eq!(code, ExitCode::SUCCESS);
        let parsed: Value = serde_json::from_str(&fs::read_to_string(&out_path).unwrap()).unwrap();
        assert_eq!(parsed["overall_status"], "Yellow");
        assert_eq!(parsed["fallback_thresholds_active"], true);
    }

    #[test]
    fn run_returns_one_for_malformed_quality_toml() {
        let dir = tempdir();
        let pr_path = dir.path.join("pr.json");
        let toml_path = dir.path.join("bad.toml");
        let out_path = dir.path.join("scorecard.json");
        fs::write(
            &pr_path,
            r#"{"pr_number":1,"head_sha":"x","base_sha":"y","is_fork":false}"#,
        )
        .unwrap();
        fs::write(&toml_path, "[rows.coverage]\nwarn_pp_delta = \"tight\"\n").unwrap();
        let code = run([
            OsString::from("--pr-meta"),
            OsString::from(pr_path.as_os_str()),
            OsString::from("--coverage-delta-pp"),
            OsString::from("-2.5"),
            OsString::from("--quality-toml"),
            OsString::from(toml_path.as_os_str()),
            OsString::from("--out"),
            OsString::from(out_path.as_os_str()),
        ]);
        assert_eq!(code, ExitCode::from(1));
        // Malformed TOML must NOT silently produce a fallback artifact.
        assert!(!out_path.exists());
    }

    /// Minimal scoped tempdir without pulling in a dev-dep — mirrors the
    /// pattern in `emit_schema::tests`. Tests that fail mid-execution
    /// leak the dir, which is acceptable for /tmp.
    struct TmpDir {
        path: PathBuf,
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn tempdir() -> TmpDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let pid = std::process::id();
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("scorecard-aggregate-{pid}-{nonce}"));
        fs::create_dir_all(&path).expect("create tempdir");
        TmpDir { path }
    }

    #[test]
    fn write_scorecard_creates_parent_dirs_and_emits_json() {
        let dir = tempdir();
        let out = dir.path.join("nested/out/scorecard.json");
        let sc = build_with_delta(0.0);
        write_scorecard(&sc, &out).expect("write");
        let content = fs::read_to_string(&out).expect("read back");
        let parsed: Value = serde_json::from_str(&content).expect("valid json");
        assert_eq!(parsed["overall_status"], "Green");
        // CoverageDelta + five wired rows (BDD feature-skip + BDD
        // scenario-skip + CI wall-clock + flaky + changed-scope) +
        // four producer-pending stubs.
        assert_eq!(parsed["rows"].as_array().map(|a| a.len()), Some(10));
    }

    // ── Changed-scope diagram producer ──────────────────────────────

    #[test]
    fn build_changed_scope_row_renders_no_diff_for_empty_scope() {
        let scope = ChangedScope::default();
        let row = build_changed_scope_row(&scope);
        let Row::ChangedScopeDiagram {
            status,
            mermaid_md,
            node_count,
            delta_text,
            ..
        } = row
        else {
            panic!("expected ChangedScopeDiagram")
        };
        assert_eq!(status, Status::Green);
        assert_eq!(node_count, 0);
        assert_eq!(delta_text, "(no diff)");
        assert!(mermaid_md.contains("graph LR"), "got: {mermaid_md}");
        assert!(mermaid_md.contains("(no diff)"), "got: {mermaid_md}");
    }

    #[test]
    fn build_changed_scope_row_renders_each_touched_crate_as_node() {
        let scope = ChangedScope {
            touched: vec!["mokumo-shop".into(), "kikan".into(), "web".into()],
            truncated: false,
        };
        let row = build_changed_scope_row(&scope);
        let Row::ChangedScopeDiagram {
            mermaid_md,
            node_count,
            delta_text,
            ..
        } = row
        else {
            panic!("expected ChangedScopeDiagram")
        };
        assert_eq!(node_count, 3);
        assert_eq!(delta_text, "3 crates touched");
        assert!(mermaid_md.contains("mokumo-shop"));
        assert!(mermaid_md.contains("kikan"));
        assert!(mermaid_md.contains("web"));
    }

    #[test]
    fn build_changed_scope_row_includes_truncation_footer_when_set() {
        let touched: Vec<String> = (0..CHANGED_SCOPE_NODE_LIMIT)
            .map(|i| format!("crate-{i}"))
            .collect();
        let scope = ChangedScope {
            touched,
            truncated: true,
        };
        let row = build_changed_scope_row(&scope);
        let Row::ChangedScopeDiagram {
            mermaid_md,
            delta_text,
            ..
        } = row
        else {
            panic!("expected ChangedScopeDiagram")
        };
        assert!(
            mermaid_md.contains("truncated"),
            "expected truncation footer, got: {mermaid_md}"
        );
        assert!(delta_text.contains("truncated"), "got: {delta_text}");
    }

    #[test]
    fn read_changed_scope_returns_none_when_path_absent() {
        assert!(read_changed_scope(None).expect("absent").is_none());
    }

    #[test]
    fn read_changed_scope_groups_paths_by_crate() {
        let dir = tempdir();
        let path = dir.path.join("changed.txt");
        fs::write(
            &path,
            "crates/mokumo-shop/src/lib.rs\n\
             crates/mokumo-shop/src/customer.rs\n\
             crates/kikan/src/lib.rs\n\
             apps/web/src/lib/foo.ts\n",
        )
        .expect("write");
        let scope = read_changed_scope(Some(&path))
            .expect("parse")
            .expect("Some");
        assert!(!scope.truncated);
        assert_eq!(scope.touched.len(), 3);
        assert!(scope.touched.contains(&"mokumo-shop".to_string()));
        assert!(scope.touched.contains(&"kikan".to_string()));
        assert!(scope.touched.contains(&"web".to_string()));
    }

    #[test]
    fn read_changed_scope_marks_truncated_above_limit() {
        let dir = tempdir();
        let path = dir.path.join("changed.txt");
        let mut body = String::new();
        for i in 0..(CHANGED_SCOPE_NODE_LIMIT + 5) {
            body.push_str(&format!("crates/crate-{i}/src/lib.rs\n"));
        }
        fs::write(&path, body).expect("write");
        let scope = read_changed_scope(Some(&path))
            .expect("parse")
            .expect("Some");
        assert!(scope.truncated);
        assert_eq!(scope.touched.len(), CHANGED_SCOPE_NODE_LIMIT);
    }

    #[test]
    fn read_changed_scope_skips_blank_lines() {
        let dir = tempdir();
        let path = dir.path.join("changed.txt");
        fs::write(&path, "\n   \ncrates/mokumo-shop/src/lib.rs\n\n").expect("write");
        let scope = read_changed_scope(Some(&path))
            .expect("parse")
            .expect("Some");
        assert_eq!(scope.touched, vec!["mokumo-shop".to_string()]);
    }

    #[test]
    fn build_scorecard_emits_changed_scope_row_after_flaky() {
        // V4 (#769) row order: coverage, bdd-feature, bdd-scenario,
        // ci-wall-clock, flaky, changed-scope, then stubs. Changed
        // scope is the sixth row (index 5).
        let sc = build_with_delta(0.3);
        let Row::ChangedScopeDiagram {
            status, node_count, ..
        } = &sc.rows[5]
        else {
            panic!("expected ChangedScopeDiagram as the sixth row")
        };
        assert_eq!(*status, Status::Green);
        assert_eq!(*node_count, 0);
    }

    // ── Flaky-population producer ───────────────────────────────────

    #[test]
    fn build_flaky_population_row_green_when_below_warn() {
        let corpus = FlakyCorpus {
            marker_count: 2,
            retry_events: 0,
        };
        let row = build_flaky_population_row(&corpus, &FlakyPopulationThresholds::default());
        let Row::FlakyPopulation {
            status,
            flaky_marker_count,
            nextest_retry_events,
            delta_text,
            ..
        } = row
        else {
            panic!("expected FlakyPopulation")
        };
        assert_eq!(status, Status::Green);
        assert_eq!(flaky_marker_count, 2);
        assert_eq!(nextest_retry_events, 0);
        assert_eq!(delta_text, "2 markers");
    }

    #[test]
    fn build_flaky_population_row_yellow_at_warn_threshold() {
        let corpus = FlakyCorpus {
            marker_count: 5,
            retry_events: 0,
        };
        let row = build_flaky_population_row(&corpus, &FlakyPopulationThresholds::default());
        assert!(matches!(
            row,
            Row::FlakyPopulation {
                status: Status::Yellow,
                ..
            }
        ));
    }

    #[test]
    fn build_flaky_population_row_red_at_fail_threshold_carries_detail() {
        let corpus = FlakyCorpus {
            marker_count: 25,
            retry_events: 0,
        };
        let row = build_flaky_population_row(&corpus, &FlakyPopulationThresholds::default());
        let Row::FlakyPopulation {
            status,
            failure_detail_md,
            ..
        } = row
        else {
            panic!("expected FlakyPopulation")
        };
        assert_eq!(status, Status::Red);
        let detail = failure_detail_md.expect("Red rows carry failure_detail_md");
        assert!(detail.contains("25"), "got: {detail}");
        assert!(detail.contains("at or above"), "got: {detail}");
    }

    #[test]
    fn build_flaky_population_row_includes_retries_in_delta_text() {
        let corpus = FlakyCorpus {
            marker_count: 3,
            retry_events: 7,
        };
        let row = build_flaky_population_row(&corpus, &FlakyPopulationThresholds::default());
        let Row::FlakyPopulation { delta_text, .. } = row else {
            panic!("expected FlakyPopulation")
        };
        assert_eq!(delta_text, "3 markers / 7 retries");
    }

    #[test]
    fn discover_flaky_corpus_counts_markers_in_supported_extensions() {
        let dir = tempdir();
        fs::write(
            dir.path.join("a.rs"),
            "fn x() { /* ok */ }\n// FLAKY: timing-sensitive\nfn y() {}\n",
        )
        .expect("write rs");
        fs::write(
            dir.path.join("b.ts"),
            "// FLAKY: dom timing\nconst x = 1;\n// FLAKY: another\n",
        )
        .expect("write ts");
        // Wrong extension — should not be scanned.
        fs::write(dir.path.join("c.txt"), "// FLAKY: ignored\n").expect("write txt");
        let corpus =
            discover_flaky_corpus(std::slice::from_ref(&dir.path), None).expect("discover ok");
        assert_eq!(corpus.marker_count, 3);
        assert_eq!(corpus.retry_events, 0);
    }

    #[test]
    fn discover_flaky_corpus_picks_up_retry_count_from_json() {
        let dir = tempdir();
        let retry = NextestRetryJson { retry_count: 4 };
        let corpus =
            discover_flaky_corpus(std::slice::from_ref(&dir.path), Some(&retry)).expect("discover");
        assert_eq!(corpus.marker_count, 0);
        assert_eq!(corpus.retry_events, 4);
    }

    #[test]
    fn discover_flaky_corpus_skips_missing_root() {
        let dir = tempdir();
        let missing = dir.path.join("nope");
        let corpus = discover_flaky_corpus(&[missing], None).expect("missing root is empty");
        assert_eq!(corpus.marker_count, 0);
    }

    #[test]
    fn read_nextest_retry_json_returns_none_when_path_absent() {
        assert!(read_nextest_retry_json(None).expect("absent").is_none());
    }

    #[test]
    fn read_nextest_retry_json_parses_valid_file() {
        let dir = tempdir();
        let path = dir.path.join("retry.json");
        fs::write(&path, r#"{"retry_count":4}"#).expect("write");
        let parsed = read_nextest_retry_json(Some(&path))
            .expect("parse")
            .expect("Some");
        assert_eq!(parsed.retry_count, 4);
    }

    #[test]
    fn build_scorecard_emits_flaky_row_after_ci_wall_clock() {
        // Flaky is the fifth row (index 4) after the V4 BDD split.
        let sc = build_with_delta(0.3);
        let Row::FlakyPopulation {
            status,
            flaky_marker_count,
            ..
        } = &sc.rows[4]
        else {
            panic!("expected FlakyPopulation as the fifth row")
        };
        assert_eq!(*status, Status::Green);
        assert_eq!(*flaky_marker_count, 0);
    }

    // ── CI wall-clock producer ──────────────────────────────────────

    #[test]
    fn build_ci_wall_clock_row_green_when_no_base_data() {
        let json = CiWallClockJson {
            total_seconds: 600.0,
            base_total_seconds: None,
        };
        let row = build_ci_wall_clock_row(&json, &CiWallClockThresholds::default());
        let Row::CiWallClockDelta {
            status,
            total_ci_seconds,
            delta_seconds,
            delta_text,
            ..
        } = row
        else {
            panic!("expected CiWallClockDelta")
        };
        assert_eq!(status, Status::Green);
        assert_eq!(total_ci_seconds, 600.0);
        assert_eq!(delta_seconds, 0.0);
        assert!(delta_text.contains("(no base)"), "got: {delta_text}");
    }

    #[test]
    fn build_ci_wall_clock_row_yellow_at_warn_slowdown() {
        let json = CiWallClockJson {
            total_seconds: 660.0,
            base_total_seconds: Some(600.0),
        };
        let row = build_ci_wall_clock_row(&json, &CiWallClockThresholds::default());
        assert!(matches!(
            row,
            Row::CiWallClockDelta {
                status: Status::Yellow,
                ..
            }
        ));
    }

    #[test]
    fn build_ci_wall_clock_row_red_at_fail_slowdown_carries_detail() {
        let json = CiWallClockJson {
            total_seconds: 1200.0,
            base_total_seconds: Some(600.0),
        };
        let row = build_ci_wall_clock_row(&json, &CiWallClockThresholds::default());
        let Row::CiWallClockDelta {
            status,
            failure_detail_md,
            ..
        } = row
        else {
            panic!("expected CiWallClockDelta")
        };
        assert_eq!(status, Status::Red);
        let detail = failure_detail_md.expect("Red rows carry failure_detail_md");
        assert!(detail.contains("600s"), "got: {detail}");
        assert!(detail.contains("at or above"), "got: {detail}");
    }

    #[test]
    fn build_ci_wall_clock_row_speedup_resolves_green() {
        // Negative delta — CI sped up.
        let json = CiWallClockJson {
            total_seconds: 400.0,
            base_total_seconds: Some(600.0),
        };
        let row = build_ci_wall_clock_row(&json, &CiWallClockThresholds::default());
        let Row::CiWallClockDelta {
            status,
            delta_seconds,
            ..
        } = row
        else {
            panic!("expected CiWallClockDelta")
        };
        assert_eq!(status, Status::Green);
        assert_eq!(delta_seconds, -200.0);
    }

    #[test]
    fn read_ci_wall_clock_json_returns_none_when_path_absent() {
        let parsed = read_ci_wall_clock_json(None).expect("absent flag is None");
        assert!(parsed.is_none());
    }

    #[test]
    fn read_ci_wall_clock_json_parses_valid_file() {
        let dir = tempdir();
        let path = dir.path.join("wc.json");
        fs::write(
            &path,
            r#"{"total_seconds":600.0,"base_total_seconds":580.0}"#,
        )
        .expect("write");
        let parsed = read_ci_wall_clock_json(Some(&path)).expect("parse");
        let parsed = parsed.expect("Some");
        assert_eq!(parsed.total_seconds, 600.0);
        assert_eq!(parsed.base_total_seconds, Some(580.0));
    }

    #[test]
    fn read_ci_wall_clock_json_omits_base_total_seconds_field() {
        let dir = tempdir();
        let path = dir.path.join("wc.json");
        fs::write(&path, r#"{"total_seconds":600.0}"#).expect("write");
        let parsed = read_ci_wall_clock_json(Some(&path))
            .expect("parse")
            .expect("Some");
        assert!(parsed.base_total_seconds.is_none());
    }

    #[test]
    fn read_ci_wall_clock_json_rejects_non_finite_total() {
        let dir = tempdir();
        let path = dir.path.join("wc.json");
        // serde_json's default Number does not handle NaN/inf — but we
        // can sneak a `null` into base_total_seconds (valid). Invalid
        // shape: missing total_seconds. Use that to exercise the error
        // path.
        fs::write(&path, r#"{"base_total_seconds":600.0}"#).expect("write");
        let err = read_ci_wall_clock_json(Some(&path)).unwrap_err();
        assert!(err.contains("CiWallClockJson"), "got: {err}");
    }

    #[test]
    fn read_ci_wall_clock_json_rejects_unknown_field() {
        let dir = tempdir();
        let path = dir.path.join("wc.json");
        fs::write(
            &path,
            r#"{"total_seconds":600.0,"base_total_seconds":580.0,"extra":1}"#,
        )
        .expect("write");
        let err = read_ci_wall_clock_json(Some(&path)).unwrap_err();
        assert!(err.contains("unknown field") || err.contains("CiWallClockJson"));
    }

    #[test]
    fn build_scorecard_emits_ci_wall_clock_row_after_bdd() {
        // CI wall-clock is the fourth row (index 3) after the V4 BDD
        // split — coverage, feature-skip, scenario-skip, ci-wall-clock.
        let sc = build_with_delta(0.3);
        let Row::CiWallClockDelta {
            status,
            total_ci_seconds,
            delta_seconds,
            ..
        } = &sc.rows[3]
        else {
            panic!("expected CiWallClockDelta as the fourth row")
        };
        assert_eq!(*status, Status::Green);
        assert_eq!(*total_ci_seconds, 0.0);
        assert_eq!(*delta_seconds, 0.0);
    }

    // ── BDD producer helpers ─────────────────────────────────────────

    #[test]
    fn classify_feature_line_recognises_each_kind() {
        use FeatureLineKind::*;
        assert_eq!(classify_feature_line(""), Skip);
        assert_eq!(classify_feature_line("# comment"), Skip);
        assert_eq!(classify_feature_line("@wip @future"), Tags);
        assert_eq!(classify_feature_line("Feature: foo"), FeatureOrRule);
        assert_eq!(classify_feature_line("Rule: bar"), FeatureOrRule);
        assert_eq!(classify_feature_line("Scenario: baz"), ScenarioStart);
        assert_eq!(classify_feature_line("Scenario Outline: o"), ScenarioStart);
        assert_eq!(classify_feature_line("Example: e"), ScenarioStart);
        assert_eq!(classify_feature_line("Background:"), Background);
        assert_eq!(classify_feature_line("Given a step"), Other);
        assert_eq!(classify_feature_line("| col | row |"), Other);
    }

    #[test]
    fn extract_tags_collects_only_at_prefixed_tokens() {
        assert_eq!(extract_tags("@wip"), vec!["@wip".to_string()]);
        assert_eq!(
            extract_tags("@wip @future @tracked:mokumo#1"),
            vec![
                "@wip".to_string(),
                "@future".to_string(),
                "@tracked:mokumo#1".to_string()
            ]
        );
        // Non-@ tokens are silently dropped.
        assert_eq!(
            extract_tags("@wip not_a_tag @real"),
            vec!["@wip".to_string(), "@real".to_string()]
        );
        assert!(extract_tags("").is_empty());
    }

    #[test]
    fn tally_scenario_tags_counts_scenario_skip_when_pending_carries_skip_tag() {
        let mut parsed = ParsedFeature::default();
        let mut pending = vec!["@wip".to_string()];
        tally_scenario_tags(&mut parsed, &mut pending);
        assert_eq!(parsed.total_scenarios, 1);
        assert_eq!(parsed.scenario_skipped, 1);
        assert_eq!(parsed.scenario_by_tag.get("@wip"), Some(&1));
        assert!(pending.is_empty(), "pending must be drained");
    }

    #[test]
    fn tally_scenario_tags_does_not_count_skip_when_only_neutral_tags() {
        let mut parsed = ParsedFeature::default();
        let mut pending = vec!["@happy-path".to_string()];
        tally_scenario_tags(&mut parsed, &mut pending);
        assert_eq!(parsed.total_scenarios, 1);
        assert_eq!(parsed.scenario_skipped, 0);
        assert!(parsed.scenario_by_tag.is_empty());
    }

    #[test]
    fn tally_scenario_tags_skips_scenario_breakdown_when_feature_level_skipped() {
        // When the file is feature-level skipped, the scenario count
        // increments but the scenario-level skip + by_tag stay empty —
        // the file's signal lives on `feature_by_tag` instead.
        let mut parsed = ParsedFeature {
            feature_level_skipped: true,
            ..ParsedFeature::default()
        };
        let mut pending = vec!["@wip".to_string()];
        tally_scenario_tags(&mut parsed, &mut pending);
        assert_eq!(parsed.total_scenarios, 1);
        assert_eq!(parsed.scenario_skipped, 0);
        assert!(parsed.scenario_by_tag.is_empty());
    }

    #[test]
    fn is_bdd_skip_tag_recognises_skip_tags_and_tracked_prefix() {
        assert!(is_bdd_skip_tag("@wip"));
        assert!(is_bdd_skip_tag("@future"));
        assert!(is_bdd_skip_tag("@ignore"));
        assert!(is_bdd_skip_tag("@skip"));
        assert!(is_bdd_skip_tag("@tracked:mokumo#1"));
        assert!(!is_bdd_skip_tag("@happy-path"));
        assert!(!is_bdd_skip_tag("@area:auth"));
    }

    #[test]
    fn is_feature_file_only_matches_feature_extension() {
        use std::path::Path;
        assert!(is_feature_file(Path::new("foo.feature")));
        assert!(is_feature_file(Path::new("dir/sub/foo.feature")));
        assert!(!is_feature_file(Path::new("foo.txt")));
        assert!(!is_feature_file(Path::new("foo")));
        assert!(!is_feature_file(Path::new("foo.feature.bak")));
    }

    #[test]
    fn walk_files_matching_returns_sorted_results() {
        let dir = tempdir();
        fs::write(dir.path.join("b.feature"), "").unwrap();
        fs::write(dir.path.join("a.feature"), "").unwrap();
        fs::write(dir.path.join("c.txt"), "").unwrap();
        let found = walk_files_matching(std::slice::from_ref(&dir.path), is_feature_file);
        assert_eq!(found.len(), 2);
        let names: Vec<_> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["a.feature".to_string(), "b.feature".to_string()]
        );
    }

    #[test]
    fn walk_files_matching_silently_skips_missing_roots() {
        let dir = tempdir();
        let missing = dir.path.join("nope");
        let found = walk_files_matching(&[missing], is_feature_file);
        assert!(found.is_empty());
    }

    #[test]
    fn read_source_file_surfaces_io_error_with_label() {
        let dir = tempdir();
        let missing = dir.path.join("nope.txt");
        let err = read_source_file(&missing, "feature").unwrap_err();
        assert!(err.contains("feature file"), "got: {err}");
        assert!(err.contains(missing.to_str().unwrap()), "got: {err}");
    }

    #[test]
    fn merge_parsed_feature_aggregates_scenario_signal_into_per_crate_bucket() {
        let mut per_crate: std::collections::BTreeMap<String, BddCrateAcc> = Default::default();
        let mut summary = BddSummary::default();
        let parsed = ParsedFeature {
            total_scenarios: 3,
            scenario_skipped: 1,
            feature_level_skipped: false,
            feature_tags: vec![],
            scenario_by_tag: std::collections::BTreeMap::from([("@wip".into(), 1)]),
        };
        merge_parsed_feature(&mut per_crate, &mut summary, "foo".into(), parsed);
        assert_eq!(summary.total_features, 1);
        assert_eq!(summary.skipped_features, 0);
        assert_eq!(summary.total_scenarios, 3);
        assert_eq!(summary.skipped_scenarios, 1);
        assert_eq!(summary.scenario_by_tag.get("@wip"), Some(&1));

        let bucket = per_crate.get("foo").unwrap();
        assert_eq!(bucket.feature_total, 1);
        assert_eq!(bucket.feature_skipped, 0);
        assert_eq!(bucket.scenario_total, 3);
        assert_eq!(bucket.scenario_skipped, 1);
        assert_eq!(bucket.scenario_tag_counts.get("@wip"), Some(&1));
    }

    #[test]
    fn merge_parsed_feature_aggregates_feature_signal_when_feature_level_skipped() {
        let mut per_crate: std::collections::BTreeMap<String, BddCrateAcc> = Default::default();
        let mut summary = BddSummary::default();
        let parsed = ParsedFeature {
            total_scenarios: 5,
            scenario_skipped: 0,
            feature_level_skipped: true,
            feature_tags: vec!["@wip".into()],
            scenario_by_tag: Default::default(),
        };
        merge_parsed_feature(&mut per_crate, &mut summary, "bar".into(), parsed);
        assert_eq!(summary.total_features, 1);
        assert_eq!(summary.skipped_features, 1);
        assert_eq!(summary.feature_by_tag.get("@wip"), Some(&1));
        // Scenario-level signal stays empty when the file is feature-
        // level gated — the count lives on the feature row.
        assert_eq!(summary.skipped_scenarios, 0);
        assert!(summary.scenario_by_tag.is_empty());

        let bucket = per_crate.get("bar").unwrap();
        assert_eq!(bucket.feature_skipped, 1);
        assert_eq!(bucket.feature_tag_counts.get("@wip"), Some(&1));
        assert!(bucket.scenario_tag_counts.is_empty());
    }

    // ── Flaky-marker helpers ─────────────────────────────────────────

    #[test]
    fn is_flaky_scannable_only_matches_known_extensions() {
        use std::path::Path;
        for ext in ["rs", "ts", "tsx", "js", "mjs", "svelte"] {
            assert!(is_flaky_scannable(Path::new(&format!("file.{ext}"))));
        }
        assert!(!is_flaky_scannable(Path::new("file.txt")));
        assert!(!is_flaky_scannable(Path::new("README")));
    }

    #[test]
    fn is_flaky_marker_line_accepts_real_marker() {
        assert!(is_flaky_marker_line("// FLAKY: timing-sensitive"));
        assert!(is_flaky_marker_line("    // FLAKY: indented"));
    }

    #[test]
    fn is_flaky_marker_line_rejects_doc_comment_reference() {
        // `///` rustdoc references mention the marker without being one.
        assert!(!is_flaky_marker_line("/// `// FLAKY:` source markers..."));
        assert!(!is_flaky_marker_line("    /// see `// FLAKY:`"));
    }

    #[test]
    fn is_flaky_marker_line_rejects_string_literal_reference() {
        // The constant declaration that defines the marker itself.
        assert!(!is_flaky_marker_line(
            "const FLAKY_MARKER: &str = \"// FLAKY:\";"
        ));
        // Test fixtures embedding the marker in a literal.
        assert!(!is_flaky_marker_line("    \"// FLAKY: ignored\\n\""));
    }

    #[test]
    fn count_flaky_markers_handles_mixed_content() {
        let body = "fn x() {}\n\
                    // FLAKY: one\n\
                    /// `// FLAKY:` doc comment\n\
                    \"// FLAKY: literal\"\n\
                    // FLAKY: two\n";
        assert_eq!(count_flaky_markers(body), 2);
    }

    #[test]
    fn count_flaky_markers_returns_zero_on_empty_input() {
        assert_eq!(count_flaky_markers(""), 0);
    }

    // ── Changed-scope helpers ────────────────────────────────────────

    #[test]
    fn project_changed_scope_dedupes_per_crate() {
        let body = "crates/foo/src/a.rs\n\
                    crates/foo/src/b.rs\n\
                    apps/web/src/a.ts\n";
        let scope = project_changed_scope(body);
        assert_eq!(scope.touched, vec!["foo".to_string(), "web".to_string()]);
        assert!(!scope.truncated);
    }

    #[test]
    fn project_changed_scope_drops_paths_outside_crates_and_apps() {
        // Documentation, workflow, and root-file changes are NOT
        // workspace crates and must not appear in the diagram —
        // historically these silently merged into an `unknown` bucket
        // which was actively misleading on docs-only PRs.
        let body = "README.md\n\
                    CHANGELOG.md\n\
                    .github/workflows/quality.yml\n\
                    docs/architecture.md\n\
                    crates/foo/src/lib.rs\n";
        let scope = project_changed_scope(body);
        assert_eq!(scope.touched, vec!["foo".to_string()]);
        assert!(!scope.truncated);
    }

    #[test]
    fn project_changed_scope_returns_empty_for_docs_only_diff() {
        // A docs-only PR produces an empty scope (renderer paints the
        // "no diff" placeholder). Previously this rendered a single
        // `unknown` node — now it correctly shows nothing-touched.
        let body = "README.md\nCHANGELOG.md\n";
        let scope = project_changed_scope(body);
        assert!(scope.touched.is_empty());
        assert!(!scope.truncated);
    }

    #[test]
    fn project_changed_scope_marks_truncated_when_unique_crates_exceed_limit() {
        let mut body = String::new();
        for i in 0..(CHANGED_SCOPE_NODE_LIMIT + 5) {
            body.push_str(&format!("crates/c{i}/src/lib.rs\n"));
        }
        let scope = project_changed_scope(&body);
        assert!(scope.truncated);
        assert_eq!(scope.touched.len(), CHANGED_SCOPE_NODE_LIMIT);
    }

    #[test]
    fn mermaid_empty_scope_uses_no_diff_label() {
        let s = mermaid_empty_scope();
        assert!(s.contains("graph LR"));
        assert!(s.contains("(no diff)"));
    }

    #[test]
    fn mermaid_truncation_footer_mentions_limit_and_logs() {
        let s = mermaid_truncation_footer();
        assert!(s.contains(&CHANGED_SCOPE_NODE_LIMIT.to_string()));
        assert!(s.contains("workflow logs"));
    }

    // ── Wall-clock helpers ───────────────────────────────────────────

    #[test]
    fn delta_sign_prefix_is_plus_for_zero_and_positive() {
        assert_eq!(delta_sign_prefix(0.0), "+");
        assert_eq!(delta_sign_prefix(60.0), "+");
        assert_eq!(delta_sign_prefix(-1.0), "");
    }

    #[test]
    fn validate_ci_wall_clock_finite_rejects_nan_total() {
        let parsed = CiWallClockJson {
            total_seconds: f64::NAN,
            base_total_seconds: None,
        };
        let err = validate_ci_wall_clock_finite(&parsed, std::path::Path::new("x"))
            .expect_err("non-finite total must reject");
        assert!(err.contains("finite"), "got: {err}");
    }

    #[test]
    fn validate_ci_wall_clock_finite_rejects_nan_base() {
        let parsed = CiWallClockJson {
            total_seconds: 100.0,
            base_total_seconds: Some(f64::NAN),
        };
        validate_ci_wall_clock_finite(&parsed, std::path::Path::new("x"))
            .expect_err("non-finite base must reject");
    }

    #[test]
    fn validate_ci_wall_clock_finite_accepts_valid() {
        let parsed = CiWallClockJson {
            total_seconds: 100.0,
            base_total_seconds: Some(80.0),
        };
        validate_ci_wall_clock_finite(&parsed, std::path::Path::new("x"))
            .expect("finite values must pass");
    }

    // ── Run dispatch helpers ─────────────────────────────────────────

    #[test]
    fn usage_err_pairs_message_with_exit_two() {
        assert_eq!(usage_err("oops".into()).1, 2);
    }

    #[test]
    fn runtime_err_pairs_message_with_exit_one() {
        assert_eq!(runtime_err("oops".into()).1, 1);
    }

    // ── BDD producer ────────────────────────────────────────────────

    #[test]
    fn parse_feature_counts_simple_scenario() {
        let body = r#"
Feature: example

  Scenario: alpha
    Given a step
"#;
        let parsed = parse_feature(body);
        assert_eq!(parsed.total_scenarios, 1);
        assert_eq!(parsed.scenario_skipped, 0);
        assert!(!parsed.feature_level_skipped);
    }

    #[test]
    fn parse_feature_counts_scenario_skip_via_wip_tag() {
        let body = r#"
Feature: example

  @wip
  Scenario: deferred
    Given a step

  Scenario: shipping
    Given another step
"#;
        let parsed = parse_feature(body);
        assert_eq!(parsed.total_scenarios, 2);
        assert_eq!(parsed.scenario_skipped, 1);
        assert_eq!(parsed.scenario_by_tag.get("@wip"), Some(&1));
        assert!(!parsed.feature_level_skipped);
    }

    #[test]
    fn parse_feature_counts_tracked_prefix_as_scenario_skipped() {
        let body = r#"
Feature: example

  @tracked:mokumo#123
  Scenario: deferred
    Given a step
"#;
        let parsed = parse_feature(body);
        assert_eq!(parsed.scenario_skipped, 1);
        assert_eq!(parsed.scenario_by_tag.get("@tracked:mokumo#123"), Some(&1));
    }

    #[test]
    fn parse_feature_marks_feature_level_skipped_when_feature_tag_present() {
        // Feature-level skip tag promotes the file to feature-level
        // skipped and clears the scenario-level signal.
        let body = r#"
@wip
Feature: example

  Scenario: alpha
    Given a

  Scenario: beta
    Given b
"#;
        let parsed = parse_feature(body);
        assert_eq!(parsed.total_scenarios, 2);
        assert!(parsed.feature_level_skipped);
        assert_eq!(parsed.scenario_skipped, 0);
        assert!(parsed.scenario_by_tag.is_empty());
        assert_eq!(parsed.feature_tags, vec!["@wip".to_string()]);
    }

    #[test]
    fn parse_feature_counts_scenario_outline_and_example() {
        let body = r#"
Feature: example

  Scenario Outline: alpha
    Given <x>

    Examples:
      | x |
      | 1 |

  Example: beta
    Given a step
"#;
        let parsed = parse_feature(body);
        assert_eq!(parsed.total_scenarios, 2);
    }

    #[test]
    fn parse_feature_ignores_comments_and_blank_lines() {
        let body = r#"
# top comment
Feature: example

  # in-feature comment

  Scenario: alpha
    Given a step
"#;
        let parsed = parse_feature(body);
        assert_eq!(parsed.total_scenarios, 1);
    }

    fn empty_summary() -> BddSummary {
        BddSummary::default()
    }

    #[test]
    fn build_bdd_feature_skip_row_green_below_warn_threshold() {
        let mut summary = empty_summary();
        summary.total_features = 40;
        summary.skipped_features = 4;
        let row = build_bdd_feature_skip_row(&summary, &BddFeatureSkipThresholds::default());
        let Row::BddFeatureLevelSkipped {
            status,
            total_features,
            skipped_features,
            delta_text,
            failure_detail_md,
            ..
        } = row
        else {
            panic!("expected BddFeatureLevelSkipped")
        };
        assert_eq!(status, Status::Green);
        assert_eq!(total_features, 40);
        assert_eq!(skipped_features, 4);
        assert_eq!(delta_text, "4 WIP / 40 features");
        assert!(failure_detail_md.is_none());
    }

    #[test]
    fn build_bdd_feature_skip_row_yellow_at_warn_threshold() {
        let mut summary = empty_summary();
        summary.total_features = 40;
        summary.skipped_features = 10;
        let row = build_bdd_feature_skip_row(&summary, &BddFeatureSkipThresholds::default());
        assert!(matches!(
            row,
            Row::BddFeatureLevelSkipped {
                status: Status::Yellow,
                ..
            }
        ));
    }

    #[test]
    fn build_bdd_feature_skip_row_red_at_fail_threshold_carries_detail() {
        let mut summary = empty_summary();
        summary.total_features = 40;
        summary.skipped_features = 20;
        let row = build_bdd_feature_skip_row(&summary, &BddFeatureSkipThresholds::default());
        let Row::BddFeatureLevelSkipped {
            status,
            failure_detail_md,
            ..
        } = row
        else {
            panic!("expected BddFeatureLevelSkipped")
        };
        assert_eq!(status, Status::Red);
        let detail = failure_detail_md.expect("Red rows carry failure_detail_md");
        assert!(detail.contains("20"), "got: {detail}");
        assert!(detail.contains("at or above"), "got: {detail}");
    }

    #[test]
    fn build_bdd_scenario_skip_row_green_below_warn_threshold() {
        let mut summary = empty_summary();
        summary.total_scenarios = 900;
        summary.skipped_scenarios = 5;
        let row = build_bdd_scenario_skip_row(&summary, &BddScenarioSkipThresholds::default());
        let Row::BddScenarioLevelSkipped {
            status,
            total_scenarios,
            skipped_scenarios,
            delta_text,
            failure_detail_md,
            ..
        } = row
        else {
            panic!("expected BddScenarioLevelSkipped")
        };
        assert_eq!(status, Status::Green);
        assert_eq!(total_scenarios, 900);
        assert_eq!(skipped_scenarios, 5);
        assert_eq!(delta_text, "5 skipped / 900 scenarios");
        assert!(failure_detail_md.is_none());
    }

    #[test]
    fn build_bdd_scenario_skip_row_yellow_at_warn_threshold() {
        let mut summary = empty_summary();
        summary.total_scenarios = 900;
        summary.skipped_scenarios = 40;
        let row = build_bdd_scenario_skip_row(&summary, &BddScenarioSkipThresholds::default());
        assert!(matches!(
            row,
            Row::BddScenarioLevelSkipped {
                status: Status::Yellow,
                ..
            }
        ));
    }

    #[test]
    fn build_bdd_scenario_skip_row_red_at_fail_threshold() {
        let mut summary = empty_summary();
        summary.total_scenarios = 900;
        summary.skipped_scenarios = 60;
        let row = build_bdd_scenario_skip_row(&summary, &BddScenarioSkipThresholds::default());
        let Row::BddScenarioLevelSkipped {
            status,
            failure_detail_md,
            ..
        } = row
        else {
            panic!("expected BddScenarioLevelSkipped")
        };
        assert_eq!(status, Status::Red);
        let detail = failure_detail_md.expect("Red rows carry failure_detail_md");
        assert!(detail.contains("60"), "got: {detail}");
    }

    #[test]
    fn discover_bdd_corpus_walks_feature_files_in_root_split_signal() {
        // Three files: one feature-level skipped, one with a single
        // scenario-level skip, one clean. Asserts the split: feature
        // signal counts the gated file as 1; scenario signal counts
        // the in-feature skip as 1; the gated file's scenarios do NOT
        // double-count into the scenario signal.
        let dir = tempdir();
        let crate_dir = dir.path.join("crates/example/tests/features");
        fs::create_dir_all(&crate_dir).expect("mkdir");
        fs::write(
            crate_dir.join("a.feature"),
            "@wip\nFeature: a\n\n  Scenario: alpha\n    Given x\n  Scenario: beta\n    Given y\n",
        )
        .expect("write a");
        fs::write(
            crate_dir.join("b.feature"),
            "Feature: b\n\n  @wip\n  Scenario: deferred\n    Given y\n  Scenario: ships\n    Given z\n",
        )
        .expect("write b");
        fs::write(
            crate_dir.join("c.feature"),
            "Feature: c\n\n  Scenario: clean\n    Given y\n",
        )
        .expect("write c");

        let summary = discover_bdd_corpus(std::slice::from_ref(&dir.path)).expect("walk");
        assert_eq!(summary.total_features, 3);
        assert_eq!(
            summary.skipped_features, 1,
            "only `a.feature` is feature-level @wip"
        );
        assert_eq!(summary.feature_by_tag.get("@wip"), Some(&1));

        // Scenarios in feature-level-skipped files do NOT count toward
        // scenario_skipped, so the only scenario-level skip is the
        // single @wip scenario in b.feature.
        assert_eq!(summary.total_scenarios, 5);
        assert_eq!(summary.skipped_scenarios, 1);
        assert_eq!(summary.scenario_by_tag.get("@wip"), Some(&1));

        assert_eq!(summary.feature_breakouts.len(), 1);
        let fb = &summary.feature_breakouts[0];
        assert_eq!(fb.crate_name, "example");
        assert_eq!(fb.feature_total, 3);
        assert_eq!(fb.feature_skipped, 1);

        let sb = &summary.scenario_breakouts[0];
        assert_eq!(sb.crate_name, "example");
        assert_eq!(sb.scenario_total, 5);
        assert_eq!(sb.scenario_skipped, 1);
    }

    #[test]
    fn discover_bdd_corpus_returns_empty_for_missing_root() {
        let dir = tempdir();
        let missing = dir.path.join("nope");
        let summary = discover_bdd_corpus(&[missing]).expect("missing root is empty corpus");
        assert_eq!(summary.total_features, 0);
        assert_eq!(summary.skipped_features, 0);
        assert_eq!(summary.total_scenarios, 0);
        assert_eq!(summary.skipped_scenarios, 0);
        assert!(summary.feature_breakouts.is_empty());
        assert!(summary.scenario_breakouts.is_empty());
    }

    #[test]
    fn crate_name_from_path_extracts_crate_segment() {
        let p = Path::new("crates/mokumo-shop/tests/features/quote.feature");
        assert_eq!(crate_name_from_path(p), Some("mokumo-shop".to_string()));
    }

    #[test]
    fn crate_name_from_path_extracts_apps_segment() {
        let p = Path::new("apps/web/tests/customer.feature");
        assert_eq!(crate_name_from_path(p), Some("web".to_string()));
    }

    #[test]
    fn crate_name_from_path_returns_none_for_unrecognised_segment() {
        // Root files, docs, .github/* etc. are not workspace crates —
        // the projector drops them so the changed-scope diagram does
        // not show an `unknown` node for every workflow tweak.
        assert_eq!(
            crate_name_from_path(Path::new("/tmp/random/path.feature")),
            None
        );
        assert_eq!(crate_name_from_path(Path::new("README.md")), None);
        assert_eq!(
            crate_name_from_path(Path::new(".github/workflows/quality.yml")),
            None
        );
    }

    #[test]
    fn write_scorecard_leaves_no_tmp_file_after_success() {
        // The atomic-write pattern (write tmp + rename) must not leave
        // .tmp.<pid> sidecars on the happy path.
        let dir = tempdir();
        let out = dir.path.join("scorecard.json");
        let sc = build_with_delta(0.0);
        write_scorecard(&sc, &out).expect("write");
        let entries: Vec<_> = fs::read_dir(&dir.path)
            .expect("read tmpdir")
            .filter_map(|e| e.ok().map(|e| e.file_name().into_string().ok()))
            .flatten()
            .collect();
        assert_eq!(entries, vec!["scorecard.json".to_string()]);
    }

    #[test]
    fn tmp_sibling_keeps_same_parent_and_is_distinct() {
        let out = Path::new("/tmp/work/scorecard.json");
        let tmp = tmp_sibling(out);
        assert_eq!(tmp.parent(), out.parent());
        assert_ne!(tmp, out);
        let name = tmp.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("scorecard.json.tmp."), "got: {name}");
    }

    #[test]
    fn ensure_parent_dir_creates_missing_parents() {
        let dir = tempdir();
        let out = dir.path.join("a/b/c/file.json");
        ensure_parent_dir(&out).expect("create");
        assert!(out.parent().unwrap().is_dir());
    }

    #[test]
    fn ensure_parent_dir_noop_for_bare_filename() {
        ensure_parent_dir(Path::new("x.json")).expect("noop");
    }

    #[test]
    fn render_scorecard_appends_trailing_newline() {
        let sc = build_with_delta(0.0);
        let s = render_scorecard(&sc).expect("render");
        assert!(s.ends_with('\n'));
    }

    #[test]
    fn read_pr_meta_rejects_missing_file_with_clear_error() {
        let path = PathBuf::from("/tmp/scorecard-aggregate-does-not-exist.json");
        let err = read_pr_meta(&path).unwrap_err();
        assert!(err.contains("--pr-meta"), "got: {err}");
    }

    #[test]
    fn read_pr_meta_rejects_invalid_json_with_clear_error() {
        let dir = tempdir();
        let path = dir.path.join("bad.json");
        fs::write(&path, "{not json}").unwrap();
        let err = read_pr_meta(&path).unwrap_err();
        assert!(err.contains("--pr-meta"), "got: {err}");
    }

    #[test]
    fn read_pr_meta_parses_valid_fixture() {
        let dir = tempdir();
        let path = dir.path.join("pr.json");
        fs::write(
            &path,
            r#"{"pr_number":42,"head_sha":"a","base_sha":"b","is_fork":true}"#,
        )
        .unwrap();
        let pr = read_pr_meta(&path).expect("parse");
        assert_eq!(pr.pr_number.0, 42);
        assert!(pr.is_fork);
    }

    #[test]
    fn parse_cli_returns_help_for_long_flag() {
        let code = parse_cli([OsString::from("--help")]).unwrap_err();
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn parse_cli_rejects_missing_required_args() {
        let code =
            parse_cli([OsString::from("--pr-meta"), OsString::from("/tmp/x.json")]).unwrap_err();
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn parse_cli_returns_struct_for_valid_args() {
        let cli = parse_cli([
            OsString::from("--pr-meta"),
            OsString::from("/tmp/pr.json"),
            OsString::from("--coverage-delta-pp"),
            OsString::from("-2.5"),
            OsString::from("--out"),
            OsString::from("/tmp/out.json"),
        ])
        .expect("parsed");
        assert_eq!(cli.pr_meta, PathBuf::from("/tmp/pr.json"));
        assert_eq!(cli.coverage_delta_pp, -2.5);
        assert_eq!(cli.out, PathBuf::from("/tmp/out.json"));
    }

    #[test]
    fn parse_cli_rejects_missing_coverage_delta_flag() {
        let code = parse_cli([
            OsString::from("--pr-meta"),
            OsString::from("/tmp/pr.json"),
            OsString::from("--out"),
            OsString::from("/tmp/out.json"),
        ])
        .unwrap_err();
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn parse_cli_rejects_non_numeric_coverage_delta() {
        let code = parse_cli([
            OsString::from("--pr-meta"),
            OsString::from("/tmp/pr.json"),
            OsString::from("--coverage-delta-pp"),
            OsString::from("not-a-number"),
            OsString::from("--out"),
            OsString::from("/tmp/out.json"),
        ])
        .unwrap_err();
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn parse_finite_f64_accepts_finite_inputs() {
        assert_eq!(parse_finite_f64("0.0").unwrap(), 0.0);
        assert_eq!(parse_finite_f64("-2.5").unwrap(), -2.5);
        assert_eq!(parse_finite_f64("12345.6789").unwrap(), 12345.6789);
    }

    #[test]
    fn parse_finite_f64_rejects_non_finite_inputs() {
        for bad in ["NaN", "nan", "inf", "+inf", "-inf", "Infinity"] {
            match parse_finite_f64(bad) {
                Ok(v) => panic!("expected error for {bad}, got {v}"),
                Err(err) => assert!(err.contains("finite"), "for {bad}: {err}"),
            }
        }
    }

    #[test]
    fn parse_cli_rejects_nan_coverage_delta() {
        let code = parse_cli([
            OsString::from("--pr-meta"),
            OsString::from("/tmp/pr.json"),
            OsString::from("--coverage-delta-pp"),
            OsString::from("NaN"),
            OsString::from("--out"),
            OsString::from("/tmp/out.json"),
        ])
        .unwrap_err();
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn run_returns_two_for_invalid_pr_meta_path() {
        let dir = tempdir();
        let out = dir.path.join("scorecard.json");
        let code = run([
            OsString::from("--pr-meta"),
            OsString::from("/tmp/does-not-exist-aggregate.json"),
            OsString::from("--coverage-delta-pp"),
            OsString::from("0.0"),
            OsString::from("--out"),
            OsString::from(out.as_os_str()),
        ]);
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn run_returns_two_for_unknown_flag() {
        let code = run([OsString::from("--bogus")]);
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn run_returns_success_for_help() {
        let code = run([OsString::from("--help")]);
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn run_writes_valid_scorecard_for_good_pr_meta() {
        // Routes through the fallback path explicitly via --quality-toml
        // so the test stays cwd-independent regardless of whether the
        // default `.config/scorecard/quality.toml` exists in the repo.
        let dir = tempdir();
        let pr_path = dir.path.join("pr.json");
        let out_path = dir.path.join("scorecard.json");
        let absent_toml = dir.path.join("absent.toml");
        fs::write(
            &pr_path,
            r#"{"pr_number":1,"head_sha":"x","base_sha":"y","is_fork":false}"#,
        )
        .unwrap();
        let code = run([
            OsString::from("--pr-meta"),
            OsString::from(pr_path.as_os_str()),
            OsString::from("--coverage-delta-pp"),
            OsString::from("-2.5"),
            OsString::from("--quality-toml"),
            OsString::from(absent_toml.as_os_str()),
            OsString::from("--out"),
            OsString::from(out_path.as_os_str()),
        ]);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out_path.exists());
        // Round-trip sanity: the artifact carries the new fields and
        // resolves to the expected status for the supplied delta.
        let content = fs::read_to_string(&out_path).expect("read back");
        let parsed: Value = serde_json::from_str(&content).expect("valid json");
        assert_eq!(parsed["overall_status"], "Yellow");
        assert_eq!(parsed["fallback_thresholds_active"], true);
    }
}
