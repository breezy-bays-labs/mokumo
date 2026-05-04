//! Decode the per-handler coverage producer artifact (mokumo#583).
//!
//! The producer (`tools/docs-gen --bin coverage-breakouts`) emits a JSON
//! artifact pairing each handler route with its branch-coverage stats.
//! This module mirrors the wire shape so the aggregator can consume the
//! artifact without taking a build-time dependency on `docs-gen`.
//!
//! **Wire contract** — must stay byte-compatible with
//! [`docs_gen::coverage::artifact::CoverageBreakoutArtifact`]. Field
//! adds in the producer must add corresponding fields here (with
//! `#[serde(default)]` so a producer at version `N+1` doesn't break a
//! consumer at version `N`). Field removes are an additive-rejection
//! event and require a version bump on both sides.
//!
//! Translation flow:
//!
//! ```text
//!  coverage-breakouts.json
//!         │
//!         ▼
//!   parse_artifact()  ──►  CoverageBreakoutArtifact (this module)
//!         │
//!         ▼
//!   to_wire_breakouts()  ──►  Breakouts { by_crate[], handlers[] }
//!         │                              (lib.rs wire types)
//!         ▼
//!   build_coverage_row()  consumes wire types directly
//! ```
//!
//! Diagnostics from the producer (unresolved handlers / unresolvable
//! routes) are surfaced as `failure_detail_md` on a Red row when the
//! gate is enforcing — see the threshold module.

use crate::{Breakouts, CrateBreakout, HandlerBreakout};
use serde::Deserialize;
use std::path::Path;

/// Result alias for the artifact reader. Uses `String` rather than
/// `anyhow::Error` so the scorecard lib's deps-zero-by-default invariant
/// (serde + schemars + serde_json only) holds — anyhow joins the deps
/// only when the optional `cli` feature is enabled, and the breakouts
/// reader is callable from non-cli paths (BDD step-defs at minimum).
pub type Result<T> = std::result::Result<T, String>;

