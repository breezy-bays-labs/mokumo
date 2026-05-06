//! Producer pipeline: pub-walker × lcov index → [`PubApiAuditArtifact`].
//!
//! Pure function over its inputs (modulo `now_override` for determinism).
//! The CLI shim ([`super::cli`]) handles argv parsing and file I/O.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use super::artifact::{
    ARTIFACT_VERSION, CratePubItems, Diagnostics, LcovError as ArtifactLcovError, ParseError,
    PubApiAuditArtifact, PubItemEntry,
};
use super::lcov_loader::{self, LcovIndex};
use super::pub_walker::{self, PubItem};
use crate::coverage::crap_exclusions::ExcludedCrates;

pub struct ProducerInput {
    pub workspace_root: PathBuf,
    pub lcov_paths: Vec<PathBuf>,
    pub now_override: Option<String>,
}

#[derive(Debug)]
pub struct ProducerOutput {
    pub artifact: PubApiAuditArtifact,
    /// `0` clean. `2` diagnostics non-empty (parse errors, lcov errors).
    /// Note: uncovered items are NOT a producer-level fail — the gate
    /// decides whether to escalate based on baseline + allowlist.
    pub exit_code: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum ProducerError {
    #[error("workspace discovery failed: {0}")]
    Discovery(String),
    #[error("walker failed: {0}")]
    Walker(String),
}

pub fn run(input: &ProducerInput) -> Result<ProducerOutput> {
    let crates = discover_crates(&input.workspace_root)
        .map_err(|e| ProducerError::Discovery(e.to_string()))?;
    let excluded = ExcludedCrates::read(&input.workspace_root)
        .with_context(|| "reading crap4rs.toml exclusions")?;
    let scan_targets: Vec<(String, PathBuf)> = crates
        .iter()
        .filter(|(pkg, _)| !excluded.contains_package(pkg))
        .cloned()
        .collect();
    let walk = pub_walker::walk(&scan_targets).map_err(|e| ProducerError::Walker(e.to_string()))?;
    let lcov = lcov_loader::load_files(&input.lcov_paths);

    let by_crate = build_by_crate(walk.items, &lcov.index, &input.workspace_root);

    let parse_errors: Vec<ParseError> = walk
        .parse_errors
        .into_iter()
        .map(|p| ParseError {
            file: p
                .source_file
                .strip_prefix(&input.workspace_root)
                .unwrap_or(&p.source_file)
                .to_string_lossy()
                .into_owned(),
            reason: p.reason,
        })
        .collect();
    let lcov_errors: Vec<ArtifactLcovError> = lcov
        .errors
        .into_iter()
        .map(|e| ArtifactLcovError {
            file: e.file.to_string_lossy().into_owned(),
            reason: e.reason,
        })
        .collect();
    let items_walked: u64 = by_crate.iter().map(|c| c.items.len() as u64).sum();

    let diagnostics = Diagnostics {
        excluded_crates: excluded.sorted_packages(),
        parse_errors,
        lcov_errors,
        items_walked,
        lcov_files_consumed: lcov.files_consumed,
    };

    let exit_code = if diagnostics.parse_errors.is_empty() && diagnostics.lcov_errors.is_empty() {
        0
    } else {
        2
    };

    let artifact = PubApiAuditArtifact {
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

fn build_by_crate(
    items: Vec<PubItem>,
    lcov: &LcovIndex,
    workspace_root: &Path,
) -> Vec<CratePubItems> {
    let mut grouped: BTreeMap<String, Vec<PubItemEntry>> = BTreeMap::new();
    for item in items {
        let entry = to_entry(&item, lcov, workspace_root);
        grouped.entry(item.crate_name).or_default().push(entry);
    }
    grouped
        .into_iter()
        .map(|(crate_name, mut items)| {
            items.sort_by(|a, b| a.item_path.cmp(&b.item_path));
            CratePubItems { crate_name, items }
        })
        .collect()
}

fn to_entry(item: &PubItem, lcov: &LcovIndex, workspace_root: &Path) -> PubItemEntry {
    let rel = item
        .source_file
        .strip_prefix(workspace_root)
        .unwrap_or(&item.source_file)
        .to_path_buf();
    let (covered, total) = span_coverage_any_key(
        lcov,
        &rel,
        &item.source_file,
        item.source_line_start,
        item.source_line_end,
    );
    PubItemEntry {
        item_path: item.item_path.clone(),
        kind: item.kind.as_str().to_string(),
        source_file: rel.to_string_lossy().into_owned(),
        source_line_start: item.source_line_start,
        source_line_end: item.source_line_end,
        bdd_covered_lines: covered,
        bdd_total_lines: total,
    }
}

/// `cargo llvm-cov` writes lcov with absolute paths; the walker emits
/// either repo-relative or absolute. Try both, prefer the one that hits.
fn span_coverage_any_key(
    lcov: &LcovIndex,
    rel: &Path,
    abs: &Path,
    start: u32,
    end: u32,
) -> (u32, u32) {
    let total = end.saturating_sub(start).saturating_add(1);
    let (cov_rel, _) = lcov.span_coverage(rel, start, end);
    if cov_rel > 0 {
        return (cov_rel, total);
    }
    let (cov_abs, _) = lcov.span_coverage(abs, start, end);
    if cov_abs > 0 {
        return (cov_abs, total);
    }
    // Neither matched. Return zero hits but keep correct total so the
    // gate's binary "is_bdd_covered()" reads false.
    (0, total)
}

// ---------------------------------------------------------------------------
// Crate discovery — kept independent of the coverage::producer copy so
// tightening one doesn't inadvertently change the other.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CargoTomlPackage {
    package: PackageEntry,
}

#[derive(Debug, Deserialize)]
struct PackageEntry {
    name: String,
}

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

fn read_package_name(crate_dir: &Path) -> Result<Option<String>> {
    let cargo_toml = crate_dir.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&cargo_toml)
        .with_context(|| format!("reading {}", cargo_toml.display()))?;
    match toml::from_str::<CargoTomlPackage>(&raw) {
        Ok(parsed) => Ok(Some(parsed.package.name)),
        Err(_) => Ok(None),
    }
}

fn now_iso8601() -> String {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (y, m, d, hh, mm, ss) = epoch_to_ymdhms(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "epoch arithmetic is bounded — see the matching helper in coverage::producer"
)]
fn epoch_to_ymdhms(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let hh = (rem / 3_600) as u32;
    let mm = ((rem % 3_600) / 60) as u32;
    let ss = (rem % 60) as u32;
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

    fn write_mock_workspace(root: &Path, crate_name: &str, body: &str) {
        let crate_dir = root.join("crates").join(crate_name);
        fs::create_dir_all(crate_dir.join("src")).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{crate_name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n"
            ),
        )
        .unwrap();
        fs::write(crate_dir.join("src/lib.rs"), body).unwrap();
        fs::write(
            root.join("crap4rs.toml"),
            "preset = \"strict\"\nexclude = []\n",
        )
        .unwrap();
    }

