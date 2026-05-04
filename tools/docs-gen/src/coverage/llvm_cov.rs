//! Parse `cargo llvm-cov nextest --branch --json` output and compute
//! per-function branch coverage.
//!
//! The JSON shape is LLVM's coverage export format
//! (`llvm.coverage.json.export`, version 2.x). Per-function entries carry
//! a `branches[]` array where each entry is a 9-element tuple:
//!
//! ```text
//! [start_line, start_col, end_line, end_col,
//!  true_count, false_count,
//!  file_id, expanded_file_id, kind]
//! ```
//!
//! Branch coverage convention (matches LLVM/cargo-llvm-cov summaries):
//! - **total** = `2 * branches.len()` (each branch contributes 2 sides)
//! - **covered** = sum over branches of `(true_count > 0) + (false_count > 0)`
//! - **percent** = `100 * covered / total` (0.0 when total == 0)
//!
//! A partially-covered branch (one side hit, the other missed) registers
//! as 1-of-2 sides — this is exactly the negative-path signal Quinn's
//! blind-spot 3 wants surfaced. A handler at 50% branch coverage has tests
//! exercising every branch in only one direction; raising it to 100%
//! requires tests that cover both sides of every conditional.

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct CoverageJson {
    #[serde(rename = "type")]
    kind: String,
    version: String,
    data: Vec<CoverageData>,
}

#[derive(Debug, Deserialize)]
struct CoverageData {
    functions: Vec<RawFunctionCoverage>,
}

#[derive(Debug, Deserialize)]
struct RawFunctionCoverage {
    name: String,
    filenames: Vec<String>,
    /// Each branch entry is a 9-element tuple. Deserialize as `Vec<i64>`
    /// so a future schema extension that adds a 10th field doesn't break
    /// us — we only index 4 (true_count) and 5 (false_count).
    branches: Vec<Vec<i64>>,
    #[serde(default)]
    count: u64,
}

/// Coverage stats for one demangled function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCoverage {
    /// Demangled Rust path (e.g. `kikan::user::handler::create_user`).
    /// Generic monomorphisations have their type args stripped during
    /// merge — the path matches what the route walker emits.
    pub rust_path: String,
    /// Source file holding the function (absolute path).
    pub filename: String,
    /// Total branch sides (`2 * branches.len()`).
    pub branches_total: u64,
    /// Covered branch sides.
    pub branches_covered: u64,
    /// Number of distinct LLVM function entries merged into this row.
    /// Generics produce one entry per monomorphisation; we sum their
    /// branch stats and report `function_count` for diagnostic visibility.
    pub function_count: u32,
}

impl FunctionCoverage {
    /// Branch coverage percentage. Returns `0.0` when `branches_total == 0`.
    #[must_use]
    pub fn branch_coverage_pct(&self) -> f64 {
        if self.branches_total == 0 {
            return 0.0;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "branch counts in a single function are well below f64 mantissa precision"
        )]
        {
            100.0 * (self.branches_covered as f64) / (self.branches_total as f64)
        }
    }
}

/// Parsed coverage payload, indexed by demangled Rust path.
#[derive(Debug)]
pub struct CoverageIndex {
    by_path: HashMap<String, FunctionCoverage>,
}

impl CoverageIndex {
    /// Look up a function by Rust path. Bare-ident handlers must already
    /// be resolved upstream — this map is keyed by fully-qualified path.
    #[must_use]
    pub fn get(&self, rust_path: &str) -> Option<&FunctionCoverage> {
        self.by_path.get(rust_path)
    }

    /// All functions in the index. Iteration order is deterministic
    /// (sorted by rust_path) for reproducible producer output.
    pub fn iter_sorted(&self) -> impl Iterator<Item = &FunctionCoverage> {
        let mut v: Vec<_> = self.by_path.values().collect();
        v.sort_by(|a, b| a.rust_path.cmp(&b.rust_path));
        v.into_iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }
}

/// Parse a `cargo llvm-cov nextest --branch --json` output file and
/// produce an index keyed by demangled Rust path.
pub fn parse(path: &Path) -> Result<CoverageIndex> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

