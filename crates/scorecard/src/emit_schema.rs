//! `emit-schema` entry-point logic, split out of `bin/emit-schema.rs` so
//! the argument parser, the schema serializer, and the file writer are
//! reachable as plain library functions and exercised by unit tests.
//!
//! The bin target is a one-line wrapper around [`main_entry`]; everything
//! testable lives here.
//!
//! The binary emits two JSON Schema artifacts:
//!
//! - `.config/scorecard/schema.json` — wire schema for the `scorecard.json`
//!   artifact the producer writes for the renderer.
//! - `.config/scorecard/quality.config.schema.json` — operator-facing schema
//!   for `.config/scorecard/quality.toml`, validated against by ajv on the
//!   `scorecard-drift` CI gate.
//!
//! The two artifacts have different audiences (the renderer vs. operators)
//! but share the deps-zero invariant: both are derived from `schemars`
//! `JsonSchema` derives in the lib, so `cargo build -p scorecard
//! --no-default-features --bin emit-schema` keeps `toml` out of the
//! transitive dep tree.

#![doc(hidden)]

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use schemars::schema_for;

use crate::Scorecard;
use crate::schema_postprocess::{
    inject_failure_detail_xss_pattern, inject_red_requires_detail,
    strip_nonstandard_number_formats, tighten_url_fields,
};
use crate::threshold::ThresholdConfig;

/// Default committed path for the wire schema (the artifact the
/// renderer validates against).
pub const SCORECARD_SCHEMA_DEFAULT_PATH: &str = ".config/scorecard/schema.json";

/// Default committed path for the operator-facing schema (the artifact
/// ajv validates the committed `quality.toml` against).
pub const QUALITY_CONFIG_SCHEMA_DEFAULT_PATH: &str = ".config/scorecard/quality.config.schema.json";

/// Which schema the binary should write on this invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Wire schema for `scorecard.json`.
    Scorecard,
    /// Operator schema for `quality.toml`.
    Quality,
    /// Both wire and operator schemas, each to its default committed path.
    Both,
}

/// Outcome of [`parse_args`] — either an emit request (target + optional
/// override path), or a `--help` request.
#[derive(Debug, PartialEq, Eq)]
pub enum ParsedArgs {
    /// Emit `target` to `out` if provided, otherwise to that target's
    /// default path. `out` is rejected when `target == Target::Both` —
    /// two outputs to one path is ill-defined.
    Run {
        target: Target,
        out: Option<PathBuf>,
    },
    Help,
}

