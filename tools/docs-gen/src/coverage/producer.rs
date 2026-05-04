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
use crate::coverage::crap_exclusions::{self, ExcludedCrates};
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
    let crates = discover_crates(&input.workspace_root)
        .map_err(|e| ProducerError::Discovery(e.to_string()))?;
    let excluded = ExcludedCrates::read(&input.workspace_root)
        .with_context(|| "reading crap4rs.toml exclusions")?;

    // Filter out excluded crates and crates without handler-bearing code.
    let scan_targets: Vec<(String, PathBuf)> = crates
        .iter()
        .filter(|(pkg, _)| !excluded.contains_package(pkg))
        .map(|(pkg, dir)| (crap_exclusions::to_ident(pkg), dir.clone()))
        .collect();

    let walk_outcome =
        route_walker::walk(&scan_targets).map_err(|e| ProducerError::Walker(e.to_string()))?;

    let coverage = llvm_cov::parse(&input.coverage_json)
        .map_err(|e| ProducerError::Coverage(e.to_string()))?;

    // Group routes by crate.
    let mut grouped: BTreeMap<String, Vec<RouteEntry>> = BTreeMap::new();
    for r in walk_outcome.routes {
        grouped.entry(r.crate_name.clone()).or_default().push(r);
    }

    let mut by_crate: Vec<CrateHandlerSet> = Vec::new();
    let mut unresolved_handlers: Vec<UnresolvedHandler> = Vec::new();

    for (crate_name, routes) in grouped {
        let mut handlers: Vec<HandlerArtifactEntry> = Vec::new();
        for r in routes {
            let route_label = format!("{} {}", r.method, r.path);
            match coverage.get(&r.rust_path) {
                Some(fc) => handlers.push(HandlerArtifactEntry {
                    route: route_label,
                    rust_path: r.rust_path,
                    filename: fc.filename.clone(),
                    branches_total: fc.branches_total,
                    branches_covered: fc.branches_covered,
                    branch_coverage_pct: fc.branch_coverage_pct(),
                    function_count: fc.function_count,
                }),
                None => unresolved_handlers.push(UnresolvedHandler {
                    route: route_label,
                    rust_path: r.rust_path,
                    source_file: r.source_file.to_string_lossy().into_owned(),
                    source_line: r.source_line,
                }),
            }
        }
        // Don't emit empty handler sets — they clutter the artifact and
        // the renderer with no signal.
        if handlers.is_empty() {
            continue;
        }
        by_crate.push(CrateHandlerSet {
            crate_name,
            handlers,
        });
    }

    let unresolvable_routes: Vec<UnresolvableRoute> = walk_outcome
        .unresolvable
        .into_iter()
        .map(|u| UnresolvableRoute {
            route_literal: u.route_literal,
            source_file: u.source_file.to_string_lossy().into_owned(),
            source_line: u.source_line,
            reason: u.reason,
        })
        .collect();

    let diagnostics = Diagnostics {
        unresolved_handlers,
        unresolvable_routes,
        excluded_crates: excluded.sorted_packages(),
    };

    let exit_code = i32::from(
        !diagnostics.unresolved_handlers.is_empty() || !diagnostics.unresolvable_routes.is_empty(),
    );

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
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?
        {
            let entry = entry?;
            let crate_dir = entry.path();
            if !crate_dir.is_dir() {
                continue;
            }
            let cargo_toml = crate_dir.join("Cargo.toml");
            if !cargo_toml.is_file() {
                continue;
            }
            let raw = std::fs::read_to_string(&cargo_toml)
                .with_context(|| format!("reading {}", cargo_toml.display()))?;
            let Ok(parsed) = toml::from_str::<CargoTomlPackage>(&raw) else {
                // Likely a workspace root or a manifest without [package].
                continue;
            };
            out.push((parsed.package.name, crate_dir));
        }
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
}