/// String-input variant of [`parse`] — exposed for tests + the BDD
/// harness that needs to compose fixtures inline.
pub fn parse_str(raw: &str) -> Result<CoverageIndex> {
    let json: CoverageJson = serde_json::from_str(raw).context("decoding coverage JSON")?;
    if json.kind != "llvm.coverage.json.export" {
        bail!(
            "unexpected coverage payload type `{}` (want `llvm.coverage.json.export`)",
            json.kind
        );
    }
    // LLVM's coverage JSON export schema has cycled through 2.x and 3.x;
    // both share the per-function `branches[]` shape this producer needs
    // (verified against rustc 1.97-nightly + cargo-llvm-cov 0.8.5 emitting
    // `3.1.0`). Accept any 2.x or 3.x; bail loudly on anything else so a
    // future schema break shows up as an explicit producer failure rather
    // than silently-wrong coverage.
    if !json.version.starts_with("2.") && !json.version.starts_with("3.") {
        bail!(
            "unexpected coverage payload version `{}` (want 2.x or 3.x)",
            json.version
        );
    }
    let data = json
        .data
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("coverage payload has no `data[]` entries"))?;

    // Demangle and merge by demangled path. Multiple LLVM function entries
    // can share a demangled path when generics are monomorphised; sum
    // their branch stats so the merged row reflects the function's
    // aggregate coverage.
    let mut by_path: HashMap<String, FunctionCoverage> = HashMap::new();
    for raw_fn in data.functions {
        let Some(demangled) = demangle_to_path(&raw_fn.name) else {
            // Skip entries we can't demangle (rare — e.g. C symbols from FFI shims).
            continue;
        };
        let filename = raw_fn
            .filenames
            .first()
            .cloned()
            .unwrap_or_else(|| "<unknown>".to_string());
        let total = (raw_fn.branches.len() as u64).saturating_mul(2);
        let covered = raw_fn
            .branches
            .iter()
            .map(|b| {
                let true_count = b.get(4).copied().unwrap_or(0);
                let false_count = b.get(5).copied().unwrap_or(0);
                u64::from(true_count > 0) + u64::from(false_count > 0)
            })
            .sum::<u64>();

        let entry = by_path
            .entry(demangled.clone())
            .or_insert_with(|| FunctionCoverage {
                rust_path: demangled,
                filename: filename.clone(),
                branches_total: 0,
                branches_covered: 0,
                function_count: 0,
            });
        entry.branches_total = entry.branches_total.saturating_add(total);
        entry.branches_covered = entry.branches_covered.saturating_add(covered);
        entry.function_count = entry.function_count.saturating_add(1);
        // Prefer the first non-empty filename we saw.
        if entry.filename == "<unknown>" {
            entry.filename = filename;
        }
        let _ = raw_fn.count; // call count not used; field present for forward-compat.
    }

    Ok(CoverageIndex { by_path })
}

