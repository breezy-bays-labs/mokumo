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
    self, BddSkipThresholds, CiWallClockThresholds, CoverageThresholds, FlakyPopulationThresholds,
    ThresholdConfig,
};
use crate::{
    BddCrateBreakout, Breakouts, GateRun, PrMeta, Row, RowCommon, Scorecard, Status, TagCount,
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
    /// multiple roots. When the flag is omitted the producer emits a
    /// `BddSkipCount` row with `0 skipped / 0 total` rather than a
    /// producer-pending stub — the row is wired even on a corpus-less
    /// run.
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
        | Row::BddSkipCount { status, .. }
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
// V4 (#769) §4 wired row. The producer walks operator-supplied
// `.feature` directory roots, parses each file's tag stack and scenario
// keywords, and aggregates per-crate breakouts. The threshold resolver
// in `threshold::resolve_bdd_skip` maps the total `skipped` count to a
// [`Status`].

/// Tags that mark a scenario as skipped from execution. Matches the
/// cucumber-rs convention used across the workspace.
const BDD_SKIP_TAGS: &[&str] = &["@wip", "@future", "@ignore", "@skip"];

/// Tag prefix that marks a scenario as tracked-but-deferred. Tag
/// payloads after the colon (`@tracked:mokumo#123`) act as upstream
/// issue references the renderer can autolink.
const BDD_TRACKED_TAG_PREFIX: &str = "@tracked:";

/// Aggregated BDD corpus statistics computed from one or more
/// `.feature` files. Pure-data input to [`build_bdd_skip_row`] —
/// callers either build it via [`discover_bdd_corpus`] (CLI path) or
/// hand-roll it for unit tests.
#[derive(Debug, Default, Clone)]
pub struct BddSummary {
    /// Total scenarios across the corpus (`Scenario:` +
    /// `Scenario Outline:` + `Example:`).
    pub total_scenarios: u32,
    /// Scenarios bearing at least one tag in [`BDD_SKIP_TAGS`] or with
    /// the [`BDD_TRACKED_TAG_PREFIX`] prefix.
    pub skipped: u32,
    /// Per-crate breakdown. Sorted by `crate_name` for deterministic
    /// artifacts.
    pub breakouts: Vec<BddCrateBreakout>,
}

/// `true` when a tag literal counts toward `skipped`.
fn is_bdd_skip_tag(tag: &str) -> bool {
    BDD_SKIP_TAGS.contains(&tag) || tag.starts_with(BDD_TRACKED_TAG_PREFIX)
}

#[derive(Debug, Default)]
struct ParsedFeature {
    total: u32,
    skipped: u32,
    by_tag: std::collections::BTreeMap<String, u32>,
}

/// Parse a `.feature` file body into per-file scenario / skip counts.
///
/// Recognises Gherkin-style tag lines (one or more `@...` tokens),
/// `Feature:` / `Rule:` / `Scenario:` / `Scenario Outline:` /
/// `Example:` keywords. Feature-level tags (those above `Feature:`)
/// apply to every scenario in the file. Step / docstring / table /
/// comment lines are ignored.
///
/// Not a full Gherkin parser — good enough for counting + tagging.
fn parse_feature(contents: &str) -> ParsedFeature {
    let mut feature_tags: Vec<String> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut feature_seen = false;
    let mut parsed = ParsedFeature::default();

    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('@') {
            for tok in line.split_whitespace() {
                if tok.starts_with('@') {
                    pending.push(tok.to_string());
                }
            }
            continue;
        }
        if line.starts_with("Feature:") || line.starts_with("Rule:") {
            if !feature_seen {
                feature_tags = std::mem::take(&mut pending);
                feature_seen = true;
            } else {
                pending.clear();
            }
            continue;
        }
        if line.starts_with("Scenario:")
            || line.starts_with("Scenario Outline:")
            || line.starts_with("Example:")
        {
            parsed.total += 1;
            let mut effective = feature_tags.clone();
            effective.append(&mut pending);
            let mut is_skipped = false;
            for tag in &effective {
                *parsed.by_tag.entry(tag.clone()).or_insert(0) += 1;
                if is_bdd_skip_tag(tag) {
                    is_skipped = true;
                }
            }
            if is_skipped {
                parsed.skipped += 1;
            }
            continue;
        }
        // Background / step / examples / docstring / table — clear
        // pending tags so a stray `@` line followed by a non-Scenario
        // keyword does not bleed into the next scenario.
        if line.starts_with("Background:") {
            pending.clear();
        }
    }

    parsed
}

