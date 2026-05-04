//! `coverage-breakouts` — produce the per-handler-branch coverage artifact
//! consumed by `scorecard aggregate --coverage-breakouts-json`.
//!
//! CC=1 shim around [`docs_gen::coverage::run`] (mokumo#583, scorecard V4
//! `Row::CoverageDelta.breakouts.handlers[]`). Coverage credit lands on
//! the library — see the `lcov-dedup` and `adr-validate` siblings under
//! `tools/docs-gen/src/bin/`.
//!
//! Usage:
//!
//! ```text
//! coverage-breakouts \
//!   --workspace-root /workspace \
//!   --coverage-json /workspace/coverage-branches.json \
//!   --output /workspace/coverage-breakouts.json
//! ```
//!
//! Exit codes:
//! - `0` — artifact written, no diagnostics.
//! - `1` — I/O / parse error.
//! - `2` — diagnostics non-empty (unresolved handlers or unresolvable
//!   routes); artifact still written so the operator can inspect the
//!   partial output. Loudest signal possible without crashing.

use std::path::PathBuf;
use std::process::ExitCode;

use docs_gen::coverage::{ProducerInput, run};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut workspace_root: Option<PathBuf> = None;
    let mut coverage_json: Option<PathBuf> = None;
    let mut output_path: Option<PathBuf> = None;
    let mut now_override: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace-root" => workspace_root = args.next().map(PathBuf::from),
            "--coverage-json" => coverage_json = args.next().map(PathBuf::from),
            "--output" => output_path = args.next().map(PathBuf::from),
            "--now" => now_override = args.next(),
            "-h" | "--help" => {
                eprintln!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("coverage-breakouts: unknown argument `{other}`\n{USAGE}");
                return ExitCode::from(1);
            }
        }
    }
    let Some(workspace_root) = workspace_root else {
        eprintln!("coverage-breakouts: --workspace-root is required\n{USAGE}");
        return ExitCode::from(1);
    };
    let Some(coverage_json) = coverage_json else {
        eprintln!("coverage-breakouts: --coverage-json is required\n{USAGE}");
        return ExitCode::from(1);
    };
    let Some(output_path) = output_path else {
        eprintln!("coverage-breakouts: --output is required\n{USAGE}");
        return ExitCode::from(1);
    };

    let input = ProducerInput {
        workspace_root,
        coverage_json,
        now_override,
    };
    match run(&input) {
        Ok(out) => {
            let json = match serde_json::to_string_pretty(&out.artifact) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("coverage-breakouts: serialise artifact: {e}");
                    return ExitCode::from(1);
                }
            };
            if let Err(e) = std::fs::write(&output_path, format!("{json}\n")) {
                eprintln!("coverage-breakouts: write {}: {e}", output_path.display());
                return ExitCode::from(1);
            }
            // Emit a one-line summary on stderr for CI logs.
            let crates = out.artifact.by_crate.len();
            let handlers: usize = out.artifact.by_crate.iter().map(|c| c.handlers.len()).sum();
            let unresolved = out.artifact.diagnostics.unresolved_handlers.len();
            let unresolvable = out.artifact.diagnostics.unresolvable_routes.len();
            eprintln!(
                "coverage-breakouts: wrote {} ({crates} crate(s), {handlers} handler(s), {unresolved} unresolved, {unresolvable} unresolvable)",
                output_path.display()
            );
            if out.exit_code == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
        Err(e) => {
            eprintln!("coverage-breakouts: {e:#}");
            ExitCode::from(1)
        }
    }
}

const USAGE: &str = "usage: coverage-breakouts \\\n  --workspace-root <DIR> \\\n  --coverage-json <PATH> \\\n  --output <PATH> \\\n  [--now <ISO-8601>]";
