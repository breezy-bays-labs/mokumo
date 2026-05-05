//! Library implementation of the `coverage-breakouts` binary.
//!
//! Exposed here (not `src/bin/`) so the CRAP gate sees these branches via
//! `cargo nextest`'s lib-test profile — `cargo-llvm-cov nextest` doesn't
//! credit `src/bin/` code, so any non-trivial bin reads as 0% covered and
//! trips the gate. The `src/bin/coverage-breakouts.rs` shim is a one-line
//! forwarder; logic, tests, and decision branches all live here.
//!
//! Shape matches the `validate::execute` sibling deliberately — same CLI
//! conventions, same exit-code contract.
//!
//! Exit codes:
//! - `0` — artifact written, no diagnostics.
//! - `1` — CLI error, I/O error, or producer panic — no artifact written.
//! - `2` — diagnostics non-empty (unresolved handlers or unresolvable
//!   routes); artifact still written so the operator can inspect partial
//!   output. Loudest signal possible without losing data.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

use crate::coverage::{ProducerInput, ProducerOutput, run};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub workspace_root: PathBuf,
    pub coverage_json: PathBuf,
    pub output_path: PathBuf,
    pub now_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseOutcome {
    /// Continue to `run`.
    Run,
    /// `--help` short-circuit; the caller should print help and exit 0.
    ShowHelp,
}

pub const HELP_TEXT: &str = "coverage-breakouts — emit per-handler branch-coverage artifact\n\
                             \n\
                             Usage: coverage-breakouts \\\n  \
                             --workspace-root <DIR> \\\n  \
                             --coverage-json <PATH> \\\n  \
                             --output <PATH> \\\n  \
                             [--now <ISO-8601>]";

/// Parses a flat-flag CLI. Takes the args iterator explicitly so tests can
/// drive it without touching `env::args`.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<(Args, ParseOutcome)> {
    let mut workspace_root: Option<PathBuf> = None;
    let mut coverage_json: Option<PathBuf> = None;
    let mut output_path: Option<PathBuf> = None;
    let mut now_override: Option<String> = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--workspace-root" => {
                workspace_root = Some(PathBuf::from(next_value(&mut iter, "--workspace-root")?));
            }
            "--coverage-json" => {
                coverage_json = Some(PathBuf::from(next_value(&mut iter, "--coverage-json")?));
            }
            "--output" => {
                output_path = Some(PathBuf::from(next_value(&mut iter, "--output")?));
            }
            "--now" => {
                now_override = Some(next_value(&mut iter, "--now")?);
            }
            "-h" | "--help" => {
                return Ok((empty_args(), ParseOutcome::ShowHelp));
            }
            other => bail!("unknown argument `{other}` (try --help)"),
        }
    }
    let args = Args {
        workspace_root: workspace_root.context("--workspace-root is required")?,
        coverage_json: coverage_json.context("--coverage-json is required")?,
        output_path: output_path.context("--output is required")?,
        now_override,
    };
    Ok((args, ParseOutcome::Run))
}

fn next_value<I: Iterator<Item = String>>(iter: &mut I, flag: &str) -> Result<String> {
    iter.next()
        .with_context(|| format!("{flag} requires a value"))
}

fn empty_args() -> Args {
    Args {
        workspace_root: PathBuf::new(),
        coverage_json: PathBuf::new(),
        output_path: PathBuf::new(),
        now_override: None,
    }
}

/// Whole CLI dispatch in one function, returning an exit code so the bin
/// shim can stay branch-free. Errors are written to stderr; the artifact is
/// written to `args.output_path`.
#[allow(
    clippy::similar_names,
    reason = "argv is the raw process input, args is the parsed value — relating them by name is intentional"
)]
pub fn execute(argv: Vec<String>) -> i32 {
    let (args, outcome) = match parse_args(argv) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("coverage-breakouts: {e:#}");
            return 1;
        }
    };
    if outcome == ParseOutcome::ShowHelp {
        println!("{HELP_TEXT}");
        return 0;
    }
    let input = ProducerInput {
        workspace_root: args.workspace_root,
        coverage_json: args.coverage_json,
        now_override: args.now_override,
    };
    let output = match run(&input) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("coverage-breakouts: {e:#}");
            return 1;
        }
    };
    match write_artifact(&args.output_path, &output) {
        Ok(()) => {
            log_summary(&args.output_path, &output);
            // ProducerOutput.exit_code already follows the 0/2 contract.
            output.exit_code
        }
        Err(e) => {
            eprintln!("coverage-breakouts: {e:#}");
            1
        }
    }
}

