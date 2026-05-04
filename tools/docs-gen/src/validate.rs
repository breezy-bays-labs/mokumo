//! Library implementation of the `adr-validate` binary.
//!
//! Exposed here (rather than living entirely under `src/bin/`) so the
//! coverage-driven CRAP gate sees these functions through `cargo nextest`'s
//! library-test profile. The thin shim under `src/bin/adr-validate.rs`
//! re-exports `parse_args`, `run`, and `Args` and forwards to them; the
//! logic, decision branches, and tests all live here.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use crate::adr::{self, EnforcedBy, EnforcedByKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub workspace_root: PathBuf,
    pub adr_root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseOutcome {
    /// Continue to `run`.
    Run,
    /// `--help` short-circuit; the caller should print help and exit 0.
    ShowHelp,
}

/// Parses a flat-flag CLI: `--adr-root <PATH>`, `--workspace-root <PATH>`,
/// `-h | --help`. Takes the args iterator explicitly so tests can drive it
/// without touching the process-global `env::args`.
///
/// Defaults: workspace root is discovered from the current working
/// directory; ADR root is `<workspace>/docs/adr`.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<(Args, ParseOutcome)> {
    let mut workspace_root: Option<PathBuf> = None;
    let mut adr_root: Option<PathBuf> = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--adr-root" => {
                let v = iter.next().context("--adr-root requires a path argument")?;
                adr_root = Some(PathBuf::from(v));
            }
            "--workspace-root" => {
                let v = iter
                    .next()
                    .context("--workspace-root requires a path argument")?;
                workspace_root = Some(PathBuf::from(v));
            }
            "-h" | "--help" => {
                return Ok((empty_args(), ParseOutcome::ShowHelp));
            }
            other => bail!("unknown argument `{other}` (try --help)"),
        }
    }
    let workspace_root = match workspace_root {
        Some(p) => p,
        None => crate::workspace::find_root()?,
    };
    let adr_root = adr_root.unwrap_or_else(|| workspace_root.join("docs/adr"));
    Ok((
        Args {
            workspace_root,
            adr_root,
        },
        ParseOutcome::Run,
    ))
}

fn empty_args() -> Args {
    Args {
        workspace_root: PathBuf::new(),
        adr_root: PathBuf::new(),
    }
}

pub const HELP_TEXT: &str = "adr-validate — resolve ADR `enforced-by:` references\n\
                             \n\
                             Usage: adr-validate [--adr-root <PATH>] [--workspace-root <PATH>]\n\
                             \n\
                             Defaults:\n  \
                             --workspace-root: discovered by walking up from cwd\n  \
                             --adr-root: <workspace-root>/docs/adr";

/// Outcome of a single `run` invocation, returned to the bin shim instead
/// of calling `process::exit` directly so tests can assert on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub checked: usize,
    pub failures: Vec<String>,
}

impl RunReport {
    pub fn ok(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Whole CLI dispatch in one function, returning an exit code so the bin
/// shim can stay branch-free (and out of the CRAP gate's blind spot for
/// uncovered binaries). Reads from / writes to the streams directly so
/// behaviour is observable end-to-end without process forking.
pub fn execute(argv: Vec<String>) -> i32 {
    let (args, outcome) = match parse_args(argv) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("adr-validate: {e:#}");
            return 2;
        }
    };
    if outcome == ParseOutcome::ShowHelp {
        println!("{HELP_TEXT}");
        return 0;
    }
    let report = match run(&args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("adr-validate: {e:#}");
            return 2;
        }
    };
    if report.ok() {
        eprintln!(
            "adr-validate: {} ADR(s) checked, all references resolve",
            report.checked
        );
        return 0;
    }
    eprintln!("adr-validate: unresolved references:");
    for f in &report.failures {
        eprintln!("  - {f}");
    }
    1
}

/// Walks `args.adr_root` for ADRs and resolves every `enforced-by:`
/// reference. Returns a [`RunReport`] — the bin shim translates that to an
/// exit code. Errors here are I/O / parser errors, not unresolved refs.
pub fn run(args: &Args) -> Result<RunReport> {
    let adrs = adr::walk_adrs(&args.adr_root)?;
    let mut failures: Vec<String> = Vec::new();
    for adr in &adrs {
        for ev in &adr.enforced_by {
            if let Err(reason) = resolve(&args.workspace_root, ev) {
                failures.push(format!(
                    "{}: kind={} ref={:?} — {}",
                    adr.path.display(),
                    ev.kind.as_str(),
                    ev.r#ref,
                    reason
                ));
            }
        }
    }
    Ok(RunReport {
        checked: adrs.len(),
        failures,
    })
}

