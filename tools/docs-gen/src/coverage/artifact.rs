//! Producer-artifact wire types — the JSON shape `coverage-breakouts` emits
//! and `scorecard aggregate --coverage-breakouts-json` consumes.
//!
//! Stable contract between two binaries that ship in the same repo;
//! versioned so a future schema bump (e.g. method-axis split) can be
//! detected without ambiguity. Distinct from the scorecard wire schema —
//! the aggregator translates `HandlerArtifactEntry` → `HandlerBreakout`,
//! dropping the producer-internal fields (`rust_path`, branch counts).

use serde::{Deserialize, Serialize};

/// Producer artifact format version. Increment on any breaking change to
/// the JSON shape; the aggregator validates this on read and refuses
/// artifacts with a higher version than it knows.
pub const ARTIFACT_VERSION: u32 = 1;

/// Top-level producer artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoverageBreakoutArtifact {
    /// Producer-artifact format version (see [`ARTIFACT_VERSION`]).
    pub version: u32,
    /// ISO-8601 UTC timestamp set by the producer at emit time.
    pub generated_at: String,
    /// Per-crate handler entries, keyed by crate name (Rust ident form,
    /// hyphens → underscores). Sorted by crate name for deterministic
    /// output across runs.
    pub by_crate: Vec<CrateHandlerSet>,
    /// Producer diagnostics — handlers found in routes but missing from
    /// coverage, or vice-versa. Empty in a healthy run; non-empty entries
    /// cause the producer to exit non-zero.
    pub diagnostics: Diagnostics,
}

/// One crate's handler set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrateHandlerSet {
    /// Crate name in Rust-ident form (e.g. `mokumo_shop`, `kikan`).
    pub crate_name: String,
    /// Per-handler coverage entries, sorted by `(method, route)` for
    /// deterministic output.
    pub handlers: Vec<HandlerArtifactEntry>,
}

/// Per-handler coverage entry — producer-internal shape including fields
/// the wire schema doesn't carry (`rust_path`, branch counts). The
/// aggregator translates this into the wire `HandlerBreakout`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandlerArtifactEntry {
    /// HTTP method + URL path (e.g. `"POST /api/users"`). Matches the
    /// existing `HandlerCoverageAxis::handler` format so the two axes
    /// can be cross-referenced in the renderer.
    pub route: String,
    /// Fully-qualified Rust path of the handler (e.g.
    /// `"mokumo_shop::user::handler::create_user"`). Stays in the producer
    /// artifact only — wire `HandlerBreakout` carries just the route.
    pub rust_path: String,
    /// Source file holding the handler definition (absolute path on the
    /// build runner; relative-to-workspace would be cleaner but
    /// `cargo llvm-cov`'s `filenames[]` are absolute).
    pub filename: String,
    /// Total branch sides counted by LLVM (= `2 * branches.len()` for the
    /// handler function). Each conditional contributes both true and
    /// false sides.
    pub branches_total: u64,
    /// Number of branch sides hit by at least one test execution.
    pub branches_covered: u64,
    /// Branch coverage percentage = `100 * covered / total`. **`100.0`
    /// when `branches_total == 0`** — a handler with no conditionals is
    /// vacuously fully covered. Encoding the empty case as `0.0` would
    /// trip the worst-of `[rows.coverage_handler]` gate against handlers
    /// that have nothing to cover, so the schema pins this to the
    /// passing end of the range.
    pub branch_coverage_pct: f64,
    /// Number of distinct LLVM functions matched for this handler.
    /// Usually 1; >1 indicates monomorphisation (generic handler) and
    /// branch counts are summed across instantiations.
    pub function_count: u32,
}

/// Producer diagnostics — non-empty entries cause `coverage-breakouts`
/// to exit non-zero with a structured error message naming the offenders.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Diagnostics {
    /// Routes whose handler symbol the route walker found but which is
    /// absent from the coverage payload. Most common cause: handler
    /// renamed in source without a coverage rerun.
    pub unresolved_handlers: Vec<UnresolvedHandler>,
    /// Routes whose handler couldn't be resolved at the source level
    /// (bare ident with no matching `use`, or qualified path that
    /// doesn't lead anywhere). Producer fails on these.
    pub unresolvable_routes: Vec<UnresolvableRoute>,
    /// Crates excluded from analysis per `crap4rs.toml`. Reported for
    /// auditability; doesn't fail the producer.
    pub excluded_crates: Vec<String>,
}

/// A route whose handler symbol exists in source but not in coverage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnresolvedHandler {
    pub route: String,
    pub rust_path: String,
    pub source_file: String,
    pub source_line: u32,
}

/// A route whose handler symbol couldn't be resolved at source level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnresolvableRoute {
    pub route_literal: String,
    pub source_file: String,
    pub source_line: u32,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_round_trips_through_serde_json() {
        let original = CoverageBreakoutArtifact {
            version: ARTIFACT_VERSION,
            generated_at: "2026-05-04T00:00:00Z".to_string(),
            by_crate: vec![CrateHandlerSet {
                crate_name: "kikan".to_string(),
                handlers: vec![HandlerArtifactEntry {
                    route: "POST /api/users".to_string(),
                    rust_path: "kikan::user::create".to_string(),
                    filename: "/workspace/crates/kikan/src/user.rs".to_string(),
                    branches_total: 16,
                    branches_covered: 14,
                    branch_coverage_pct: 87.5,
                    function_count: 1,
                }],
            }],
            diagnostics: Diagnostics::default(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: CoverageBreakoutArtifact = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn diagnostics_default_is_all_empty() {
        let d = Diagnostics::default();
        assert!(d.unresolved_handlers.is_empty());
        assert!(d.unresolvable_routes.is_empty());
        assert!(d.excluded_crates.is_empty());
    }
}
