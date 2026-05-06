//! `handler-scenario-coverage` CLI.
//!
//! Mirror of the [`crate::coverage_breakouts`] shim — flat-flag parsing,
//! `execute(argv) -> i32` so the bin file stays branch-free for CRAP credit.
//!
//! Exit codes follow the producer's 0/2 contract:
//!   * `0` — clean run.
//!   * `1` — CLI / I/O error. Artifacts may not have been written.
//!   * `2` — diagnostics non-empty (unresolvable routes, orphan
//!     observations, JSONL parse errors). Artifacts ARE written so the
//!     gate / human can inspect them.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use super::artifact::HandlerScenarioArtifact;
use super::markdown;
use super::producer::{ProducerInput, ProducerOutput, run};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub workspace_root: PathBuf,
    pub jsonl_dir: PathBuf,
    pub output_json: PathBuf,
    pub output_md: Option<PathBuf>,
    pub now_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseOutcome {
    Run,
    ShowHelp,
}

pub const HELP_TEXT: &str = "handler-scenario-coverage — emit per-route BDD scenario coverage artifact\n\
                             \n\
                             Usage: handler-scenario-coverage \\\n  \
                             --workspace-root <DIR> \\\n  \
                             --jsonl-dir <DIR> \\\n  \
                             --output-json <PATH> \\\n  \
                             [--output-md <PATH>] \\\n  \
                             [--now <ISO-8601>]";

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<(Args, ParseOutcome)> {
    let mut workspace_root: Option<PathBuf> = None;
    let mut jsonl_dir: Option<PathBuf> = None;
    let mut output_json: Option<PathBuf> = None;
    let mut output_md: Option<PathBuf> = None;
    let mut now_override: Option<String> = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--workspace-root" => {
                workspace_root = Some(PathBuf::from(next_value(&mut iter, "--workspace-root")?));
            }
            "--jsonl-dir" => {
                jsonl_dir = Some(PathBuf::from(next_value(&mut iter, "--jsonl-dir")?));
            }
            "--output-json" => {
                output_json = Some(PathBuf::from(next_value(&mut iter, "--output-json")?));
            }
            "--output-md" => {
                output_md = Some(PathBuf::from(next_value(&mut iter, "--output-md")?));
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
        jsonl_dir: jsonl_dir.context("--jsonl-dir is required")?,
        output_json: output_json.context("--output-json is required")?,
        output_md,
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
        jsonl_dir: PathBuf::new(),
        output_json: PathBuf::new(),
        output_md: None,
        now_override: None,
    }
}

#[allow(
    clippy::similar_names,
    reason = "argv is the raw process input, args is the parsed value — the relation is intentional"
)]
pub fn execute(argv: Vec<String>) -> i32 {
    let (args, outcome) = match parse_args(argv) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("handler-scenario-coverage: {e:#}");
            return 1;
        }
    };
    if outcome == ParseOutcome::ShowHelp {
        println!("{HELP_TEXT}");
        return 0;
    }
    let input = ProducerInput {
        workspace_root: args.workspace_root,
        jsonl_dir: args.jsonl_dir,
        now_override: args.now_override,
    };
    let output = match run(&input) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("handler-scenario-coverage: {e:#}");
            return 1;
        }
    };
    if let Err(e) = write_artifacts(&args.output_json, args.output_md.as_deref(), &output) {
        eprintln!("handler-scenario-coverage: {e:#}");
        return 1;
    }
    log_summary(&args.output_json, &output);
    output.exit_code
}

fn write_artifacts(
    json_path: &std::path::Path,
    md_path: Option<&std::path::Path>,
    output: &ProducerOutput,
) -> Result<()> {
    write_json_artifact(json_path, &output.artifact)?;
    if let Some(md) = md_path {
        write_markdown_artifact(md, &output.artifact)?;
    }
    Ok(())
}

fn write_json_artifact(
    json_path: &std::path::Path,
    artifact: &HandlerScenarioArtifact,
) -> Result<()> {
    let json = serde_json::to_string_pretty(artifact).context("serialise json artifact")?;
    ensure_parent_dir(json_path)?;
    std::fs::write(json_path, format!("{json}\n"))
        .with_context(|| format!("write {}", json_path.display()))
}