/// Highest producer artifact version this consumer accepts. Lower
/// versions parse with `serde`'s default-on-missing behavior; a higher
/// version is a hard parse error so a producer ahead of the consumer
/// surfaces as a build failure rather than silent wrong data.
pub const ARTIFACT_VERSION_MAX: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
pub struct CoverageBreakoutArtifact {
    pub version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub by_crate: Vec<CrateHandlerSet>,
    #[serde(default)]
    pub diagnostics: Diagnostics,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CrateHandlerSet {
    pub crate_name: String,
    #[serde(default)]
    pub handlers: Vec<HandlerArtifactEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HandlerArtifactEntry {
    pub route: String,
    #[serde(default)]
    pub rust_path: String,
    #[serde(default)]
    pub filename: String,
    pub branch_coverage_pct: f64,
    #[serde(default)]
    pub branches_total: u64,
    #[serde(default)]
    pub branches_covered: u64,
    #[serde(default)]
    pub function_count: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Diagnostics {
    #[serde(default)]
    pub unresolved_handlers: Vec<UnresolvedHandler>,
    #[serde(default)]
    pub unresolvable_routes: Vec<UnresolvableRoute>,
    #[serde(default)]
    pub excluded_crates: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnresolvedHandler {
    pub route: String,
    #[serde(default)]
    pub rust_path: String,
    #[serde(default)]
    pub source_file: String,
    #[serde(default)]
    pub source_line: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnresolvableRoute {
    pub route_literal: String,
    #[serde(default)]
    pub source_file: String,
    #[serde(default)]
    pub source_line: u32,
    #[serde(default)]
    pub reason: String,
}

/// Parse the producer artifact from a JSON string.
pub fn parse_artifact(raw: &str) -> Result<CoverageBreakoutArtifact> {
    let parsed: CoverageBreakoutArtifact = serde_json::from_str(raw)
        .map_err(|e| format!("decoding coverage-breakouts artifact: {e}"))?;
    if parsed.version > ARTIFACT_VERSION_MAX {
        return Err(format!(
            "coverage-breakouts artifact version {} exceeds consumer max {} — bump the consumer or downgrade the producer",
            parsed.version, ARTIFACT_VERSION_MAX,
        ));
    }
    Ok(parsed)
}

/// Read + parse a producer artifact from disk.
pub fn read_artifact(path: &Path) -> Result<CoverageBreakoutArtifact> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    parse_artifact(&raw).map_err(|e| format!("parsing {}: {e}", path.display()))
}

/// Translate a producer artifact into the wire `Breakouts` shape.
///
/// Drops producer-internal fields (`rust_path`, `filename`, branch
/// counts, `function_count`) — the renderer only needs `(handler,
/// branch_coverage_pct)` pairs.
///
/// `line_delta_pp` per crate is set to `0.0` for now; a future producer
/// will join the per-crate base/head deltas to populate this. Leaving
/// the field at zero keeps the wire shape valid while the producer
/// catches up.
#[must_use]
pub fn to_wire_breakouts(artifact: &CoverageBreakoutArtifact) -> Breakouts {
    let mut by_crate: Vec<CrateBreakout> = artifact
        .by_crate
        .iter()
        .map(|c| {
            let mut handlers: Vec<HandlerBreakout> = c
                .handlers
                .iter()
                .map(|h| HandlerBreakout {
                    handler: h.route.clone(),
                    branch_coverage_pct: h.branch_coverage_pct,
                })
                .collect();
            handlers.sort_by(|a, b| a.handler.cmp(&b.handler));
            CrateBreakout {
                crate_name: c.crate_name.clone(),
                line_delta_pp: 0.0,
                handlers,
            }
        })
        .collect();
    by_crate.sort_by(|a, b| a.crate_name.cmp(&b.crate_name));
    Breakouts { by_crate }
}

/// Iterate every handler's branch coverage % across every crate. Used
/// by the threshold gate to compute the worst-of verdict.
pub fn iter_handler_pcts(artifact: &CoverageBreakoutArtifact) -> impl Iterator<Item = f64> + '_ {
    artifact
        .by_crate
        .iter()
        .flat_map(|c| c.handlers.iter().map(|h| h.branch_coverage_pct))
}

/// Whether the producer artifact reports any operator-actionable
/// diagnostics (unresolved handlers, unresolvable routes). Excluded
/// crates are intentionally NOT actionable — they're informational.
#[must_use]
pub fn has_actionable_diagnostics(artifact: &CoverageBreakoutArtifact) -> bool {
    !artifact.diagnostics.unresolved_handlers.is_empty()
        || !artifact.diagnostics.unresolvable_routes.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> &'static str {
        r#"{
            "version": 1,
            "generated_at": "2026-05-04T00:00:00Z",
            "by_crate": [
                {
                    "crate_name": "kikan",
                    "handlers": [
                        {
                            "route": "POST /api/users",
                            "rust_path": "kikan::user::create",
                            "filename": "/x/user.rs",
                            "branches_total": 16,
                            "branches_covered": 14,
                            "branch_coverage_pct": 87.5,
                            "function_count": 1
                        },
                        {
                            "route": "GET /api/users/{id}",
                            "rust_path": "kikan::user::show",
                            "filename": "/x/user.rs",
                            "branches_total": 4,
                            "branches_covered": 4,
                            "branch_coverage_pct": 100.0,
                            "function_count": 1
                        }
                    ]
                }
            ],
            "diagnostics": {
                "unresolved_handlers": [],
                "unresolvable_routes": [],
                "excluded_crates": ["kikan-tauri"]
            }
        }"#
    }

    #[test]
    fn parses_well_formed_artifact() {
        let a = parse_artifact(sample_json()).unwrap();
        assert_eq!(a.version, 1);
        assert_eq!(a.by_crate.len(), 1);
        assert_eq!(a.by_crate[0].handlers.len(), 2);
    }

    #[test]
    fn rejects_higher_version_than_consumer_supports() {
        let raw = r#"{"version":99,"by_crate":[]}"#;
        let err = parse_artifact(raw).unwrap_err();
        assert!(err.contains("exceeds consumer max"), "{err}");
    }

    #[test]
    fn translates_to_wire_breakouts_dropping_internal_fields() {
        let a = parse_artifact(sample_json()).unwrap();
        let breakouts = to_wire_breakouts(&a);
        assert_eq!(breakouts.by_crate.len(), 1);
        let kikan = &breakouts.by_crate[0];
        assert_eq!(kikan.crate_name, "kikan");
        assert_eq!(kikan.handlers.len(), 2);
        // Sorted alphabetically by handler label.
        assert_eq!(kikan.handlers[0].handler, "GET /api/users/{id}");
        assert_eq!(kikan.handlers[1].handler, "POST /api/users");
        // Wire shape carries pct only — translation drops rust_path etc.
        assert!((kikan.handlers[1].branch_coverage_pct - 87.5).abs() < f64::EPSILON);
    }

    #[test]
    fn iter_handler_pcts_flattens_across_crates() {
        let a = parse_artifact(sample_json()).unwrap();
        let pcts: Vec<f64> = iter_handler_pcts(&a).collect();
        assert_eq!(pcts.len(), 2);
        assert!(pcts.contains(&87.5));
        assert!(pcts.contains(&100.0));
    }

    #[test]
    fn has_actionable_diagnostics_false_on_clean_run() {
        let a = parse_artifact(sample_json()).unwrap();
        assert!(!has_actionable_diagnostics(&a));
    }

    #[test]
    fn has_actionable_diagnostics_flags_unresolved_handlers() {
        let raw = r#"{
            "version": 1,
            "by_crate": [],
            "diagnostics": {
                "unresolved_handlers": [
                    {"route": "POST /x", "rust_path": "k::x", "source_file": "/y", "source_line": 1}
                ]
            }
        }"#;
        let a = parse_artifact(raw).unwrap();
        assert!(has_actionable_diagnostics(&a));
    }

    #[test]
    fn excluded_crates_alone_are_not_actionable() {
        let raw = r#"{
            "version": 1,
            "by_crate": [],
            "diagnostics": {"excluded_crates": ["kikan-tauri"]}
        }"#;
        let a = parse_artifact(raw).unwrap();
        assert!(!has_actionable_diagnostics(&a));
    }
}