    #[test]
    fn run_walks_workspace_and_credits_lcov_hits() {
        let tmp = tempdir().unwrap();
        write_mock_workspace(
            tmp.path(),
            "demo",
            "pub fn covered() {\n    let x = 1;\n}\npub fn uncovered() {\n    let y = 2;\n}\n",
        );
        // Lcov says the first function (lines 1-3) is hit; second (4-6) is not.
        let lcov_path = tmp.path().join("bdd.lcov");
        let abs = tmp.path().join("crates/demo/src/lib.rs");
        fs::write(
            &lcov_path,
            format!(
                "SF:{}\nDA:1,1\nDA:2,1\nDA:3,1\nDA:4,0\nDA:5,0\nDA:6,0\nend_of_record\n",
                abs.display()
            ),
        )
        .unwrap();
        let input = ProducerInput {
            workspace_root: tmp.path().to_path_buf(),
            lcov_paths: vec![lcov_path],
            now_override: Some("2026-05-06T00:00:00Z".into()),
        };
        let output = run(&input).unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.artifact.version, ARTIFACT_VERSION);
        let items = &output.artifact.by_crate[0].items;
        let covered = items
            .iter()
            .find(|i| i.item_path == "demo::covered")
            .unwrap();
        let uncovered = items
            .iter()
            .find(|i| i.item_path == "demo::uncovered")
            .unwrap();
        assert!(covered.bdd_covered_lines > 0);
        assert_eq!(uncovered.bdd_covered_lines, 0);
    }

    #[test]
    fn run_credits_relative_path_lcov_records_too() {
        let tmp = tempdir().unwrap();
        write_mock_workspace(
            tmp.path(),
            "demo",
            "pub fn covered() {\n    let x = 1;\n}\n",
        );
        let lcov_path = tmp.path().join("bdd.lcov");
        // Producer also tries the workspace-relative path key.
        fs::write(
            &lcov_path,
            "SF:crates/demo/src/lib.rs\nDA:1,1\nDA:2,1\nDA:3,1\nend_of_record\n",
        )
        .unwrap();
        let input = ProducerInput {
            workspace_root: tmp.path().to_path_buf(),
            lcov_paths: vec![lcov_path],
            now_override: None,
        };
        let output = run(&input).unwrap();
        let item = &output.artifact.by_crate[0].items[0];
        assert!(item.bdd_covered_lines > 0);
    }

    #[test]
    fn run_returns_zero_diagnostics_when_lcov_empty() {
        let tmp = tempdir().unwrap();
        write_mock_workspace(tmp.path(), "demo", "pub fn x() {}\n");
        let input = ProducerInput {
            workspace_root: tmp.path().to_path_buf(),
            lcov_paths: Vec::new(),
            now_override: None,
        };
        let output = run(&input).unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.artifact.diagnostics.parse_errors.len(), 0);
        assert_eq!(output.artifact.diagnostics.lcov_errors.len(), 0);
        // Items are still walked, just all uncovered.
        assert_eq!(output.artifact.by_crate[0].items.len(), 1);
        assert_eq!(output.artifact.by_crate[0].items[0].bdd_covered_lines, 0);
    }

    #[test]
    fn run_records_lcov_error_on_missing_file() {
        let tmp = tempdir().unwrap();
        write_mock_workspace(tmp.path(), "demo", "pub fn x() {}\n");
        let input = ProducerInput {
            workspace_root: tmp.path().to_path_buf(),
            lcov_paths: vec![tmp.path().join("nonexistent.lcov")],
            now_override: None,
        };
        let output = run(&input).unwrap();
        assert_eq!(output.exit_code, 2);
        assert_eq!(output.artifact.diagnostics.lcov_errors.len(), 1);
    }

    #[test]
    fn run_errors_when_workspace_has_no_crates() {
        let tmp = tempdir().unwrap();
        fs::write(
            tmp.path().join("crap4rs.toml"),
            "preset = \"strict\"\nexclude = []\n",
        )
        .unwrap();
        let input = ProducerInput {
            workspace_root: tmp.path().to_path_buf(),
            lcov_paths: Vec::new(),
            now_override: None,
        };
        let err = run(&input).unwrap_err();
        assert!(err.to_string().contains("no crates discovered"));
    }

    #[test]
    fn run_groups_items_by_crate_and_sorts_alphabetically() {
        let tmp = tempdir().unwrap();
        write_mock_workspace(tmp.path(), "alpha", "pub fn a() {}\npub struct Beta;\n");
        let beta = tmp.path().join("crates/zeta");
        fs::create_dir_all(beta.join("src")).unwrap();
        fs::write(
            beta.join("Cargo.toml"),
            "[package]\nname = \"zeta\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(beta.join("src/lib.rs"), "pub fn z() {}\n").unwrap();
        let input = ProducerInput {
            workspace_root: tmp.path().to_path_buf(),
            lcov_paths: Vec::new(),
            now_override: None,
        };
        let output = run(&input).unwrap();
        assert_eq!(output.artifact.by_crate.len(), 2);
        assert_eq!(output.artifact.by_crate[0].crate_name, "alpha");
        assert_eq!(output.artifact.by_crate[1].crate_name, "zeta");
        // Within crate, items sorted alphabetically by item_path.
        let names: Vec<&str> = output.artifact.by_crate[0]
            .items
            .iter()
            .map(|i| i.item_path.as_str())
            .collect();
        assert_eq!(names, vec!["alpha::Beta", "alpha::a"]);
    }

    #[test]
    fn now_iso8601_returns_iso_shape() {
        let s = now_iso8601();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
    }

    #[test]
    fn epoch_to_ymdhms_handles_known_seconds() {
        let (y, m, d, hh, mm, ss) = epoch_to_ymdhms(1_700_000_000);
        assert_eq!((y, m, d, hh, mm, ss), (2023, 11, 14, 22, 13, 20));
    }
}
