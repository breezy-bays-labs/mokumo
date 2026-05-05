//! Producer entry point — joins route walker output with the LLVM coverage
//! payload and emits the producer artifact JSON.
//!
//! High-level flow:
//! 1. Discover crates: walk `crates/*/Cargo.toml` and `apps/*/Cargo.toml`
//!    for `[package].name`. Pair each with its source directory.
//! 2. Read `crap4rs.toml` — exclude the same crates the CRAP gate excludes.
//! 3. Walk routes via [`route_walker::walk`] over the remaining crates.
//! 4. Parse coverage via [`llvm_cov::parse`].
//! 5. Join: for each route, look up its handler in the coverage index;
//!    emit a [`HandlerArtifactEntry`] when found, append to
//!    `unresolved_handlers` when not.
//! 6. Emit [`CoverageBreakoutArtifact`] JSON to stdout (or the path
//!    supplied by the caller). Exit non-zero when diagnostics are
//!    non-empty so CI surfaces drift loudly.

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::coverage::artifact::{
    ARTIFACT_VERSION, CoverageBreakoutArtifact, CrateHandlerSet, Diagnostics, HandlerArtifactEntry,
    UnresolvableRoute, UnresolvedHandler,
};
use crate::coverage::crap_exclusions::ExcludedCrates;
use crate::coverage::llvm_cov;
use crate::coverage::route_walker::{self, RouteEntry};

/// Inputs for the producer run. Constructed by `coverage-breakouts` from
/// CLI args / env; exposed here so library consumers (and tests) can
/// drive the full pipeline without spawning a subprocess.
pub struct ProducerInput {
    /// Workspace root — the directory containing `Cargo.toml` and
    /// `crap4rs.toml`.
    pub workspace_root: PathBuf,
    /// Path to the coverage JSON emitted by `cargo llvm-cov --branch`.
    pub coverage_json: PathBuf,
    /// Optional override for the current timestamp (test determinism).
    /// `None` means "now". Format: ISO-8601 UTC string.
    pub now_override: Option<String>,
}

/// Producer output: the artifact + an exit-code suggestion. The shim's
/// `main()` returns the suggestion so callers see fail-loud behavior
/// even when the JSON is captured to a file.
#[derive(Debug)]
pub struct ProducerOutput {
    pub artifact: CoverageBreakoutArtifact,
    pub exit_code: i32,
}

/// Errors a producer run can surface. Distinct from build errors
/// (failed to read coverage.json, failed to parse a Cargo.toml) so the
/// shim can return a structured exit code.
#[derive(Debug, thiserror::Error)]
pub enum ProducerError {
    #[error("workspace discovery failed: {0}")]
    Discovery(String),
    #[error("route walker failed: {0}")]
    Walker(String),
    #[error("coverage parse failed: {0}")]
    Coverage(String),
}

/// Run the producer pipeline. Pure function over [`ProducerInput`] —
/// no file writes. Caller decides where to put the artifact.
pub fn run(input: &ProducerInput) -> Result<ProducerOutput> {
    let (walk_outcome, excluded) = gather_walker_inputs(input)?;
    let coverage = llvm_cov::parse(&input.coverage_json)
        .map_err(|e| ProducerError::Coverage(e.to_string()))?;
    let (by_crate, unresolved_handlers) = join_routes_with_coverage(walk_outcome.routes, &coverage);
    let diagnostics = Diagnostics {
        unresolved_handlers,
        unresolvable_routes: lift_unresolvable(walk_outcome.unresolvable),
        excluded_crates: excluded.sorted_packages(),
    };
    // 0/2 contract — see `ProducerOutput.exit_code` doc. Library callers
    // that translate this into a process exit code get the right value
    // directly, without coverage_breakouts having to re-coerce.
    let exit_code = if diagnostics.unresolved_handlers.is_empty()
        && diagnostics.unresolvable_routes.is_empty()
    {
        0
    } else {
        2
    };
    let artifact = CoverageBreakoutArtifact {
        version: ARTIFACT_VERSION,
        generated_at: input.now_override.clone().unwrap_or_else(now_iso8601),
        by_crate,
        diagnostics,
    };
    Ok(ProducerOutput {
        artifact,
        exit_code,
    })
}

