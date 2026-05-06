//! Parse lcov files into a `(source_file, line) → hits` map.
//!
//! lcov fields consumed:
//! - `SF:<path>` — opens a record. Path may be absolute or relative.
//! - `DA:<line>,<hits>[,<checksum>]` — line-coverage entry.
//! - `end_of_record` — closes the record. Other fields (`FN:`, `FNDA:`,
//!   `BRDA:`, etc.) are ignored — we only need line-level hits for
//!   "any line in this item's span ≥ 1 hit".
//!
//! Tolerant of:
//! - Multiple records per source file (sum the hits).
//! - Path canonicalisation: paths are stored as-given; the producer
//!   normalizes relative-to-workspace before keying lookups.
//! - Existing `lcov-dedup` tool (mokumo#783): we don't need to call it
//!   first because we sum hits across duplicate records ourselves;
//!   but the producer respects the dedup output if it's been run.
//!
//! Out of scope: branch coverage, checksums, function-level entries.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct LcovIndex {
    /// `path → (line → hit-count)` summed across all input records.
    pub by_file: HashMap<PathBuf, HashMap<u32, u32>>,
}

impl LcovIndex {
    /// Sum of lines with hit ≥ 1 in `file` whose 1-based line falls in
    /// `[start..=end]`. Returns `(covered_lines, total_lines)`.
    #[must_use]
    pub fn span_coverage(&self, file: &Path, start: u32, end: u32) -> (u32, u32) {
        let total = end.saturating_sub(start).saturating_add(1);
        let Some(per_line) = self.by_file.get(file) else {
            return (0, total);
        };
        let mut covered = 0u32;
        for line in start..=end {
            if per_line.get(&line).copied().unwrap_or(0) > 0 {
                covered = covered.saturating_add(1);
            }
        }
        (covered, total)
    }
}