fn write_markdown_artifact(
    md_path: &std::path::Path,
    artifact: &HandlerScenarioArtifact,
) -> Result<()> {
    ensure_parent_dir(md_path)?;
    let body = markdown::render(artifact);
    std::fs::write(md_path, body).with_context(|| format!("write {}", md_path.display()))
}

fn ensure_parent_dir(path: &std::path::Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))
}

fn log_summary(json_path: &std::path::Path, output: &ProducerOutput) {
    let crates = output.artifact.by_crate.len();
    let handlers: usize = output
        .artifact
        .by_crate
        .iter()
        .map(|c| c.handlers.len())
        .sum();
    let rows = output.artifact.diagnostics.rows_consumed;
    let unresolvable = output.artifact.diagnostics.unresolvable_routes.len();
    let orphans = output.artifact.diagnostics.orphan_observations.len();
    let parse_errors = output.artifact.diagnostics.jsonl_errors.len();
    eprintln!(
        "handler-scenario-coverage: wrote {} ({crates} crate(s), {handlers} handler(s), \
         {rows} row(s), {unresolvable} unresolvable, {orphans} orphan(s), \
         {parse_errors} parse error(s))",
        json_path.display()
    );
}

#[allow(
    dead_code,
    reason = "consumed by integration tests in tests/handler_scenario_cli.rs"
)]
pub(crate) fn artifact_for_testing(json: &str) -> Result<HandlerScenarioArtifact> {
    serde_json::from_str(json).context("deserialise artifact")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario_coverage::artifact::Diagnostics;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parse_args_accepts_minimum_flags() {
        let (args, outcome) = parse_args(argv(&[
            "--workspace-root",
            "/ws",
            "--jsonl-dir",
            "/jsonl",
            "--output-json",
            "/out.json",
        ]))
        .unwrap();
        assert_eq!(outcome, ParseOutcome::Run);
        assert_eq!(args.workspace_root, PathBuf::from("/ws"));
        assert_eq!(args.jsonl_dir, PathBuf::from("/jsonl"));
        assert_eq!(args.output_json, PathBuf::from("/out.json"));
        assert_eq!(args.output_md, None);
    }

    #[test]
    fn parse_args_accepts_md_output() {
        let (args, _) = parse_args(argv(&[
            "--workspace-root",
            "/ws",
            "--jsonl-dir",
            "/jsonl",
            "--output-json",
            "/out.json",
            "--output-md",
            "/out.md",
            "--now",
            "2026-05-06T00:00:00Z",
        ]))
        .unwrap();
        assert_eq!(args.output_md, Some(PathBuf::from("/out.md")));
        assert_eq!(args.now_override.as_deref(), Some("2026-05-06T00:00:00Z"));
    }

    #[test]
    fn parse_args_help_short_circuits() {
        let (_, outcome) = parse_args(argv(&["--help"])).unwrap();
        assert_eq!(outcome, ParseOutcome::ShowHelp);
    }

    #[test]
    fn parse_args_rejects_missing_required() {
        assert!(parse_args(argv(&["--workspace-root", "/x"])).is_err());
    }

    #[test]
    fn parse_args_rejects_unknown_arg() {
        assert!(parse_args(argv(&["--unknown"])).is_err());
    }

    // ---------------------------------------------------------------------
    // execute() / write_artifacts() integration — mock workspace, both
    // output paths under tempdir, exit-code contract checked.
    // ---------------------------------------------------------------------

    fn write_minimal_workspace(root: &std::path::Path) {
        let crate_dir = root.join("crates/demo");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            crate_dir.join("src/lib.rs"),
            "use axum::{Router, routing::get};\npub fn r() -> Router { Router::new().route(\"/api/h\", get(h)) }\nfn h() {}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crap4rs.toml"),
            "preset = \"strict\"\nexclude = []\n",
        )
        .unwrap();
    }

    #[test]
    fn execute_writes_json_and_md_artifacts_then_returns_zero_on_clean_run() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_workspace(tmp.path());
        let json_out = tmp.path().join("out/artifact.json");
        let md_out = tmp.path().join("out/artifact.md");
        let code = execute(
            [
                "--workspace-root",
                tmp.path().to_str().unwrap(),
                "--jsonl-dir",
                tmp.path().join("empty-bdd-dir").to_str().unwrap(),
                "--output-json",
                json_out.to_str().unwrap(),
                "--output-md",
                md_out.to_str().unwrap(),
                "--now",
                "2026-05-06T00:00:00Z",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        assert_eq!(code, 0);
        let json = std::fs::read_to_string(&json_out).unwrap();
        let md = std::fs::read_to_string(&md_out).unwrap();
        assert!(json.contains("\"version\""));
        assert!(json.ends_with('\n'));
        assert!(md.contains("# Handler ↔ Scenario Map"));
    }

    #[test]
    fn execute_returns_zero_on_help_short_circuit() {
        let code = execute(vec!["--help".into()]);
        assert_eq!(code, 0);
    }

    #[test]
    fn execute_returns_one_on_parse_error() {
        let code = execute(vec!["--unknown-flag".into()]);
        assert_eq!(code, 1);
    }

    #[test]
    fn execute_returns_one_on_missing_required_flag() {
        let code = execute(vec![
            "--workspace-root".into(),
            "/some/path".into(),
            // missing --jsonl-dir and --output-json
        ]);
        assert_eq!(code, 1);
    }

    #[test]
    fn execute_returns_two_on_diagnostics_present() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_workspace(tmp.path());
        let jsonl_dir = tmp.path().join("bdd");
        std::fs::create_dir_all(&jsonl_dir).unwrap();
        // Row matches no walked route → orphan → exit 2.
        std::fs::write(
            jsonl_dir.join("api.jsonl"),
            "{\"feature_path\":\"f\",\"feature_title\":\"F\",\"scenario\":\"s\",\"method\":\"GET\",\"matched_path\":\"/no/such\",\"status\":200,\"status_class\":\"happy\"}\n",
        )
        .unwrap();
        let code = execute(
            [
                "--workspace-root",
                tmp.path().to_str().unwrap(),
                "--jsonl-dir",
                jsonl_dir.to_str().unwrap(),
                "--output-json",
                tmp.path().join("a.json").to_str().unwrap(),
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn execute_returns_one_when_workspace_root_has_no_crates() {
        let tmp = tempfile::tempdir().unwrap();
        // No crates/ or apps/ — discover_crates errors.
        let code = execute(
            [
                "--workspace-root",
                tmp.path().to_str().unwrap(),
                "--jsonl-dir",
                tmp.path().join("nope").to_str().unwrap(),
                "--output-json",
                tmp.path().join("a.json").to_str().unwrap(),
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn write_artifacts_creates_parent_dirs_for_both_outputs() {
        let tmp = tempfile::tempdir().unwrap();
        let nested_json = tmp.path().join("a/b/c/out.json");
        let nested_md = tmp.path().join("x/y/z/out.md");
        let output = ProducerOutput {
            artifact: HandlerScenarioArtifact {
                version: 1,
                generated_at: "x".into(),
                by_crate: Vec::new(),
                diagnostics: Diagnostics::default(),
            },
            exit_code: 0,
        };
        write_artifacts(&nested_json, Some(&nested_md), &output).unwrap();
        assert!(nested_json.is_file());
        assert!(nested_md.is_file());
    }

    #[test]
    fn write_artifacts_skips_md_when_path_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("only.json");
        let output = ProducerOutput {
            artifact: HandlerScenarioArtifact {
                version: 1,
                generated_at: "x".into(),
                by_crate: Vec::new(),
                diagnostics: Diagnostics::default(),
            },
            exit_code: 0,
        };
        write_artifacts(&json_path, None, &output).unwrap();
        assert!(json_path.is_file());
    }

    #[test]
    fn artifact_for_testing_round_trips_through_serde() {
        let json = r#"{"version":1,"generated_at":"x","by_crate":[],"diagnostics":{"unresolvable_routes":[],"orphan_observations":[],"excluded_crates":[],"jsonl_errors":[],"rows_consumed":0}}"#;
        let artifact = artifact_for_testing(json).unwrap();
        assert_eq!(artifact.version, 1);
    }
}