/// Parse `--target`, `--out`, `--help`, `-h` from an iterator of raw OS
/// args (the caller passes `std::env::args_os().skip(1)`).
///
/// `--target` is optional and defaults to [`Target::Both`] when absent.
/// `--out PATH` overrides the default committed path; it is rejected
/// when paired with `--target both` because two artifacts cannot share
/// one output path.
///
/// Returned errors carry the exact human-readable message printed by the
/// binary, so callers can `eprintln!` them directly.
pub fn parse_args<I>(args: I) -> Result<ParsedArgs, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut iter = args.into_iter();
    let mut out: Option<PathBuf> = None;
    let mut target: Option<Target> = None;
    while let Some(arg) = iter.next() {
        match arg.to_string_lossy().as_ref() {
            "--out" => {
                let v = iter
                    .next()
                    .ok_or_else(|| "--out requires a path".to_string())?;
                out = Some(PathBuf::from(v));
            }
            "--target" => {
                let v = iter
                    .next()
                    .ok_or_else(|| "--target requires a value".to_string())?;
                let parsed = match v.to_string_lossy().as_ref() {
                    "scorecard" => Target::Scorecard,
                    "quality" => Target::Quality,
                    "both" => Target::Both,
                    other => {
                        return Err(format!(
                            "--target must be one of `scorecard`, `quality`, `both`; got `{other}`"
                        ));
                    }
                };
                target = Some(parsed);
            }
            "--help" | "-h" => return Ok(ParsedArgs::Help),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let target = match (target, &out) {
        (Some(t), _) => t,
        // `--out <PATH>` without `--target` is the single-artifact
        // shorthand: it implies `--target scorecard` because the wire
        // schema is the only artifact that ships at a caller-chosen
        // path. The default (no flags) writes both artifacts at their
        // canonical paths under `.config/scorecard/`.
        (None, Some(_)) => Target::Scorecard,
        (None, None) => Target::Both,
    };
    if matches!(target, Target::Both) && out.is_some() {
        return Err(
            "--out cannot be combined with --target both (two artifacts cannot share one path)"
                .to_string(),
        );
    }
    Ok(ParsedArgs::Run { target, out })
}

/// Render the post-processed wire JSON Schema for [`Scorecard`] as a
/// UTF-8 string. Trailing newline included so the committed file is
/// POSIX-clean (most editors and `git diff` flag missing trailing
/// newlines).
pub fn render_schema_string() -> String {
    let mut schema = schema_for!(Scorecard);
    inject_red_requires_detail(&mut schema);
    inject_failure_detail_xss_pattern(&mut schema);
    tighten_url_fields(&mut schema);
    let mut content = serde_json::to_string_pretty(&schema)
        .expect("scorecard schema serializes to a JSON string");
    content.push('\n');
    content
}

/// Render the operator-facing JSON Schema for [`ThresholdConfig`] as a
/// UTF-8 string.
///
/// Uses `schemars` derives (deps-zero) plus a single post-process step
/// that strips schemars' non-standard numeric `format` annotations
/// (`"double"`, `"uint32"`, ...). ajv-cli treats those as unrecognized
/// schema formats and degrades validation; removing them keeps ajv
/// strict and the shape drift it surfaces sharp. `deny_unknown_fields`
/// from the type derives carries through to the JSON Schema's
/// `additionalProperties: false`, which is sufficient for ajv to
/// reject typos and drift in the committed `quality.toml` (after
/// `tomllib` projects it to JSON on the drift gate).
pub fn render_quality_config_schema_string() -> String {
    let mut schema = schema_for!(ThresholdConfig);
    strip_nonstandard_number_formats(&mut schema);
    let mut content = serde_json::to_string_pretty(&schema)
        .expect("quality config schema serializes to a JSON string");
    content.push('\n');
    content
}

/// Write the wire schema (post-processed [`Scorecard`] derive) to
/// `out_path`, creating parent directories as needed.
pub fn write_schema(out_path: &Path) -> io::Result<()> {
    write_string_to_path(out_path, &render_schema_string())
}

/// Write the operator-facing schema (plain [`ThresholdConfig`] derive)
/// to `out_path`, creating parent directories as needed.
pub fn write_quality_config_schema(out_path: &Path) -> io::Result<()> {
    write_string_to_path(out_path, &render_quality_config_schema_string())
}

fn write_string_to_path(out_path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_path, content)
}

fn emit(target: Target, out: Option<PathBuf>) -> Result<(), String> {
    match target {
        Target::Scorecard => {
            let path = out.unwrap_or_else(|| PathBuf::from(SCORECARD_SCHEMA_DEFAULT_PATH));
            write_schema(&path).map_err(|e| format!("failed to write {}: {e}", path.display()))?;
        }
        Target::Quality => {
            let path = out.unwrap_or_else(|| PathBuf::from(QUALITY_CONFIG_SCHEMA_DEFAULT_PATH));
            write_quality_config_schema(&path)
                .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
        }
        Target::Both => {
            // `--out` is rejected for `--target both` at parse time, so
            // `out` is `None` here; both schemas land at their defaults.
            let scorecard_path = PathBuf::from(SCORECARD_SCHEMA_DEFAULT_PATH);
            let quality_path = PathBuf::from(QUALITY_CONFIG_SCHEMA_DEFAULT_PATH);
            write_schema(&scorecard_path)
                .map_err(|e| format!("failed to write {}: {e}", scorecard_path.display()))?;
            write_quality_config_schema(&quality_path)
                .map_err(|e| format!("failed to write {}: {e}", quality_path.display()))?;
        }
    }
    Ok(())
}

const USAGE: &str = "usage: emit-schema [--target scorecard|quality|both] [--out <path>]\n\
     \n\
     Default `--target both` writes the wire schema to .config/scorecard/schema.json\n\
     and the operator schema to .config/scorecard/quality.config.schema.json.\n\
     `--out` overrides the destination for single-target invocations only.";