/// Derive a crate / app name from a `.feature` file path.
///
/// Looks for `crates/<name>/...` or `apps/<name>/...` segments in the
/// path. Falls back to `"unknown"` when no recognisable workspace
/// segment is present (rare — only happens on hand-fed test fixtures).
fn crate_name_from_path(path: &Path) -> String {
    let parts: Vec<_> = path.components().collect();
    for (i, c) in parts.iter().enumerate() {
        if let std::path::Component::Normal(s) = c {
            let s = s.to_string_lossy();
            if (s == "crates" || s == "apps") && i + 1 < parts.len() {
                if let std::path::Component::Normal(name) = &parts[i + 1] {
                    return name.to_string_lossy().into_owned();
                }
            }
        }
    }
    "unknown".into()
}

/// Walk one or more roots for `.feature` files and aggregate the BDD
/// corpus into a [`BddSummary`].
///
/// Returns an error when a discovered file cannot be read; missing
/// roots are silently skipped (an empty `--bdd-features-root` set
/// produces an empty summary).
pub fn discover_bdd_corpus(roots: &[PathBuf]) -> Result<BddSummary, String> {
    use std::collections::BTreeMap;

    type CrateBucket = (u32, u32, BTreeMap<String, u32>);
    let mut per_crate: BTreeMap<String, CrateBucket> = BTreeMap::new();
    let mut total = 0u32;
    let mut skipped = 0u32;

    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.path().extension().and_then(|s| s.to_str()) != Some("feature") {
                continue;
            }
            let contents = fs::read_to_string(entry.path()).map_err(|e| {
                format!(
                    "aggregate: failed to read feature file {}: {e}",
                    entry.path().display()
                )
            })?;
            let parsed = parse_feature(&contents);
            let crate_name = crate_name_from_path(entry.path());
            total += parsed.total;
            skipped += parsed.skipped;
            let bucket = per_crate.entry(crate_name).or_default();
            bucket.0 += parsed.total;
            bucket.1 += parsed.skipped;
            for (tag, n) in parsed.by_tag {
                *bucket.2.entry(tag).or_insert(0) += n;
            }
        }
    }

    let breakouts = per_crate
        .into_iter()
        .map(|(crate_name, (total, skipped, by_tag))| BddCrateBreakout {
            crate_name,
            total,
            skipped,
            by_tag: by_tag
                .into_iter()
                .map(|(tag, count)| TagCount { tag, count })
                .collect(),
        })
        .collect();

    Ok(BddSummary {
        total_scenarios: total,
        skipped,
        breakouts,
    })
}

/// Render the inline failure detail for a Red BDD skip-count row.
fn bdd_failure_detail(skipped: u32, fail_threshold: u32) -> String {
    format!("BDD skip count is {skipped} — at or above the {fail_threshold} fail threshold.")
}

/// Format the `delta_text` for a wired BDD skip row.
fn bdd_delta_text(total: u32, skipped: u32) -> String {
    format!("{skipped} skipped / {total} total")
}

