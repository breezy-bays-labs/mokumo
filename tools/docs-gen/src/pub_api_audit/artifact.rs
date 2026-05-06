//! Wire shape for the public-API spec audit artifact.
//!
//! Two-form output: a structured JSON the gate consumes
//! ([`PubApiAuditArtifact`]) and a human-readable Markdown view rendered
//! from the same struct. Both are deterministic — sorted by
//! `(crate, item_path)` — so the artifact diffs cleanly when committed.

use serde::{Deserialize, Serialize};

/// Highest artifact version this consumer accepts. Mirrors
/// [`crate::coverage::artifact::ARTIFACT_VERSION_MAX`] discipline — bump
/// only on a non-additive wire change.
pub const ARTIFACT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PubApiAuditArtifact {
    pub version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub by_crate: Vec<CratePubItems>,
    #[serde(default)]
    pub diagnostics: Diagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CratePubItems {
    pub crate_name: String,
    pub items: Vec<PubItemEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PubItemEntry {
    /// Fully-qualified Rust path, e.g. `mokumo_shop::customer::Customer`
    /// or `mokumo_shop::customer::CustomerService::list`. Sorted by the
    /// producer; the gate uses this as the stable key.
    pub item_path: String,
    /// One of `fn`, `struct`, `enum`, `trait`, `const`, `static`, `type`,
    /// `method`. Methods are emitted with their `impl` parent merged
    /// into the path.
    pub kind: String,
    /// Source file, repo-relative (e.g. `crates/mokumo-shop/src/customer/domain.rs`).
    pub source_file: String,
    /// 1-based start line of the item declaration.
    pub source_line_start: u32,
    /// 1-based end line of the item declaration (inclusive).
    pub source_line_end: u32,
    /// Number of source lines in the item's span that have lcov hit ≥ 1.
    /// 0 → uncovered, ≥ 1 → covered (gate's binary attribution).
    #[serde(default)]
    pub bdd_covered_lines: u32,
    /// Total source lines in the item's span (line_end - line_start + 1).
    #[serde(default)]
    pub bdd_total_lines: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Diagnostics {
    /// Crates skipped because they sit on `crap4rs.toml`'s exclusion
    /// list — same posture as the per-route producer (mokumo#655).
    #[serde(default)]
    pub excluded_crates: Vec<String>,
    /// Files the syn walker could not parse end-to-end. Each entry pins
    /// the file + reason. The producer continues with what it could
    /// parse so a single bad file doesn't lose a whole run.
    #[serde(default)]
    pub parse_errors: Vec<ParseError>,
    /// Lcov files the loader could not parse. Same posture as parse_errors:
    /// continue with what we have, surface the failure.
    #[serde(default)]
    pub lcov_errors: Vec<LcovError>,
    /// Total pub items walked across all crates. Useful for "did the
    /// walker see anything?" sanity checks in CI.
    #[serde(default)]
    pub items_walked: u64,
    /// Total lcov files consumed.
    #[serde(default)]
    pub lcov_files_consumed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseError {
    pub file: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LcovError {
    pub file: String,
    pub reason: String,
}

impl PubItemEntry {
    /// Binary "covered or not" call used by the fail-closed gate. An
    /// item is covered when any line in its span has lcov hit ≥ 1.
    #[must_use]
    pub fn is_bdd_covered(&self) -> bool {
        self.bdd_covered_lines > 0
    }
}
