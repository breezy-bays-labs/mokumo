//! JSONL row parsing for the BDD scenario-coverage capture stream.
//!
//! Wire shape produced by
//! `crates/mokumo-shop/tests/api_bdd_world/scenario_coverage.rs`:
//!
//! ```json
//! {"feature_path": "tests/api_features/customers.feature",
//!  "feature_title": "Customer CRUD",
//!  "scenario": "list returns all customers",
//!  "method": "GET",
//!  "matched_path": "/api/customers",
//!  "status": 200,
//!  "status_class": "happy"}
//! ```
//!
//! Parsing is permissive on extra fields (the captured side may add new
//! columns later) and strict on the seven required ones. A malformed row
//! is recorded in [`crate::scenario_coverage::artifact::Diagnostics::jsonl_errors`]
//! and the rest of the file continues to parse — one bad row should not
//! lose a whole run.

use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::artifact::JsonlError;

#[derive(Debug, Clone, Deserialize)]
pub struct Row {
    #[serde(default)]
    pub feature_path: String,
    #[serde(default)]
    pub feature_title: String,
    pub scenario: String,
    pub method: String,
    pub matched_path: String,
    #[allow(
        dead_code,
        reason = "kept on the wire for the markdown rendering's appendix; the gate consumes status_class only"
    )]
    pub status: u16,
    pub status_class: String,
}

#[derive(Debug, Default)]
pub struct ParseOutcome {
    pub rows: Vec<Row>,
    pub errors: Vec<JsonlError>,
    pub files_read: Vec<PathBuf>,
}

/// Read every `*.jsonl` file directly under `dir` (non-recursive — the
/// capture middleware writes to a flat directory). Returns parsed rows
/// plus per-row parse errors. A missing directory is treated as "no
/// rows captured": the gate then reports zero coverage for everything,
/// which is the loudest possible signal that capture didn't run.
pub fn read_dir(dir: &Path) -> ParseOutcome {
    let mut outcome = ParseOutcome::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return outcome;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    files.sort();
    for path in files {
        read_file_into(&path, &mut outcome);
        outcome.files_read.push(path);
    }
    outcome
}

fn read_file_into(path: &Path, outcome: &mut ParseOutcome) {
    let Some(reader) = open_reader(path, outcome) else {
        return;
    };
    for (idx, line_result) in reader.lines().enumerate() {
        let line_no = (idx as u64) + 1;
        process_line(path, line_no, line_result, outcome);
    }
}

fn open_reader(path: &Path, outcome: &mut ParseOutcome) -> Option<BufReader<File>> {
    match File::open(path) {
        Ok(f) => Some(BufReader::new(f)),
        Err(err) => {
            outcome.errors.push(JsonlError {
                file: path.to_string_lossy().into_owned(),
                line: 0,
                reason: format!("open: {err}"),
            });
            None
        }
    }
}

fn process_line(
    path: &Path,
    line_no: u64,
    line_result: std::io::Result<String>,
    outcome: &mut ParseOutcome,
) {
    let line = match line_result {
        Ok(s) => s,
        Err(err) => {
            outcome.errors.push(JsonlError {
                file: path.to_string_lossy().into_owned(),
                line: line_no,
                reason: format!("read: {err}"),
            });
            return;
        }
    };
    if line.trim().is_empty() {
        return;
    }
    match serde_json::from_str::<Row>(&line) {
        Ok(row) => outcome.rows.push(row),
        Err(err) => outcome.errors.push(JsonlError {
            file: path.to_string_lossy().into_owned(),
            line: line_no,
            reason: format!("parse: {err}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn reads_well_formed_rows() {
        let dir = tempdir().unwrap();
        let mut f = File::create(dir.path().join("api_bdd-1.jsonl")).unwrap();
        writeln!(
            f,
            r#"{{"feature_path":"f.feature","feature_title":"F","scenario":"s1","method":"GET","matched_path":"/api/x","status":200,"status_class":"happy"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"feature_path":"f.feature","feature_title":"F","scenario":"s2","method":"POST","matched_path":"/api/x","status":400,"status_class":"error_4xx"}}"#
        )
        .unwrap();
        drop(f);
        let outcome = read_dir(dir.path());
        assert_eq!(outcome.rows.len(), 2);
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.files_read.len(), 1);
    }

    #[test]
    fn keeps_good_rows_when_one_is_malformed() {
        let dir = tempdir().unwrap();
        let mut f = File::create(dir.path().join("api_bdd-1.jsonl")).unwrap();
        writeln!(f, r#"{{"not":"a row"}}"#).unwrap();
        writeln!(
            f,
            r#"{{"feature_path":"f.feature","feature_title":"F","scenario":"s","method":"GET","matched_path":"/api/x","status":200,"status_class":"happy"}}"#
        )
        .unwrap();
        drop(f);
        let outcome = read_dir(dir.path());
        assert_eq!(outcome.rows.len(), 1);
        assert_eq!(outcome.errors.len(), 1);
        assert_eq!(outcome.errors[0].line, 1);
    }

    #[test]
    fn missing_directory_returns_empty_outcome_silently() {
        let outcome = read_dir(Path::new("/nonexistent/directory/xyz"));
        assert!(outcome.rows.is_empty());
        assert!(outcome.errors.is_empty());
    }

    #[test]
    fn records_open_error_when_file_unreadable() {
        let mut outcome = ParseOutcome::default();
        // Path that doesn't exist on disk → File::open errors.
        read_file_into(Path::new("/definitely/not/a/real/path.jsonl"), &mut outcome);
        assert_eq!(outcome.errors.len(), 1);
        assert_eq!(outcome.errors[0].line, 0);
        assert!(outcome.errors[0].reason.starts_with("open:"));
        assert!(outcome.rows.is_empty());
    }

    #[test]
    fn records_parse_error_with_line_number() {
        let dir = tempdir().unwrap();
        let mut f = File::create(dir.path().join("bad.jsonl")).unwrap();
        writeln!(f, "{{not json").unwrap();
        drop(f);
        let outcome = read_dir(dir.path());
        assert_eq!(outcome.errors.len(), 1);
        assert_eq!(outcome.errors[0].line, 1);
        assert!(outcome.errors[0].reason.starts_with("parse:"));
    }

    #[test]
    fn ignores_non_jsonl_files() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("README.md")).unwrap();
        let outcome = read_dir(dir.path());
        assert!(outcome.rows.is_empty());
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.files_read.len(), 0);
    }

    #[test]
    fn skips_blank_lines() {
        let dir = tempdir().unwrap();
        let mut f = File::create(dir.path().join("api_bdd-1.jsonl")).unwrap();
        writeln!(f).unwrap();
        writeln!(
            f,
            r#"{{"feature_path":"f.feature","feature_title":"F","scenario":"s","method":"GET","matched_path":"/api/x","status":200,"status_class":"happy"}}"#
        )
        .unwrap();
        writeln!(f).unwrap();
        drop(f);
        let outcome = read_dir(dir.path());
        assert_eq!(outcome.rows.len(), 1);
        assert!(outcome.errors.is_empty());
    }
}