/// Build a wired `Row::BddSkipCount` from a corpus summary + thresholds.
pub fn build_bdd_skip_row(summary: &BddSummary, thresholds: &BddSkipThresholds) -> Row {
    let common = RowCommon {
        id: "bdd_skip".into(),
        label: "BDD skips".into(),
        anchor: "bdd-skip".into(),
    };
    let delta_text = bdd_delta_text(summary.total_scenarios, summary.skipped);
    match threshold::resolve_bdd_skip(summary.skipped, thresholds) {
        Status::Green => Row::bdd_skip_count_green(
            common,
            summary.total_scenarios,
            summary.skipped,
            summary.breakouts.clone(),
            delta_text,
        ),
        Status::Yellow => Row::bdd_skip_count_yellow(
            common,
            summary.total_scenarios,
            summary.skipped,
            summary.breakouts.clone(),
            delta_text,
        ),
        Status::Red => Row::bdd_skip_count_red(
            common,
            summary.total_scenarios,
            summary.skipped,
            summary.breakouts.clone(),
            delta_text,
            bdd_failure_detail(summary.skipped, thresholds.fail_skipped),
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

/// Format the `delta_text` for a wired CI wall-clock row.
fn ci_wall_clock_delta_text(json: &CiWallClockJson, delta_seconds: f64) -> String {
    match json.base_total_seconds {
        Some(_) => {
            let sign = if delta_seconds >= 0.0 { "+" } else { "" };
            format!(
                "{:.0}s total / {sign}{delta_seconds:.0}s vs base",
                json.total_seconds
            )
        }
        None => format!("{:.0}s total / (no base)", json.total_seconds),
    }
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
    if !parsed.total_seconds.is_finite() {
        return Err(format!(
            "aggregate: --ci-wall-clock-json {} total_seconds must be finite, got {}",
            path.display(),
            parsed.total_seconds
        ));
    }
    if let Some(base) = parsed.base_total_seconds {
        if !base.is_finite() {
            return Err(format!(
                "aggregate: --ci-wall-clock-json {} base_total_seconds must be finite, got {base}",
                path.display(),
            ));
        }
    }
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

/// Walk one or more roots for source files and count `// FLAKY:`
/// markers. Reports a clear error when a discovered file cannot be
/// read; missing roots are silently skipped.
pub fn discover_flaky_corpus(
    source_roots: &[PathBuf],
    retry_json: Option<&NextestRetryJson>,
) -> Result<FlakyCorpus, String> {
    let mut marker_count = 0u32;
    for root in source_roots {
        if !root.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let ext = entry.path().extension().and_then(|s| s.to_str());
            if !ext.is_some_and(|e| FLAKY_SCAN_EXTENSIONS.contains(&e)) {
                continue;
            }
            let contents = fs::read_to_string(entry.path()).map_err(|e| {
                format!(
                    "aggregate: failed to read source file {}: {e}",
                    entry.path().display()
                )
            })?;
            for line in contents.lines() {
                if line.contains(FLAKY_MARKER) {
                    marker_count = marker_count.saturating_add(1);
                }
            }
        }
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
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let p = Path::new(trimmed);
        let name = crate_name_from_path(p);
        seen.insert(name);
    }
    let truncated = seen.len() > CHANGED_SCOPE_NODE_LIMIT;
    let touched: Vec<String> = seen.into_iter().take(CHANGED_SCOPE_NODE_LIMIT).collect();
    Ok(Some(ChangedScope { touched, truncated }))
}

/// Render the Mermaid `graph LR` body for a [`ChangedScope`].
///
/// Empty scope yields `graph LR\n  empty[(no diff)]` so the row
/// always carries a non-empty Mermaid body — operators see the row
/// shape even when the PR does not change any tracked file.
fn render_changed_scope_mermaid(scope: &ChangedScope) -> String {
    if scope.touched.is_empty() {
        return "graph LR\n  empty[\"(no diff)\"]".to_string();
    }
    let mut out = String::from("graph LR\n  changed[\"Changed scope\"]\n");
    for (i, name) in scope.touched.iter().enumerate() {
        // Mermaid node ids must be alphanumeric / underscore. Keep the
        // pretty crate name in the label and use a deterministic id.
        out.push_str(&format!("  changed --> n{i}[\"{name}\"]\n"));
    }
    if scope.truncated {
        out.push_str(&format!(
            "  changed --> truncated[\"(diagram truncated at {CHANGED_SCOPE_NODE_LIMIT} nodes — see workflow logs)\"]\n",
        ));
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
    let node_count = scope.touched.len() as u32;
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
    let bdd = build_bdd_skip_row(bdd_summary, &thresholds.rows.bdd_skip);
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
        bdd,
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

/// Drive the CLI from raw OS args. Extracted for testability.
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let cli = match parse_cli(args) {
        Ok(c) => c,
        Err(code) => return code,
    };

    let pr = match read_pr_meta(&cli.pr_meta) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{msg}");
            // Missing/invalid --pr-meta is a usage failure for the
            // caller, exit 2 (per session prompt: "rejects invalid
            // --pr-meta paths with a clear error (exit code 2)").
            return ExitCode::from(2);
        }
    };

    let source = match resolve_threshold_source(&cli.quality_toml) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(1);
        }
    };
    let bdd_summary = match discover_bdd_corpus(&cli.bdd_features_roots) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(1);
        }
    };
    let ci_wall_clock = match read_ci_wall_clock_json(cli.ci_wall_clock_json.as_deref()) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(1);
        }
    };
    let nextest_retry = match read_nextest_retry_json(cli.nextest_retry_json.as_deref()) {
        Ok(r) => r,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(1);
        }
    };
    let flaky_corpus = match discover_flaky_corpus(&cli.flaky_source_roots, nextest_retry.as_ref())
    {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(1);
        }
    };
    let changed_scope = match read_changed_scope(cli.changed_files.as_deref()) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(1);
        }
    };
    let scorecard = build_scorecard(
        pr,
        cli.coverage_delta_pp,
        &bdd_summary,
        ci_wall_clock.as_ref(),
        &flaky_corpus,
        changed_scope.as_ref(),
        &source.config(),
        source.fallback_active(),
    );
    if let Err(msg) = write_scorecard(&scorecard, &cli.out) {
        eprintln!("{msg}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
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
        // V4 emits the coverage row + four wired rows (BddSkipCount,
        // CiWallClockDelta, FlakyPopulation, ChangedScopeDiagram) +
        // four producer-pending stubs (CrapDelta, MutationSurvivors,
        // HandlerCoverageAxis, GateRuns) — nine rows total.
        let sc = build_with_delta(0.3);
        assert_eq!(sc.rows.len(), 9);
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
    fn build_scorecard_emits_bdd_skip_row_after_coverage() {
        // BDD skip is the second row in the artifact — wired in C3 and
        // sourced from `BddSummary`. Empty summary lands Green.
        let sc = build_with_delta(0.3);
        let Row::BddSkipCount {
            status,
            total_scenarios,
            skipped,
            ..
        } = &sc.rows[1]
        else {
            panic!("expected BddSkipCount as the second row")
        };
        assert_eq!(*status, Status::Green);
        assert_eq!(*total_scenarios, 0);
        assert_eq!(*skipped, 0);
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
        // CoverageDelta + four wired rows + four producer-pending stubs.
        assert_eq!(parsed["rows"].as_array().map(|a| a.len()), Some(9));
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
        let sc = build_with_delta(0.3);
        let Row::ChangedScopeDiagram {
            status, node_count, ..
        } = &sc.rows[4]
        else {
            panic!("expected ChangedScopeDiagram as the fifth row")
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
        let corpus = discover_flaky_corpus(&[dir.path.clone()], None).expect("discover ok");
        assert_eq!(corpus.marker_count, 3);
        assert_eq!(corpus.retry_events, 0);
    }

    #[test]
    fn discover_flaky_corpus_picks_up_retry_count_from_json() {
        let dir = tempdir();
        let retry = NextestRetryJson { retry_count: 4 };
        let corpus = discover_flaky_corpus(&[dir.path.clone()], Some(&retry)).expect("discover");
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
        let sc = build_with_delta(0.3);
        let Row::FlakyPopulation {
            status,
            flaky_marker_count,
            ..
        } = &sc.rows[3]
        else {
            panic!("expected FlakyPopulation as the fourth row")
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
        let sc = build_with_delta(0.3);
        let Row::CiWallClockDelta {
            status,
            total_ci_seconds,
            delta_seconds,
            ..
        } = &sc.rows[2]
        else {
            panic!("expected CiWallClockDelta as the third row")
        };
        assert_eq!(*status, Status::Green);
        assert_eq!(*total_ci_seconds, 0.0);
        assert_eq!(*delta_seconds, 0.0);
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
        assert_eq!(parsed.total, 1);
        assert_eq!(parsed.skipped, 0);
    }

    #[test]
    fn parse_feature_counts_skipped_scenarios_via_wip_tag() {
        let body = r#"
Feature: example

  @wip
  Scenario: deferred
    Given a step

  Scenario: shipping
    Given another step
"#;
        let parsed = parse_feature(body);
        assert_eq!(parsed.total, 2);
        assert_eq!(parsed.skipped, 1);
        assert_eq!(parsed.by_tag.get("@wip"), Some(&1));
    }

    #[test]
    fn parse_feature_counts_tracked_prefix_as_skipped() {
        let body = r#"
Feature: example

  @tracked:mokumo#123
  Scenario: deferred
    Given a step
"#;
        let parsed = parse_feature(body);
        assert_eq!(parsed.skipped, 1);
        assert_eq!(parsed.by_tag.get("@tracked:mokumo#123"), Some(&1));
    }

    #[test]
    fn parse_feature_propagates_feature_level_tags_to_each_scenario() {
        // Feature-level tags above `Feature:` apply to every scenario.
        let body = r#"
@feature-tag
Feature: example

  Scenario: alpha
    Given a

  Scenario: beta
    Given b
"#;
        let parsed = parse_feature(body);
        assert_eq!(parsed.total, 2);
        assert_eq!(parsed.skipped, 0);
        assert_eq!(parsed.by_tag.get("@feature-tag"), Some(&2));
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
        assert_eq!(parsed.total, 2);
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
        assert_eq!(parsed.total, 1);
    }

    #[test]
    fn build_bdd_skip_row_green_below_warn_threshold() {
        let summary = BddSummary {
            total_scenarios: 100,
            skipped: 10,
            breakouts: vec![],
        };
        let row = build_bdd_skip_row(&summary, &BddSkipThresholds::default());
        let Row::BddSkipCount {
            status,
            total_scenarios,
            skipped,
            delta_text,
            failure_detail_md,
            ..
        } = row
        else {
            panic!("expected BddSkipCount")
        };
        assert_eq!(status, Status::Green);
        assert_eq!(total_scenarios, 100);
        assert_eq!(skipped, 10);
        assert_eq!(delta_text, "10 skipped / 100 total");
        assert!(failure_detail_md.is_none());
    }

    #[test]
    fn build_bdd_skip_row_yellow_at_warn_threshold() {
        let summary = BddSummary {
            total_scenarios: 100,
            skipped: 50,
            breakouts: vec![],
        };
        let row = build_bdd_skip_row(&summary, &BddSkipThresholds::default());
        assert!(matches!(
            row,
            Row::BddSkipCount {
                status: Status::Yellow,
                ..
            }
        ));
    }

    #[test]
    fn build_bdd_skip_row_red_at_fail_threshold_carries_detail() {
        let summary = BddSummary {
            total_scenarios: 500,
            skipped: 200,
            breakouts: vec![],
        };
        let row = build_bdd_skip_row(&summary, &BddSkipThresholds::default());
        let Row::BddSkipCount {
            status,
            failure_detail_md,
            ..
        } = row
        else {
            panic!("expected BddSkipCount")
        };
        assert_eq!(status, Status::Red);
        let detail = failure_detail_md.expect("Red rows carry failure_detail_md");
        assert!(detail.contains("200"), "got: {detail}");
        assert!(detail.contains("at or above"), "got: {detail}");
    }

    #[test]
    fn discover_bdd_corpus_walks_feature_files_in_root() {
        let dir = tempdir();
        let crate_dir = dir.path.join("crates/example/tests/features");
        fs::create_dir_all(&crate_dir).expect("mkdir");
        fs::write(
            crate_dir.join("a.feature"),
            "Feature: a\n\n  @wip\n  Scenario: alpha\n    Given x\n",
        )
        .expect("write a");
        fs::write(
            crate_dir.join("b.feature"),
            "Feature: b\n\n  Scenario: beta\n    Given y\n",
        )
        .expect("write b");

        let summary = discover_bdd_corpus(&[dir.path.clone()]).expect("walk");
        assert_eq!(summary.total_scenarios, 2);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.breakouts.len(), 1);
        assert_eq!(summary.breakouts[0].crate_name, "example");
        assert_eq!(summary.breakouts[0].total, 2);
        assert_eq!(summary.breakouts[0].skipped, 1);
    }

    #[test]
    fn discover_bdd_corpus_returns_empty_for_missing_root() {
        let dir = tempdir();
        let missing = dir.path.join("nope");
        let summary = discover_bdd_corpus(&[missing]).expect("missing root is empty corpus");
        assert_eq!(summary.total_scenarios, 0);
        assert_eq!(summary.skipped, 0);
        assert!(summary.breakouts.is_empty());
    }

    #[test]
    fn crate_name_from_path_extracts_crate_segment() {
        let p = Path::new("crates/mokumo-shop/tests/features/quote.feature");
        assert_eq!(crate_name_from_path(p), "mokumo-shop");
    }

    #[test]
    fn crate_name_from_path_extracts_apps_segment() {
        let p = Path::new("apps/web/tests/customer.feature");
        assert_eq!(crate_name_from_path(p), "web");
    }

    #[test]
    fn crate_name_from_path_falls_back_when_no_recognised_segment() {
        let p = Path::new("/tmp/random/path.feature");
        assert_eq!(crate_name_from_path(p), "unknown");
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