/// Demangle a v0-mangled symbol (`_RNv...`) and strip trailing closure
/// markers / generic args so the result matches the route-walker's
/// `crate::module::function` form.
///
/// Returns `None` when the name isn't demangleable into a Rust path
/// (e.g. C symbols, leftover legacy mangle that doesn't roundtrip).
pub fn demangle_to_path(mangled: &str) -> Option<String> {
    let demangled = format!("{:#}", rustc_demangle::demangle(mangled));
    // `:#` formatter strips the trailing `::h<hash>` suffix on legacy
    // mangling. v0 mangling never had that suffix, but it can have
    // `::{closure#0}` / `::{shim:vtable#0}` markers we want stripped so
    // the path matches the route walker's plain form.
    let stripped = strip_trailing_markers(&demangled);
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

/// Strip rustc-internal trailing path components like `::{closure#0}`,
/// `::{shim:vtable#0}`, `::{constant#0}`, `::{impl#0}`. These are the
/// internal cells of compiler-generated items, not user-facing paths.
fn strip_trailing_markers(path: &str) -> &str {
    let mut tail = path;
    loop {
        let Some(stripped) = tail.strip_suffix('}') else {
            return tail;
        };
        let Some(open_at) = stripped.rfind("::{") else {
            return tail;
        };
        // Confirm the segment between open_at+3 and the trailing `}` is
        // an internal marker (contains `#` digit, e.g. `closure#0`).
        let inner = &stripped[open_at + 3..];
        if inner.contains('#') {
            tail = &tail[..open_at];
        } else {
            return tail;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_coverage_pct_zero_when_no_branches() {
        let fc = FunctionCoverage {
            rust_path: "x".to_string(),
            filename: "x.rs".to_string(),
            branches_total: 0,
            branches_covered: 0,
            function_count: 1,
        };
        assert!((fc.branch_coverage_pct() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn branch_coverage_pct_full_when_all_sides_hit() {
        let fc = FunctionCoverage {
            rust_path: "x".to_string(),
            filename: "x.rs".to_string(),
            branches_total: 8,
            branches_covered: 8,
            function_count: 1,
        };
        assert!((fc.branch_coverage_pct() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn branch_coverage_pct_partial() {
        // 4 branches × 2 sides = 8 total; 6 sides hit → 75%
        let fc = FunctionCoverage {
            rust_path: "x".to_string(),
            filename: "x.rs".to_string(),
            branches_total: 8,
            branches_covered: 6,
            function_count: 1,
        };
        assert!((fc.branch_coverage_pct() - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn strip_closure_marker() {
        assert_eq!(strip_trailing_markers("a::b::c::{closure#0}"), "a::b::c");
    }

    #[test]
    fn strip_nested_markers() {
        assert_eq!(
            strip_trailing_markers("a::b::{closure#0}::{closure#1}"),
            "a::b"
        );
    }

    #[test]
    fn strip_leaves_non_marker_alone() {
        assert_eq!(strip_trailing_markers("a::b::c"), "a::b::c");
    }

    #[test]
    fn strip_does_not_eat_user_braces() {
        // A user-defined item in a crate called `weird` shouldn't be stripped —
        // markers always have `#` inside.
        assert_eq!(
            strip_trailing_markers("a::b::{nomarker}"),
            "a::b::{nomarker}"
        );
    }

    #[test]
    fn parse_rejects_wrong_payload_type() {
        let raw = r#"{"type":"not.coverage","version":"2.0.1","data":[]}"#;
        let err = parse_str(raw).unwrap_err();
        assert!(err.to_string().contains("unexpected coverage payload type"));
    }

    #[test]
    fn parse_rejects_wrong_version() {
        let raw = r#"{"type":"llvm.coverage.json.export","version":"4.0.0","data":[]}"#;
        let err = parse_str(raw).unwrap_err();
        assert!(
            err.to_string()
                .contains("unexpected coverage payload version")
        );
    }

    #[test]
    fn parse_accepts_3x_version() {
        // cargo-llvm-cov on nightly emits `3.1.0`; both 2.x and 3.x must parse.
        let raw = r#"{
            "type":"llvm.coverage.json.export",
            "version":"3.1.0",
            "data":[{"functions":[{"name":"_RNvCsXYZ_3foo3bar","filenames":["/x.rs"],"branches":[],"count":0}]}]
        }"#;
        let idx = parse_str(raw).unwrap();
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn parse_rejects_empty_data() {
        let raw = r#"{"type":"llvm.coverage.json.export","version":"2.0.1","data":[]}"#;
        let err = parse_str(raw).unwrap_err();
        assert!(err.to_string().contains("no `data[]` entries"));
    }

    #[test]
    fn parse_minimal_valid_payload() {
        // Constructed minimal payload mirroring the LLVM JSON shape:
        // one function with one branch (true=1, false=0 → 1-of-2 sides).
        let raw = r#"{
            "type":"llvm.coverage.json.export",
            "version":"2.0.1",
            "data":[{
                "functions":[{
                    "name":"_RNvCsXYZ_3foo3bar",
                    "filenames":["/tmp/foo.rs"],
                    "branches":[[10,5,10,15,1,0,0,0,4]],
                    "count":3
                }]
            }]
        }"#;
        let idx = parse_str(raw).unwrap();
        assert_eq!(idx.len(), 1);
        let fc = idx.get("foo::bar").expect("demangled foo::bar");
        assert_eq!(fc.branches_total, 2);
        assert_eq!(fc.branches_covered, 1);
        assert_eq!(fc.function_count, 1);
        assert!((fc.branch_coverage_pct() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_merges_monomorphisations_by_demangled_path() {
        // Two raw entries demangling to the same path (different generic
        // instantiations) should merge their branch stats.
        let raw = r#"{
            "type":"llvm.coverage.json.export",
            "version":"2.0.1",
            "data":[{
                "functions":[
                    {"name":"_RNvCsXYZ_3foo3bar","filenames":["/tmp/foo.rs"],"branches":[[10,5,10,15,1,1,0,0,4]],"count":1},
                    {"name":"_RNvCsXYZ_3foo3bar","filenames":["/tmp/foo.rs"],"branches":[[20,5,20,15,1,0,0,0,4]],"count":1}
                ]
            }]
        }"#;
        let idx = parse_str(raw).unwrap();
        let fc = idx.get("foo::bar").unwrap();
        assert_eq!(fc.branches_total, 4); // 2 branches × 2 sides
        assert_eq!(fc.branches_covered, 3); // (1+1) + (1+0)
        assert_eq!(fc.function_count, 2);
    }

    #[test]
    fn parse_against_real_fixture() {
        // The cov-branch.json fixture was generated by running
        // `cargo llvm-cov nextest --lib --branch -p scorecard` on nightly.
        // If the fixture has drifted, regenerate it from the workspace
        // root with the same command.
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/cov-branch-scorecard.json");
        if !fixture_path.exists() {
            // Fixture is intentionally optional locally — CI generates
            // the real artifact via the moon task. Skip when absent so
            // a fresh checkout still passes `cargo test`.
            return;
        }
        let idx = parse(&fixture_path).expect("parse fixture");
        assert!(
            !idx.is_empty(),
            "fixture should yield at least one function"
        );
        // Smoke-check that the `scorecard::` namespace appears.
        let any_scorecard = idx
            .iter_sorted()
            .any(|fc| fc.rust_path.starts_with("scorecard::"));
        assert!(
            any_scorecard,
            "expected at least one scorecard:: function in the fixture"
        );
    }
}