/// Discover crates, read CRAP exclusions, and run the route walker over
/// the scan targets — the front half of the pipeline up to (but not
/// including) coverage parsing.
fn gather_walker_inputs(
    input: &ProducerInput,
) -> Result<(route_walker::WalkOutcome, ExcludedCrates)> {
    let crates = discover_crates(&input.workspace_root)
        .map_err(|e| ProducerError::Discovery(e.to_string()))?;
    let excluded = ExcludedCrates::read(&input.workspace_root)
        .with_context(|| "reading crap4rs.toml exclusions")?;
    // Pass the Cargo package name straight through (e.g. "mokumo-shop");
    // the walker calls `to_ident` internally for path resolution against
    // mangled symbols, but the artifact's per-crate label keeps the
    // package name so hyphenated crates render as themselves in the
    // drill-down rather than as their underscore-aliased ident form.
    let scan_targets: Vec<(String, PathBuf)> = crates
        .iter()
        .filter(|(pkg, _)| !excluded.contains_package(pkg))
        .map(|(pkg, dir)| (pkg.clone(), dir.clone()))
        .collect();
    let walk_outcome =
        route_walker::walk(&scan_targets).map_err(|e| ProducerError::Walker(e.to_string()))?;
    Ok((walk_outcome, excluded))
}

/// Join walker output with coverage: emit a [`HandlerArtifactEntry`] when
/// the coverage index has the handler, otherwise append to
/// `unresolved_handlers`. Returns `(by_crate, unresolved)` — empty
/// handler sets are dropped from `by_crate` (no signal for the renderer).
fn join_routes_with_coverage(
    routes: Vec<RouteEntry>,
    coverage: &llvm_cov::CoverageIndex,
) -> (Vec<CrateHandlerSet>, Vec<UnresolvedHandler>) {
    let mut grouped: BTreeMap<String, Vec<RouteEntry>> = BTreeMap::new();
    for r in routes {
        grouped.entry(r.crate_name.clone()).or_default().push(r);
    }
    let mut by_crate: Vec<CrateHandlerSet> = Vec::new();
    let mut unresolved: Vec<UnresolvedHandler> = Vec::new();
    for (crate_name, routes) in grouped {
        let handlers = join_one_crate(routes, coverage, &mut unresolved);
        if !handlers.is_empty() {
            by_crate.push(CrateHandlerSet {
                crate_name,
                handlers,
            });
        }
    }
    (by_crate, unresolved)
}

/// Per-crate join — one handler entry per route present in coverage, one
/// `UnresolvedHandler` push per route absent from it.
fn join_one_crate(
    routes: Vec<RouteEntry>,
    coverage: &llvm_cov::CoverageIndex,
    unresolved: &mut Vec<UnresolvedHandler>,
) -> Vec<HandlerArtifactEntry> {
    let mut handlers: Vec<HandlerArtifactEntry> = Vec::new();
    for r in routes {
        let route_label = format!("{} {}", r.method, r.path);
        if let Some(fc) = coverage.get(&r.rust_path) {
            handlers.push(HandlerArtifactEntry {
                route: route_label,
                rust_path: r.rust_path,
                filename: fc.filename.clone(),
                branches_total: fc.branches_total,
                branches_covered: fc.branches_covered,
                branch_coverage_pct: fc.branch_coverage_pct(),
                function_count: fc.function_count,
            });
        } else {
            unresolved.push(UnresolvedHandler {
                route: route_label,
                rust_path: r.rust_path,
                source_file: r.source_file.to_string_lossy().into_owned(),
                source_line: r.source_line,
            });
        }
    }
    handlers
}

