//! `emit-schema` entry-point logic, split out of `bin/emit-schema.rs` so
//! the argument parser, the schema serializer, and the file writer are
//! reachable as plain library functions and exercised by unit tests.
//!
//! The bin target is a one-line wrapper around [`main_entry`]; everything
//! testable lives here.

#![doc(hidden)]

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use schemars::schema_for;

use crate::Scorecard;
use crate::schema_postprocess::inject_red_requires_detail;

/// Outcome of [`parse_args`] — either a path to write the schema to, or a
/// `--help` request.
#[derive(Debug, PartialEq, Eq)]
pub enum ParsedArgs {
    Run(PathBuf),
    Help,
}

/// Parse `--out <path>` / `--help` / `-h` from an iterator of raw OS args
/// (the caller passes `std::env::args_os().skip(1)`).
///
/// Returned errors carry the exact human-readable message printed by the
/// binary, so callers can `eprintln!` them directly.
pub fn parse_args<I>(args: I) -> Result<ParsedArgs, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut iter = args.into_iter();
    let mut out: Option<PathBuf> = None;
    while let Some(arg) = iter.next() {
        match arg.to_string_lossy().as_ref() {
            "--out" => {
                let v = iter
                    .next()
                    .ok_or_else(|| "--out requires a path".to_string())?;
                out = Some(PathBuf::from(v));
            }
            "--help" | "-h" => return Ok(ParsedArgs::Help),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    out.map(ParsedArgs::Run)
        .ok_or_else(|| "--out is required".to_string())
}

/// Render the post-processed JSON Schema for [`Scorecard`] as a UTF-8
/// string. Trailing newline included so the committed file is POSIX-clean
/// (most editors and `git diff` flag missing trailing newlines).
pub fn render_schema_string() -> String {
    let mut schema = schema_for!(Scorecard);
    inject_red_requires_detail(&mut schema);
    let mut content = serde_json::to_string_pretty(&schema)
        .expect("scorecard schema serializes to a JSON string");
    content.push('\n');
    content
}

/// Write the rendered schema to `out_path`, creating parent directories
/// as needed. Surfaces I/O errors to the caller for reporting.
pub fn write_schema(out_path: &Path) -> io::Result<()> {
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_path, render_schema_string())
}

/// Entry-point used by `bin/emit-schema.rs`. Drives [`parse_args`] and
/// [`write_schema`], handling output framing and exit codes:
/// - `0` (`SUCCESS`) — schema written, or `--help` printed.
/// - `1` — I/O failure during write.
/// - `2` — invalid arguments.
pub fn main_entry<I>(args: I) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    match parse_args(args) {
        Ok(ParsedArgs::Run(out_path)) => match write_schema(&out_path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("emit-schema: failed to write {}: {e}", out_path.display());
                ExitCode::from(1)
            }
        },
        Ok(ParsedArgs::Help) => {
            // GNU-style: --help is a success invocation, output goes to
            // stdout so users can pipe `emit-schema --help | less`.
            println!("usage: emit-schema --out <path>");
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("emit-schema: {msg}");
            eprintln!("usage: emit-schema --out <path>");
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
    fn parse_args_returns_run_for_valid_out() {
        let parsed = parse_args(argv(["--out", "/tmp/schema.json"])).unwrap();
        assert_eq!(parsed, ParsedArgs::Run(PathBuf::from("/tmp/schema.json")));
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
    fn parse_args_rejects_unknown_arg() {
        let err = parse_args(argv(["--bogus"])).unwrap_err();
        assert!(err.contains("unknown argument"), "got: {err}");
        assert!(err.contains("--bogus"), "got: {err}");
    }

    #[test]
    fn parse_args_requires_out() {
        let err = parse_args(Vec::<OsString>::new()).unwrap_err();
        assert!(err.contains("--out is required"), "got: {err}");
    }

    #[test]
    fn render_schema_string_ends_with_newline() {
        let s = render_schema_string();
        assert!(s.ends_with('\n'));
        assert!(s.contains("\"Scorecard\""));
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
    fn main_entry_run_writes_file_and_returns_success() {
        let dir = tempdir();
        let target = dir.path.join("schema.json");
        let code = main_entry(argv(["--out", target.to_str().unwrap()]));
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(target.exists());
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
    fn main_entry_write_failure_returns_one() {
        // Use a path whose parent component is an existing *file*, so
        // create_dir_all fails. /etc/hosts is present on macOS + Linux
        // and the test process cannot create children under it.
        let target = PathBuf::from("/etc/hosts/schema.json");
        let code = main_entry(argv(["--out", target.to_str().unwrap()]));
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