/// Resolution rules (T1):
/// - `workflow` — `ref` must be a path that exists relative to the workspace root.
/// - `lint` / `test` — if `ref` looks like a path, it must exist; otherwise
///   accepted (Rust test idents, clippy lint identifiers).
/// - `dep-absence` — accepted (T2 wires `Cargo.toml`/`Cargo.lock` resolution).
/// - `human-judgment` — always accepted.
pub fn resolve(workspace_root: &Path, ev: &EnforcedBy) -> std::result::Result<(), String> {
    match ev.kind {
        EnforcedByKind::Workflow => resolve_path(workspace_root, &ev.r#ref),
        EnforcedByKind::Lint | EnforcedByKind::Test => {
            if looks_like_path(&ev.r#ref) {
                resolve_path(workspace_root, &ev.r#ref)
            } else {
                Ok(())
            }
        }
        EnforcedByKind::DepAbsence | EnforcedByKind::HumanJudgment => Ok(()),
    }
}

/// True when `reference` reads like a path. Forward slash covers
/// POSIX paths; backslash covers Windows paths; recognized extensions
/// catch path-shaped refs without separators (e.g. a top-level `Cargo.toml`).
pub fn looks_like_path(reference: &str) -> bool {
    reference.contains('/')
        || reference.contains('\\')
        || reference.ends_with(".rs")
        || reference.ends_with(".yml")
        || reference.ends_with(".yaml")
        || reference.ends_with(".sh")
        || reference.ends_with(".toml")
}

fn resolve_path(workspace_root: &Path, reference: &str) -> std::result::Result<(), String> {
    let rel = Path::new(reference);
    // `Path::join` lets an absolute argument replace the base entirely
    // (`/ws".join("/etc/passwd") == "/etc/passwd"`), and `..` segments can
    // walk above the workspace root. Both violate the documented
    // "workspace-relative" contract for `enforced-by:` refs, so we reject
    // them before the existence check rather than silently following them.
    if rel.is_absolute() {
        return Err(format!(
            "path `{reference}` must be workspace-relative (absolute paths rejected)"
        ));
    }
    if rel
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "path `{reference}` must stay under the workspace root (parent-relative `..` rejected)"
        ));
    }
    let target = workspace_root.join(rel);
    if target.exists() {
        Ok(())
    } else {
        Err(format!("path {} does not exist", target.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adr::{EnforcedBy, EnforcedByKind};
    use tempfile::tempdir;

    fn ev(kind: EnforcedByKind, reference: &str) -> EnforcedBy {
        EnforcedBy {
            kind,
            r#ref: reference.to_string(),
            note: String::new(),
        }
    }

    fn args(adr_root: &Path) -> Args {
        Args {
            workspace_root: adr_root
                .parent()
                .and_then(Path::parent)
                .expect("test paths must have grandparent")
                .to_path_buf(),
            adr_root: adr_root.to_path_buf(),
        }
    }

    // ─── parse_args ──────────────────────────────────────────────────

    #[test]
    fn parse_args_defaults_when_no_flags() {
        // Don't run from cwd — pass --workspace-root so the test doesn't
        // depend on where it was invoked from.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers=[]\n").unwrap();
        let argv = vec![
            "--workspace-root".to_string(),
            dir.path().display().to_string(),
        ];
        let (args, outcome) = parse_args(argv).unwrap();
        assert_eq!(outcome, ParseOutcome::Run);
        assert_eq!(args.workspace_root, dir.path());
        assert_eq!(args.adr_root, dir.path().join("docs/adr"));
    }

    #[test]
    fn parse_args_accepts_explicit_adr_root() {
        let argv = vec![
            "--workspace-root".to_string(),
            "/ws".to_string(),
            "--adr-root".to_string(),
            "/elsewhere/adr".to_string(),
        ];
        let (args, outcome) = parse_args(argv).unwrap();
        assert_eq!(outcome, ParseOutcome::Run);
        assert_eq!(args.adr_root, PathBuf::from("/elsewhere/adr"));
    }

    #[test]
    fn parse_args_short_help_returns_show_help() {
        let argv = vec!["-h".to_string()];
        let (_, outcome) = parse_args(argv).unwrap();
        assert_eq!(outcome, ParseOutcome::ShowHelp);
    }

    #[test]
    fn parse_args_long_help_returns_show_help() {
        let argv = vec!["--help".to_string()];
        let (_, outcome) = parse_args(argv).unwrap();
        assert_eq!(outcome, ParseOutcome::ShowHelp);
    }

    #[test]
    fn parse_args_rejects_unknown_flag() {
        let err = parse_args(vec!["--bogus".to_string()]).unwrap_err();
        assert!(err.to_string().contains("unknown argument"));
    }

    #[test]
    fn parse_args_rejects_adr_root_without_value() {
        let err = parse_args(vec!["--adr-root".to_string()]).unwrap_err();
        assert!(err.to_string().contains("--adr-root requires"));
    }

    #[test]
    fn parse_args_rejects_workspace_root_without_value() {
        let err = parse_args(vec!["--workspace-root".to_string()]).unwrap_err();
        assert!(err.to_string().contains("--workspace-root requires"));
    }

    // ─── run ─────────────────────────────────────────────────────────

    #[test]
    fn run_reports_zero_when_adr_root_missing() {
        let dir = tempdir().unwrap();
        let report = run(&Args {
            workspace_root: dir.path().to_path_buf(),
            adr_root: dir.path().join("does-not-exist"),
        })
        .unwrap();
        assert_eq!(report.checked, 0);
        assert!(report.ok());
    }

    #[test]
    fn run_resolves_path_refs_against_workspace_root() {
        let dir = tempdir().unwrap();
        let adr_dir = dir.path().join("docs/adr");
        std::fs::create_dir_all(&adr_dir).unwrap();
        std::fs::create_dir_all(dir.path().join(".github/workflows")).unwrap();
        std::fs::write(
            dir.path().join(".github/workflows/quality.yml"),
            "name: q\n",
        )
        .unwrap();
        std::fs::write(
            adr_dir.join("a.md"),
            "\
---
title: A
status: approved
enforced-by:
  - kind: workflow
    ref: .github/workflows/quality.yml
    note: ok
---
",
        )
        .unwrap();
        let report = run(&args(&adr_dir)).unwrap();
        assert_eq!(report.checked, 1);
        assert!(report.ok(), "failures: {:?}", report.failures);
    }

    #[test]
    fn run_collects_failures_for_unresolvable_path_refs() {
        let dir = tempdir().unwrap();
        let adr_dir = dir.path().join("docs/adr");
        std::fs::create_dir_all(&adr_dir).unwrap();
        std::fs::write(
            adr_dir.join("a.md"),
            "\
---
title: A
status: approved
enforced-by:
  - kind: workflow
    ref: .github/workflows/missing.yml
    note: ok
  - kind: human-judgment
    ref: code review
    note: ok
---
",
        )
        .unwrap();
        let report = run(&args(&adr_dir)).unwrap();
        assert_eq!(report.checked, 1);
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].contains("missing.yml"));
        assert!(report.failures[0].contains("kind=workflow"));
    }

    // ─── resolve / looks_like_path ───────────────────────────────────

    #[test]
    fn resolve_path_rejects_absolute_references() {
        let dir = tempdir().unwrap();
        let err = resolve(dir.path(), &ev(EnforcedByKind::Workflow, "/etc/passwd")).unwrap_err();
        assert!(
            err.contains("workspace-relative") && err.contains("absolute"),
            "got: {err}"
        );
    }

    #[test]
    fn resolve_path_rejects_parent_relative_references() {
        let dir = tempdir().unwrap();
        let err = resolve(dir.path(), &ev(EnforcedByKind::Workflow, "../etc/passwd")).unwrap_err();
        assert!(
            err.contains("under the workspace root") && err.contains(".."),
            "got: {err}"
        );
    }

    #[test]
    fn resolve_path_rejects_embedded_parent_traversal() {
        let dir = tempdir().unwrap();
        let err = resolve(
            dir.path(),
            &ev(EnforcedByKind::Workflow, "docs/../../etc/passwd"),
        )
        .unwrap_err();
        assert!(err.contains("under the workspace root"), "got: {err}");
    }

    #[test]
    fn workflow_ref_must_exist() {
        let dir = tempdir().unwrap();
        let err = resolve(
            dir.path(),
            &ev(EnforcedByKind::Workflow, ".github/workflows/quality.yml"),
        )
        .unwrap_err();
        assert!(err.contains("does not exist"));
        std::fs::create_dir_all(dir.path().join(".github/workflows")).unwrap();
        std::fs::write(
            dir.path().join(".github/workflows/quality.yml"),
            "name: q\n",
        )
        .unwrap();
        resolve(
            dir.path(),
            &ev(EnforcedByKind::Workflow, ".github/workflows/quality.yml"),
        )
        .unwrap();
    }

    #[test]
    fn test_kind_accepts_rust_path_idents() {
        let dir = tempdir().unwrap();
        resolve(
            dir.path(),
            &ev(EnforcedByKind::Test, "crate::module::test_name"),
        )
        .unwrap();
    }

    #[test]
    fn test_kind_resolves_when_ref_looks_like_path() {
        let dir = tempdir().unwrap();
        let err = resolve(
            dir.path(),
            &ev(EnforcedByKind::Test, "crates/foo/tests/bar.rs"),
        )
        .unwrap_err();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn lint_kind_accepts_clippy_identifier() {
        let dir = tempdir().unwrap();
        resolve(
            dir.path(),
            &ev(EnforcedByKind::Lint, "clippy::needless_borrow"),
        )
        .unwrap();
    }

    #[test]
    fn human_judgment_always_resolves() {
        let dir = tempdir().unwrap();
        resolve(
            dir.path(),
            &ev(EnforcedByKind::HumanJudgment, "any free text here"),
        )
        .unwrap();
    }

    #[test]
    fn dep_absence_is_t2_stub_accepts_for_now() {
        let dir = tempdir().unwrap();
        resolve(dir.path(), &ev(EnforcedByKind::DepAbsence, "tauri")).unwrap();
    }

    // ─── execute ─────────────────────────────────────────────────────

    #[test]
    fn execute_returns_zero_for_help() {
        let code = execute(vec!["--help".to_string()]);
        assert_eq!(code, 0);
    }

    #[test]
    fn execute_returns_two_on_parse_error() {
        let code = execute(vec!["--bogus".to_string()]);
        assert_eq!(code, 2);
    }

    #[test]
    fn execute_returns_zero_on_clean_run() {
        let dir = tempdir().unwrap();
        let adr_dir = dir.path().join("docs/adr");
        std::fs::create_dir_all(&adr_dir).unwrap();
        // Empty ADR root → 0 ADR(s) checked → exit 0.
        let code = execute(vec![
            "--workspace-root".to_string(),
            dir.path().display().to_string(),
            "--adr-root".to_string(),
            adr_dir.display().to_string(),
        ]);
        assert_eq!(code, 0);
    }

    #[test]
    fn execute_returns_one_on_unresolved_refs() {
        let dir = tempdir().unwrap();
        let adr_dir = dir.path().join("docs/adr");
        std::fs::create_dir_all(&adr_dir).unwrap();
        std::fs::write(
            adr_dir.join("a.md"),
            "\
---
title: A
status: approved
enforced-by:
  - kind: workflow
    ref: .github/workflows/missing.yml
    note: ok
---
",
        )
        .unwrap();
        let code = execute(vec![
            "--workspace-root".to_string(),
            dir.path().display().to_string(),
            "--adr-root".to_string(),
            adr_dir.display().to_string(),
        ]);
        assert_eq!(code, 1);
    }

    #[test]
    fn execute_returns_two_on_io_or_parse_error_in_run() {
        let dir = tempdir().unwrap();
        let adr_dir = dir.path().join("docs/adr");
        std::fs::create_dir_all(&adr_dir).unwrap();
        // Malformed YAML — unclosed frontmatter.
        std::fs::write(adr_dir.join("a.md"), "---\ntitle: A\nno-closer\n").unwrap();
        let code = execute(vec![
            "--workspace-root".to_string(),
            dir.path().display().to_string(),
            "--adr-root".to_string(),
            adr_dir.display().to_string(),
        ]);
        assert_eq!(code, 2);
    }

    #[test]
    fn looks_like_path_recognizes_separators() {
        assert!(looks_like_path("foo/bar"));
        assert!(looks_like_path("foo\\bar"), "Windows backslash separator");
        assert!(looks_like_path("Cargo.toml"));
        assert!(looks_like_path("script.sh"));
        assert!(looks_like_path("workflow.yaml"));
        assert!(!looks_like_path("crate::module::test"));
        assert!(!looks_like_path("clippy::lint_name"));
    }
}