fn write_artifact(output_path: &std::path::Path, output: &ProducerOutput) -> Result<()> {
    let json = serde_json::to_string_pretty(&output.artifact).context("serialise artifact")?;
    std::fs::write(output_path, format!("{json}\n"))
        .with_context(|| format!("write {}", output_path.display()))?;
    Ok(())
}

fn log_summary(output_path: &std::path::Path, output: &ProducerOutput) {
    let crates = output.artifact.by_crate.len();
    let handlers: usize = output
        .artifact
        .by_crate
        .iter()
        .map(|c| c.handlers.len())
        .sum();
    let unresolved = output.artifact.diagnostics.unresolved_handlers.len();
    let unresolvable = output.artifact.diagnostics.unresolvable_routes.len();
    eprintln!(
        "coverage-breakouts: wrote {} ({crates} crate(s), {handlers} handler(s), {unresolved} unresolved, {unresolvable} unresolvable)",
        output_path.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parse_args_accepts_all_required_flags() {
        let (args, outcome) = parse_args(argv(&[
            "--workspace-root",
            "/ws",
            "--coverage-json",
            "/cov.json",
            "--output",
            "/out.json",
        ]))
        .unwrap();
        assert_eq!(outcome, ParseOutcome::Run);
        assert_eq!(args.workspace_root, PathBuf::from("/ws"));
        assert_eq!(args.coverage_json, PathBuf::from("/cov.json"));
        assert_eq!(args.output_path, PathBuf::from("/out.json"));
        assert!(args.now_override.is_none());
    }

    #[test]
    fn parse_args_carries_now_override() {
        let (args, _) = parse_args(argv(&[
            "--workspace-root",
            "/ws",
            "--coverage-json",
            "/cov.json",
            "--output",
            "/out.json",
            "--now",
            "2026-05-04T00:00:00Z",
        ]))
        .unwrap();
        assert_eq!(args.now_override.as_deref(), Some("2026-05-04T00:00:00Z"));
    }

    #[test]
    fn parse_args_short_circuits_on_help() {
        let (_, outcome) = parse_args(argv(&["--help"])).unwrap();
        assert_eq!(outcome, ParseOutcome::ShowHelp);
        let (_, outcome) = parse_args(argv(&["-h"])).unwrap();
        assert_eq!(outcome, ParseOutcome::ShowHelp);
    }

    #[test]
    fn parse_args_errors_on_missing_workspace_root() {
        let err = parse_args(argv(&[
            "--coverage-json",
            "/cov.json",
            "--output",
            "/out.json",
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("--workspace-root"));
    }

    #[test]
    fn parse_args_errors_on_missing_coverage_json() {
        let err =
            parse_args(argv(&["--workspace-root", "/ws", "--output", "/out.json"])).unwrap_err();
        assert!(err.to_string().contains("--coverage-json"));
    }

    #[test]
    fn parse_args_errors_on_missing_output() {
        let err = parse_args(argv(&[
            "--workspace-root",
            "/ws",
            "--coverage-json",
            "/cov.json",
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("--output"));
    }

    #[test]
    fn parse_args_errors_on_unknown_flag() {
        let err = parse_args(argv(&["--bogus"])).unwrap_err();
        assert!(err.to_string().contains("--bogus"));
    }

    #[test]
    fn parse_args_errors_on_flag_without_value() {
        let err = parse_args(argv(&["--workspace-root"])).unwrap_err();
        assert!(err.to_string().contains("--workspace-root"));
    }

    #[test]
    fn execute_returns_zero_on_help() {
        assert_eq!(execute(argv(&["--help"])), 0);
    }

    #[test]
    fn execute_returns_one_on_parse_error() {
        assert_eq!(execute(argv(&["--bogus"])), 1);
    }

    #[test]
    fn execute_returns_one_on_producer_error() {
        // No coverage.json at the supplied path.
        let tmp = tempdir().unwrap();
        let out = tmp.path().join("out.json");
        let rc = execute(argv(&[
            "--workspace-root",
            tmp.path().to_str().unwrap(),
            "--coverage-json",
            "/does-not-exist.json",
            "--output",
            out.to_str().unwrap(),
        ]));
        assert_eq!(rc, 1);
    }
}