/// Translate walker-side unresolvable findings into the artifact's
/// public-facing wire shape.
fn lift_unresolvable(
    findings: Vec<route_walker::UnresolvableRouteFinding>,
) -> Vec<UnresolvableRoute> {
    findings
        .into_iter()
        .map(|u| UnresolvableRoute {
            route_literal: u.route_literal,
            source_file: u.source_file.to_string_lossy().into_owned(),
            source_line: u.source_line,
            reason: u.reason,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Crate discovery
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CargoTomlPackage {
    package: PackageEntry,
}

#[derive(Debug, Deserialize)]
struct PackageEntry {
    name: String,
}

/// Walk `crates/*/Cargo.toml` and `apps/*/Cargo.toml`, returning
/// `(package_name, crate_dir)` pairs. Workspace `Cargo.toml`s without a
/// `[package]` section are skipped.
fn discover_crates(workspace_root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    for sub in ["crates", "apps"] {
        let dir = workspace_root.join(sub);
        if !dir.is_dir() {
            continue;
        }
        scan_subdir_for_packages(&dir, &mut out)?;
    }
    if out.is_empty() {
        return Err(anyhow!(
            "no crates discovered under {} — workspace layout differs from expectations",
            workspace_root.display()
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Read one parent directory (`crates/` or `apps/`), append every child
/// that carries a `Cargo.toml` with a `[package]` table.
fn scan_subdir_for_packages(dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let crate_dir = entry.path();
        if !crate_dir.is_dir() {
            continue;
        }
        if let Some(name) = read_package_name(&crate_dir)? {
            out.push((name, crate_dir));
        }
    }
    Ok(())
}

/// Return `Some(package_name)` when `crate_dir/Cargo.toml` exists and
/// parses with a `[package]` table; `None` for missing manifests, manifests
/// without `[package]` (e.g. workspace roots), and otherwise-skippable
/// shapes.
fn read_package_name(crate_dir: &Path) -> Result<Option<String>> {
    let cargo_toml = crate_dir.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&cargo_toml)
        .with_context(|| format!("reading {}", cargo_toml.display()))?;
    let Ok(parsed) = toml::from_str::<CargoTomlPackage>(&raw) else {
        return Ok(None);
    };
    Ok(Some(parsed.package.name))
}

fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // Minimal ISO-8601 UTC formatter. We avoid pulling chrono / time as a
    // direct dep here — the producer artifact is consumed by the
    // aggregator (which has chrono), so a short hand-rolled format with
    // second precision suffices.
    let (y, m, d, hh, mm, ss) = epoch_to_ymdhms(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "epoch arithmetic is bounded — `secs` is always non-negative (came from \
              `Duration`), and intermediate signed values stay positive past year 9999. \
              Date components fit in u32 by definition of the algorithm."
)]
fn epoch_to_ymdhms(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    // Days since 1970-01-01.
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let hh = (rem / 3_600) as u32;
    let mm = ((rem % 3_600) / 60) as u32;
    let ss = (rem % 60) as u32;
    // Civil-from-days (Howard Hinnant). Avoids chrono dep.
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = y as u32 + u32::from(m <= 2);
    (y, m, d, hh, mm, ss)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn epoch_zero_is_1970_01_01() {
        assert_eq!(epoch_to_ymdhms(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn epoch_known_2026_05_04() {
        // 2026-05-04T00:00:00Z
        let s = 1_777_852_800u64;
        assert_eq!(epoch_to_ymdhms(s), (2026, 5, 4, 0, 0, 0));
    }

    #[test]
    fn epoch_handles_hour_minute_second() {
        // 2026-05-04T01:02:03Z = midnight + 1h 2m 3s = 3723 seconds.
        let s = 1_777_852_800u64 + 3_723;
        assert_eq!(epoch_to_ymdhms(s), (2026, 5, 4, 1, 2, 3));
    }

    #[test]
    fn discover_crates_finds_package_manifests() {
        let tmp = tempdir().unwrap();
        let crates_dir = tmp.path().join("crates");
        fs::create_dir_all(crates_dir.join("alpha")).unwrap();
        fs::create_dir_all(crates_dir.join("beta")).unwrap();
        fs::write(
            crates_dir.join("alpha/Cargo.toml"),
            "[package]\nname = \"alpha\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        fs::write(
            crates_dir.join("beta/Cargo.toml"),
            "[package]\nname = \"beta\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        let crates = discover_crates(tmp.path()).unwrap();
        assert_eq!(
            crates.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
    }

    #[test]
    fn discover_skips_non_package_manifests() {
        let tmp = tempdir().unwrap();
        let crates_dir = tmp.path().join("crates");
        fs::create_dir_all(crates_dir.join("alpha")).unwrap();
        fs::write(
            crates_dir.join("alpha/Cargo.toml"),
            "[workspace]\nmembers = []\n",
        )
        .unwrap();
        // Add a real package so discovery doesn't error on empty.
        fs::create_dir_all(crates_dir.join("real")).unwrap();
        fs::write(
            crates_dir.join("real/Cargo.toml"),
            "[package]\nname = \"real\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        let crates = discover_crates(tmp.path()).unwrap();
        assert_eq!(crates.len(), 1);
        assert_eq!(crates[0].0, "real");
    }

    #[test]
    fn discover_errors_on_empty_workspace() {
        let tmp = tempdir().unwrap();
        let err = discover_crates(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("no crates discovered"));
    }

    /// Build a tempdir workspace with one crate carrying a single Axum
    /// route, write an LLVM-shaped coverage.json that includes the
    /// handler's mangled symbol, and assert `run()` joins them into a
    /// non-empty artifact with no diagnostics. Exercises the happy path
    /// end-to-end: discover → exclusions → walker → coverage parse → join.
    #[test]
    fn run_joins_routes_with_coverage() {
        let tmp = tempdir().unwrap();
        let crates_dir = tmp.path().join("crates/demo");
        let src_dir = crates_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            crates_dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            src_dir.join("lib.rs"),
            r#"
use axum::{Router, routing::get};
pub fn router() -> Router {
    Router::new().route("/api/health", get(health))
}
fn health() {}
            "#,
        )
        .unwrap();

        // Coverage payload: one function whose v0 mangled name demangles
        // to `demo::health` (matches the route walker's resolved rust_path).
        let coverage_json = tmp.path().join("coverage.json");
        fs::write(
            &coverage_json,
            r#"{
                "type":"llvm.coverage.json.export",
                "version":"3.1.0",
                "data":[{"functions":[{
                    "name":"_RNvCsXYZ_4demo6health",
                    "filenames":["/tmp/demo/src/lib.rs"],
                    "branches":[[10,5,10,15,1,1,0,0,4]],
                    "count":1
                }]}]
            }"#,
        )
        .unwrap();

        let input = ProducerInput {
            workspace_root: tmp.path().to_path_buf(),
            coverage_json,
            now_override: Some("2026-05-04T00:00:00Z".to_string()),
        };
        let out = run(&input).expect("run");
        assert_eq!(
            out.exit_code, 0,
            "no diagnostics expected: {:?}",
            out.artifact.diagnostics
        );
        assert_eq!(out.artifact.generated_at, "2026-05-04T00:00:00Z");
        assert_eq!(out.artifact.by_crate.len(), 1);
        let crate_set = &out.artifact.by_crate[0];
        assert_eq!(crate_set.crate_name, "demo");
        assert_eq!(crate_set.handlers.len(), 1);
        let h = &crate_set.handlers[0];
        assert_eq!(h.route, "GET /api/health");
        assert_eq!(h.rust_path, "demo::health");
        assert_eq!(h.branches_total, 2);
        assert_eq!(h.branches_covered, 2);
        assert!((h.branch_coverage_pct - 100.0).abs() < f64::EPSILON);
    }

    /// Same setup as above but coverage.json names a function the walker
    /// won't see — diagnostics should record an unresolved handler and
    /// exit_code becomes non-zero (the producer's loud-failure contract).
    #[test]
    fn run_records_unresolved_handler_when_coverage_misses_route() {
        let tmp = tempdir().unwrap();
        let crates_dir = tmp.path().join("crates/demo");
        let src_dir = crates_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            crates_dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            src_dir.join("lib.rs"),
            r#"
use axum::{Router, routing::get};
pub fn router() -> Router {
    Router::new().route("/api/health", get(health))
}
fn health() {}
            "#,
        )
        .unwrap();
        // Coverage names a different symbol — handler `demo::health` is
        // missing from the index, so the producer flags it.
        let coverage_json = tmp.path().join("coverage.json");
        fs::write(
            &coverage_json,
            r#"{
                "type":"llvm.coverage.json.export",
                "version":"3.1.0",
                "data":[{"functions":[{
                    "name":"_RNvCsXYZ_4demo5other",
                    "filenames":["/tmp/demo/src/lib.rs"],
                    "branches":[],
                    "count":0
                }]}]
            }"#,
        )
        .unwrap();

        let input = ProducerInput {
            workspace_root: tmp.path().to_path_buf(),
            coverage_json,
            now_override: None,
        };
        let out = run(&input).expect("run");
        assert_eq!(out.exit_code, 2, "0/2 contract: 2 on diagnostics non-empty");
        assert_eq!(out.artifact.diagnostics.unresolved_handlers.len(), 1);
        assert_eq!(
            out.artifact.by_crate.len(),
            0,
            "empty crate sets are dropped"
        );
        assert_eq!(
            out.artifact.diagnostics.unresolved_handlers[0].rust_path,
            "demo::health"
        );
    }

    /// Excluded crates skip the walker entirely and surface in
    /// `diagnostics.excluded_crates`.
    #[test]
    fn run_honours_crap4rs_exclusions() {
        let tmp = tempdir().unwrap();
        let crates_dir = tmp.path().join("crates/demo");
        let src_dir = crates_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            crates_dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(src_dir.join("lib.rs"), "// no routes\n").unwrap();
        fs::write(
            tmp.path().join("crap4rs.toml"),
            "[exclusions]\ncrates = [\"demo\"]\n",
        )
        .unwrap();
        // Coverage payload with one entry — irrelevant, walker is skipped.
        let coverage_json = tmp.path().join("coverage.json");
        fs::write(
            &coverage_json,
            r#"{"type":"llvm.coverage.json.export","version":"3.1.0","data":[{"functions":[{"name":"_RNvCsXYZ_3foo3bar","filenames":["/x.rs"],"branches":[],"count":0}]}]}"#,
        )
        .unwrap();

        let input = ProducerInput {
            workspace_root: tmp.path().to_path_buf(),
            coverage_json,
            now_override: Some("2026-01-01T00:00:00Z".to_string()),
        };
        let out = run(&input).expect("run");
        assert_eq!(out.exit_code, 0);
        assert!(out.artifact.by_crate.is_empty());
        assert_eq!(
            out.artifact.diagnostics.excluded_crates,
            vec!["demo".to_string()]
        );
    }

    /// Discovery surfaces `ProducerError::Discovery` when the workspace
    /// has no crates — keeps the error path covered without leaning on
    /// the underlying anyhow string.
    #[test]
    fn run_returns_discovery_error_on_empty_workspace() {
        let tmp = tempdir().unwrap();
        let coverage_json = tmp.path().join("coverage.json");
        // Even a syntactically-valid coverage payload doesn't matter — we
        // bail before reading it.
        fs::write(
            &coverage_json,
            r#"{"type":"llvm.coverage.json.export","version":"3.1.0","data":[{"functions":[]}]}"#,
        )
        .unwrap();
        let input = ProducerInput {
            workspace_root: tmp.path().to_path_buf(),
            coverage_json,
            now_override: None,
        };
        let err = run(&input).unwrap_err();
        assert!(err.to_string().contains("workspace discovery failed"));
    }
}
