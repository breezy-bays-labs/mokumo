//! `pub-api-spec-audit` CLI.
//!
//! Mirror of [`super::super::scenario_coverage::cli`] — flat-flag parsing,
//! `execute(argv) -> i32` so the bin file stays branch-free for CRAP credit.
//!
//! Exit codes follow the producer's 0/2 contract:
//!   * `0` — clean run, artifact written.
//!   * `1` — CLI / I/O error. Artifacts may not have been written.
//!   * `2` — diagnostics non-empty (parse errors, lcov errors).

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use super::artifact::PubApiAuditArtifact;
use super::markdown;
use super::producer::{ProducerInput, ProducerOutput, run};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub workspace_root: PathBuf,
    pub lcov_paths: Vec<PathBuf>,
    pub output_json: PathBuf,
    pub output_md: Option<PathBuf>,
    pub now_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseOutcome {
    Run,
    ShowHelp,
}

pub const HELP_TEXT: &str = "pub-api-spec-audit — emit public-API BDD-coverage artifact\n\
                             \n\
                             Usage: pub-api-spec-audit \\\n  \
                             --workspace-root <DIR> \\\n  \
                             --lcov <PATH>... \\\n  \
                             --output-json <PATH> \\\n  \
                             [--output-md <PATH>] \\\n  \
                             [--now <ISO-8601>]";

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<(Args, ParseOutcome)> {
    let mut workspace_root: Option<PathBuf> = None;
    let mut lcov_paths: Vec<PathBuf> = Vec::new();
    let mut output_json: Option<PathBuf> = None;
    let mut output_md: Option<PathBuf> = None;
    let mut now_override: Option<String> = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--workspace-root" => {
                workspace_root = Some(PathBuf::from(next_value(&mut iter, "--workspace-root")?));
            }
            "--lcov" => {
                lcov_paths.push(PathBuf::from(next_value(&mut iter, "--lcov")?));
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
        lcov_paths,
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
        lcov_paths: Vec::new(),
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
            eprintln!("pub-api-spec-audit: {e:#}");
            return 1;
        }
    };
    if outcome == ParseOutcome::ShowHelp {
        println!("{HELP_TEXT}");
        return 0;
    }
    let input = ProducerInput {
        workspace_root: args.workspace_root,
        lcov_paths: args.lcov_paths,
        now_override: args.now_override,
    };
    let output = match run(&input) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("pub-api-spec-audit: {e:#}");
            return 1;
        }
    };
    if let Err(e) = write_artifacts(&args.output_json, args.output_md.as_deref(), &output) {
        eprintln!("pub-api-spec-audit: {e:#}");
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

fn write_json_artifact(json_path: &std::path::Path, artifact: &PubApiAuditArtifact) -> Result<()> {
    let json = serde_json::to_string_pretty(artifact).context("serialise json artifact")?;
    ensure_parent_dir(json_path)?;
    std::fs::write(json_path, format!("{json}\n"))
        .with_context(|| format!("write {}", json_path.display()))
}

fn write_markdown_artifact(
    md_path: &std::path::Path,
    artifact: &PubApiAuditArtifact,
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
    let items = output.artifact.diagnostics.items_walked;
    let covered: u64 = output
        .artifact
        .by_crate
        .iter()
        .flat_map(|c| c.items.iter())
        .filter(|i| i.is_bdd_covered())
        .count() as u64;
    let lcov = output.artifact.diagnostics.lcov_files_consumed;
    let parse_errors = output.artifact.diagnostics.parse_errors.len();
    let lcov_errors = output.artifact.diagnostics.lcov_errors.len();
    eprintln!(
        "pub-api-spec-audit: wrote {} ({crates} crate(s), {items} pub item(s), \
         {covered} BDD-covered, {lcov} lcov file(s), {parse_errors} parse error(s), \
         {lcov_errors} lcov error(s))",
        json_path.display()
    );
}

#[allow(
    dead_code,
    reason = "consumed by integration tests in tests/pub_api_spec_audit_cli.rs"
)]
pub(crate) fn artifact_for_testing(json: &str) -> Result<PubApiAuditArtifact> {
    serde_json::from_str(json).context("deserialise artifact")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    fn write_minimal_workspace(root: &std::path::Path) {
        let crate_dir = root.join("crates/demo");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(crate_dir.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
        std::fs::write(
            root.join("crap4rs.toml"),
            "preset = \"strict\"\nexclude = []\n",
        )
        .unwrap();
    }

    #[test]
    fn parse_args_accepts_minimum_flags() {
        let (args, outcome) = parse_args(argv(&[
            "--workspace-root",
            "/ws",
            "--output-json",
            "/out.json",
        ]))
        .unwrap();
        assert_eq!(outcome, ParseOutcome::Run);
        assert_eq!(args.workspace_root, PathBuf::from("/ws"));
        assert!(args.lcov_paths.is_empty());
        assert_eq!(args.output_md, None);
    }

    #[test]
    fn parse_args_collects_multiple_lcov_paths() {
        let (args, _) = parse_args(argv(&[
            "--workspace-root",
            "/ws",
            "--lcov",
            "/a.lcov",
            "--lcov",
            "/b.lcov",
            "--output-json",
            "/out.json",
        ]))
        .unwrap();
        assert_eq!(args.lcov_paths.len(), 2);
    }

    #[test]
    fn parse_args_accepts_md_output_and_now_override() {
        let (args, _) = parse_args(argv(&[
            "--workspace-root",
            "/ws",
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

    #[test]
    fn parse_args_rejects_lcov_without_value() {
        assert!(parse_args(argv(&["--lcov"])).is_err());
    }

    #[test]
    fn execute_writes_json_and_md_artifacts() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_workspace(tmp.path());
        let json_out = tmp.path().join("out/audit.json");
        let md_out = tmp.path().join("out/audit.md");
        let code = execute(
            [
                "--workspace-root",
                tmp.path().to_str().unwrap(),
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
        assert!(json_out.is_file());
        assert!(md_out.is_file());
    }

    #[test]
    fn execute_returns_zero_on_help() {
        assert_eq!(execute(vec!["--help".into()]), 0);
    }

    #[test]
    fn execute_returns_one_on_parse_error() {
        assert_eq!(execute(vec!["--unknown-flag".into()]), 1);
    }

    #[test]
    fn execute_returns_one_on_missing_required_flag() {
        assert_eq!(execute(vec!["--workspace-root".into(), "/x".into()]), 1);
    }

    #[test]
    fn execute_returns_one_when_workspace_has_no_crates() {
        let tmp = tempfile::tempdir().unwrap();
        let code = execute(
            [
                "--workspace-root",
                tmp.path().to_str().unwrap(),
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
    fn execute_returns_two_when_lcov_path_missing() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_workspace(tmp.path());
        let code = execute(
            [
                "--workspace-root",
                tmp.path().to_str().unwrap(),
                "--lcov",
                tmp.path().join("missing.lcov").to_str().unwrap(),
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
    fn write_artifacts_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("a/b/c/out.json");
        let md_path = tmp.path().join("x/y/out.md");
        let output = ProducerOutput {
            artifact: PubApiAuditArtifact {
                version: 1,
                generated_at: "x".into(),
                by_crate: Vec::new(),
                diagnostics: super::super::artifact::Diagnostics::default(),
            },
            exit_code: 0,
        };
        write_artifacts(&json_path, Some(&md_path), &output).unwrap();
        assert!(json_path.is_file());
        assert!(md_path.is_file());
    }

    #[test]
    fn artifact_for_testing_round_trips() {
        let json = r#"{"version":1,"generated_at":"x","by_crate":[],"diagnostics":{"excluded_crates":[],"parse_errors":[],"lcov_errors":[],"items_walked":0,"lcov_files_consumed":0}}"#;
        let artifact = artifact_for_testing(json).unwrap();
        assert_eq!(artifact.version, 1);
    }
}