#[derive(Debug)]
pub struct LcovError {
    pub file: PathBuf,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct LoadOutcome {
    pub index: LcovIndex,
    pub errors: Vec<LcovError>,
    pub files_consumed: u64,
}

/// Load every lcov file in `paths` into a single index. Files that
/// can't be opened or parsed are recorded in `errors` and the rest
/// continue to load — one bad file shouldn't lose the whole run.
#[must_use]
pub fn load_files(paths: &[PathBuf]) -> LoadOutcome {
    let mut outcome = LoadOutcome::default();
    for p in paths {
        match parse_one(p) {
            Ok(records) => {
                merge_into(&mut outcome.index, records);
                outcome.files_consumed = outcome.files_consumed.saturating_add(1);
            }
            Err(reason) => outcome.errors.push(LcovError {
                file: p.clone(),
                reason,
            }),
        }
    }
    outcome
}

type FileRecord = (PathBuf, HashMap<u32, u32>);

/// Parse a single lcov file into a list of `(path, per-line-hits)`
/// records. Multi-record files emit one entry per `SF:` block.
fn parse_one(path: &Path) -> Result<Vec<FileRecord>, String> {
    let file = File::open(path).map_err(|e| format!("open: {e}"))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_lines: HashMap<u32, u32> = HashMap::new();
    for (idx, line_result) in reader.lines().enumerate() {
        let line_no = idx + 1;
        let line = line_result.map_err(|e| format!("read line {line_no}: {e}"))?;
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("SF:") {
            // Close any prior record (lcov allows multiple SF without
            // explicit end_of_record between them — be permissive).
            if let Some(p) = current_path.take() {
                records.push((p, std::mem::take(&mut current_lines)));
            }
            current_path = Some(PathBuf::from(rest));
        } else if let Some(rest) = trimmed.strip_prefix("DA:") {
            let (line_no, hits) = parse_da(rest, line_no)?;
            *current_lines.entry(line_no).or_insert(0) = current_lines
                .get(&line_no)
                .copied()
                .unwrap_or(0)
                .saturating_add(hits);
        } else if trimmed == "end_of_record"
            && let Some(p) = current_path.take()
        {
            records.push((p, std::mem::take(&mut current_lines)));
        }
        // Other fields are ignored — see module docs.
    }
    // Close any trailing record (file might end without end_of_record).
    if let Some(p) = current_path.take() {
        records.push((p, current_lines));
    }
    Ok(records)
}

fn parse_da(rest: &str, line_no: usize) -> Result<(u32, u32), String> {
    let mut parts = rest.split(',');
    let line_str = parts
        .next()
        .ok_or_else(|| format!("DA at lcov line {line_no}: empty value"))?;
    let hits_str = parts
        .next()
        .ok_or_else(|| format!("DA at lcov line {line_no}: missing hits"))?;
    let line: u32 = line_str
        .parse()
        .map_err(|e| format!("DA at lcov line {line_no}: bad line `{line_str}`: {e}"))?;
    let hits: u32 = hits_str
        .parse()
        .map_err(|e| format!("DA at lcov line {line_no}: bad hits `{hits_str}`: {e}"))?;
    Ok((line, hits))
}

fn merge_into(index: &mut LcovIndex, records: Vec<FileRecord>) {
    for (path, per_line) in records {
        let entry = index.by_file.entry(path).or_default();
        for (line, hits) in per_line {
            *entry.entry(line).or_insert(0) =
                entry.get(&line).copied().unwrap_or(0).saturating_add(hits);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_lcov(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        path
    }

    #[test]
    fn loads_simple_record() {
        let dir = tempdir().unwrap();
        let path = write_lcov(
            dir.path(),
            "a.lcov",
            "SF:src/foo.rs\nDA:10,5\nDA:11,0\nDA:12,3\nend_of_record\n",
        );
        let outcome = load_files(&[path]);
        assert!(outcome.errors.is_empty());
        let map = outcome.index.by_file.get(Path::new("src/foo.rs")).unwrap();
        assert_eq!(map.get(&10).copied(), Some(5));
        assert_eq!(map.get(&11).copied(), Some(0));
        assert_eq!(map.get(&12).copied(), Some(3));
    }

    #[test]
    fn span_coverage_counts_only_hit_lines() {
        let dir = tempdir().unwrap();
        let path = write_lcov(
            dir.path(),
            "a.lcov",
            "SF:src/foo.rs\nDA:10,5\nDA:11,0\nDA:12,3\nend_of_record\n",
        );
        let outcome = load_files(&[path]);
        let (cov, total) = outcome.index.span_coverage(Path::new("src/foo.rs"), 10, 12);
        assert_eq!(cov, 2);
        assert_eq!(total, 3);
    }

    #[test]
    fn span_coverage_unknown_file_is_zero_with_correct_total() {
        let outcome = LoadOutcome::default();
        let (cov, total) = outcome
            .index
            .span_coverage(Path::new("src/missing.rs"), 1, 5);
        assert_eq!(cov, 0);
        assert_eq!(total, 5);
    }

    #[test]
    fn merges_duplicate_records_for_same_file() {
        let dir = tempdir().unwrap();
        let path = write_lcov(
            dir.path(),
            "a.lcov",
            "SF:src/foo.rs\nDA:10,2\nend_of_record\nSF:src/foo.rs\nDA:10,3\nend_of_record\n",
        );
        let outcome = load_files(&[path]);
        let map = outcome.index.by_file.get(Path::new("src/foo.rs")).unwrap();
        assert_eq!(map.get(&10).copied(), Some(5));
    }

    #[test]
    fn merges_across_multiple_files() {
        let dir = tempdir().unwrap();
        let a = write_lcov(
            dir.path(),
            "a.lcov",
            "SF:src/foo.rs\nDA:10,1\nend_of_record\n",
        );
        let b = write_lcov(
            dir.path(),
            "b.lcov",
            "SF:src/foo.rs\nDA:11,1\nend_of_record\nSF:src/bar.rs\nDA:1,1\nend_of_record\n",
        );
        let outcome = load_files(&[a, b]);
        assert_eq!(outcome.files_consumed, 2);
        let foo = outcome.index.by_file.get(Path::new("src/foo.rs")).unwrap();
        assert_eq!(foo.len(), 2);
        assert!(outcome.index.by_file.contains_key(Path::new("src/bar.rs")));
    }

    #[test]
    fn missing_file_is_recorded_as_error() {
        let outcome = load_files(&[PathBuf::from("/no/such/file.lcov")]);
        assert_eq!(outcome.errors.len(), 1);
        assert!(outcome.errors[0].reason.starts_with("open:"));
    }

    #[test]
    fn malformed_da_is_recorded_as_error() {
        let dir = tempdir().unwrap();
        let path = write_lcov(
            dir.path(),
            "bad.lcov",
            "SF:src/foo.rs\nDA:abc,xyz\nend_of_record\n",
        );
        let outcome = load_files(&[path]);
        assert_eq!(outcome.errors.len(), 1);
        assert!(outcome.errors[0].reason.contains("DA"));
    }

    #[test]
    fn ignores_unrelated_fields() {
        let dir = tempdir().unwrap();
        let path = write_lcov(
            dir.path(),
            "a.lcov",
            "TN:test_run\nSF:src/foo.rs\nFN:5,foo\nFNDA:1,foo\nDA:5,1\nLF:1\nLH:1\nend_of_record\n",
        );
        let outcome = load_files(&[path]);
        assert!(outcome.errors.is_empty());
        let map = outcome.index.by_file.get(Path::new("src/foo.rs")).unwrap();
        assert_eq!(map.get(&5).copied(), Some(1));
    }

    #[test]
    fn multiple_sf_without_end_of_record_split_correctly() {
        // Some lcov producers omit `end_of_record` between back-to-back
        // SF entries. The parser must still produce two separate file
        // records, not a merged one with both filenames.
        let dir = tempdir().unwrap();
        let path = write_lcov(
            dir.path(),
            "a.lcov",
            "SF:src/a.rs\nDA:1,1\nSF:src/b.rs\nDA:2,2\n",
        );
        let outcome = load_files(&[path]);
        assert!(outcome.errors.is_empty());
        let a = outcome.index.by_file.get(Path::new("src/a.rs")).unwrap();
        let b = outcome.index.by_file.get(Path::new("src/b.rs")).unwrap();
        assert_eq!(a.get(&1).copied(), Some(1));
        assert_eq!(b.get(&2).copied(), Some(2));
    }
}