/// Entry-point used by `bin/emit-schema.rs`. Drives [`parse_args`] and
/// the per-target writers, handling output framing and exit codes:
/// - `0` (`SUCCESS`) — schema(s) written, or `--help` printed.
/// - `1` — I/O failure during write.
/// - `2` — invalid arguments.
pub fn main_entry<I>(args: I) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    match parse_args(args) {
        Ok(ParsedArgs::Run { target, out }) => match emit(target, out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("emit-schema: {msg}");
                ExitCode::from(1)
            }
        },
        Ok(ParsedArgs::Help) => {
            // GNU-style: --help is a success invocation, output goes to
            // stdout so users can pipe `emit-schema --help | less`.
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("emit-schema: {msg}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv<const N: usize>(args: [&str; N]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn parse_args_returns_run_with_explicit_target_and_out() {
        let parsed =
            parse_args(argv(["--target", "scorecard", "--out", "/tmp/schema.json"])).unwrap();
        assert_eq!(
            parsed,
            ParsedArgs::Run {
                target: Target::Scorecard,
                out: Some(PathBuf::from("/tmp/schema.json")),
            }
        );
    }

    #[test]
    fn parse_args_defaults_to_target_both_when_no_flags() {
        let parsed = parse_args(Vec::<OsString>::new()).unwrap();
        assert_eq!(
            parsed,
            ParsedArgs::Run {
                target: Target::Both,
                out: None,
            }
        );
    }

    #[test]
    fn parse_args_back_compat_out_alone_implies_target_scorecard() {
        // Single-artifact shorthand: `--out <PATH>` without `--target`
        // writes the wire schema only. Operators and CI invocations
        // that name a single output path get the wire schema there;
        // dropping `--target` is the default-both behavior.
        let parsed = parse_args(argv(["--out", "/tmp/schema.json"])).unwrap();
        assert_eq!(
            parsed,
            ParsedArgs::Run {
                target: Target::Scorecard,
                out: Some(PathBuf::from("/tmp/schema.json")),
            }
        );
    }

    #[test]
    fn parse_args_accepts_target_quality() {
        let parsed = parse_args(argv(["--target", "quality"])).unwrap();
        assert_eq!(
            parsed,
            ParsedArgs::Run {
                target: Target::Quality,
                out: None,
            }
        );
    }

    #[test]
    fn parse_args_rejects_target_both_with_out() {
        let err = parse_args(argv(["--target", "both", "--out", "/tmp/x.json"])).unwrap_err();
        assert!(err.contains("--out cannot be combined"), "got: {err}");
    }

    #[test]
    fn parse_args_rejects_unknown_target_value() {
        let err = parse_args(argv(["--target", "bogus"])).unwrap_err();
        assert!(err.contains("--target must be one of"), "got: {err}");
        assert!(err.contains("bogus"), "got: {err}");
    }

    #[test]
    fn parse_args_returns_help_for_long_flag() {
        let parsed = parse_args(argv(["--help"])).unwrap();
        assert_eq!(parsed, ParsedArgs::Help);
    }

    #[test]
    fn parse_args_returns_help_for_short_flag() {
        let parsed = parse_args(argv(["-h"])).unwrap();
        assert_eq!(parsed, ParsedArgs::Help);
    }

    #[test]
    fn parse_args_rejects_missing_out_value() {
        let err = parse_args(argv(["--out"])).unwrap_err();
        assert!(err.contains("--out requires a path"), "got: {err}");
    }

    #[test]
    fn parse_args_rejects_missing_target_value() {
        let err = parse_args(argv(["--target"])).unwrap_err();
        assert!(err.contains("--target requires a value"), "got: {err}");
    }

    #[test]
    fn parse_args_rejects_unknown_arg() {
        let err = parse_args(argv(["--bogus"])).unwrap_err();
        assert!(err.contains("unknown argument"), "got: {err}");
        assert!(err.contains("--bogus"), "got: {err}");
    }

    #[test]
    fn render_schema_string_ends_with_newline() {
        let s = render_schema_string();
        assert!(s.ends_with('\n'));
        assert!(s.contains("\"Scorecard\""));
    }

    #[test]
    fn render_quality_config_schema_string_describes_threshold_config() {
        let s = render_quality_config_schema_string();
        assert!(s.ends_with('\n'));
        assert!(s.contains("ThresholdConfig"));
        assert!(s.contains("warn_pp_delta"));
        assert!(s.contains("fail_pp_delta"));
    }

    /// Regression test for the operator-schema typo-rejection contract.
    ///
    /// `serde(deny_unknown_fields)` on every `ThresholdConfig` /
    /// `RowsConfig` / `CoverageThresholds` / `BddSkipThresholds` struct
    /// emits `additionalProperties: false` at every nesting level (root,
    /// `definitions/RowsConfig`, `definitions/CoverageThresholds`,
    /// `definitions/BddSkipThresholds`). The scorecard-drift CI step
    /// validates the committed `quality.toml` against this schema via
    /// `ajv-cli`; if a future schema-postprocess or schemars upgrade
    /// ever weakens any level, an operator typo would silently slide
    /// past the gate and into the producer (which would then loud-fail
    /// at parse, but after CI has already gone green).
    ///
    /// Pinning to one occurrence per `[rows.*]` table + the two
    /// envelope structs (root + RowsConfig) keeps the gate's reach
    /// honest; bump the count when a new `[rows.*]` section lands.
    #[test]
    fn operator_schema_rejects_unknown_fields_at_every_nesting_level() {
        let s = render_quality_config_schema_string();
        let occurrences = s.matches("\"additionalProperties\": false").count();
        assert_eq!(
            occurrences, 8,
            "operator schema must declare additionalProperties:false at the root, \
             RowsConfig, CoverageThresholds, CoverageHandlerThresholds, \
             BddFeatureSkipThresholds, BddScenarioSkipThresholds, \
             CiWallClockThresholds, and FlakyPopulationThresholds — got {occurrences}. \
             Schema body:\n{s}",
        );
    }

    #[test]
    fn write_schema_creates_parent_dirs() {
        let dir = tempdir();
        let target = dir.path.join("nested/dir/schema.json");
        write_schema(&target).expect("write_schema");
        let on_disk = fs::read_to_string(&target).expect("read back");
        assert_eq!(on_disk, render_schema_string());
    }

    #[test]
    fn write_quality_config_schema_creates_parent_dirs() {
        let dir = tempdir();
        let target = dir.path.join("nested/dir/quality.config.schema.json");
        write_quality_config_schema(&target).expect("write_quality_config_schema");
        let on_disk = fs::read_to_string(&target).expect("read back");
        assert_eq!(on_disk, render_quality_config_schema_string());
    }

    #[test]
    fn main_entry_target_scorecard_with_explicit_out_writes_only_scorecard() {
        let dir = tempdir();
        let target = dir.path.join("schema.json");
        let code = main_entry(argv([
            "--target",
            "scorecard",
            "--out",
            target.to_str().unwrap(),
        ]));
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(target.exists());
        let body = fs::read_to_string(&target).unwrap();
        assert!(body.contains("\"Scorecard\""));
        // No quality schema landed under the test dir.
        assert!(
            !dir.path.join("quality.config.schema.json").exists(),
            "quality schema should not be emitted with --target scorecard"
        );
    }

    #[test]
    fn main_entry_target_quality_with_explicit_out_writes_only_quality() {
        let dir = tempdir();
        let target = dir.path.join("quality.config.schema.json");
        let code = main_entry(argv([
            "--target",
            "quality",
            "--out",
            target.to_str().unwrap(),
        ]));
        assert_eq!(code, ExitCode::SUCCESS);
        let body = fs::read_to_string(&target).unwrap();
        assert!(body.contains("ThresholdConfig"));
        assert!(body.contains("warn_pp_delta"));
    }

    #[test]
    fn main_entry_help_returns_success() {
        let code = main_entry(argv(["--help"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn main_entry_bad_args_returns_two() {
        let code = main_entry(argv(["--bogus"]));
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn main_entry_target_both_with_out_returns_two() {
        let code = main_entry(argv(["--target", "both", "--out", "/tmp/x.json"]));
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn main_entry_unknown_target_returns_two() {
        let code = main_entry(argv(["--target", "bogus"]));
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn main_entry_write_failure_returns_one() {
        // Use a path whose parent component is an existing *file*, so
        // create_dir_all fails. /etc/hosts is present on macOS + Linux
        // and the test process cannot create children under it.
        let target = PathBuf::from("/etc/hosts/schema.json");
        let code = main_entry(argv([
            "--target",
            "scorecard",
            "--out",
            target.to_str().unwrap(),
        ]));
        assert_eq!(code, ExitCode::from(1));
    }

    /// Minimal scoped tempdir without pulling in a dev-dep. The directory
    /// is deleted on `Drop`; tests that fail mid-execution leak the dir,
    /// which is acceptable for unit tests that touch only `/tmp`.
    struct TmpDir {
        path: PathBuf,
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn tempdir() -> TmpDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let pid = std::process::id();
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("scorecard-emit-{pid}-{nonce}"));
        fs::create_dir_all(&path).expect("create tempdir");
        TmpDir { path }
    }
}
