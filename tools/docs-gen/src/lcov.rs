//! lcov.info FNDA deduplication.
//!
//! `cargo-llvm-cov nextest` (without `--lib`) dual-compiles each source file:
//! once for the lib's `#[cfg(test)] mod tests`, once for each integration-test
//! binary that depends on it. The two builds emit FN/FNDA records with the
//! same `(file, line)` pair but build-hash-prefixed mangled names, so a
//! function with hits in the lib build and zero hits in the integration-test
//! build appears as two records — and downstream consumers like `crap4rs`
//! that key off function name pick the lower (zero-hit) record, manufacturing
//! false 0% coverage rows on the sticky scorecard.
//!
//! [`dedup`] collapses duplicate `FN`/`FNDA` records within each `SF:` block
//! by `(file, line)`, summing `FNDA` hits across build hashes. `FNF`/`FNH`
//! totals are recomputed from the deduplicated set; all other records (`DA`,
//! `BRDA`, branch totals, comments) pass through verbatim. Output is
//! deterministic — lines are sorted by source-line number — so consumers
//! that diff regenerated reports stay stable.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Deduplicates `FN`/`FNDA` records by `(file, line)` within each `SF:` block,
/// summing `FNDA` hits. Returns the rewritten lcov body.
pub fn dedup(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut block: Option<SfBlock> = None;
    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("SF:") {
            if let Some(b) = block.take() {
                emit_block(&mut output, &b);
            }
            block = Some(SfBlock::new(rest));
        } else if line == "end_of_record" {
            if let Some(b) = block.take() {
                emit_block(&mut output, &b);
            }
            output.push_str("end_of_record\n");
        } else if let Some(b) = block.as_mut() {
            b.absorb(line);
        } else {
            // Header or footer outside any SF block — pass through.
            output.push_str(line);
            output.push('\n');
        }
    }
    if let Some(b) = block.take() {
        emit_block(&mut output, &b);
    }
    output
}

struct SfBlock {
    sf_path: String,
    /// Line number → first-seen canonical FN name. BTreeMap so emit order is
    /// stable and matches source order (small line numbers first).
    fn_name_at: BTreeMap<u32, String>,
    /// FN name → declared line, for FNDA back-resolution. Names are
    /// build-hash-prefixed in the dual-compilation case, so the same line
    /// can have multiple distinct names — all map back to that line.
    line_of_name: BTreeMap<String, u32>,
    /// Line → summed FNDA hit count across every build hash that reported it.
    fnda_sum_at: BTreeMap<u32, u64>,
    /// All non-FN/FNDA/FNF/FNH body lines, preserved in input order.
    other: Vec<String>,
}

impl SfBlock {
    fn new(sf: &str) -> Self {
        Self {
            sf_path: sf.to_string(),
            fn_name_at: BTreeMap::new(),
            line_of_name: BTreeMap::new(),
            fnda_sum_at: BTreeMap::new(),
            other: Vec::new(),
        }
    }

    fn absorb(&mut self, line: &str) {
        if let Some(rest) = line.strip_prefix("FN:") {
            if let Some((ln, name)) = rest.split_once(',')
                && let Ok(n) = ln.parse::<u32>()
            {
                self.fn_name_at.entry(n).or_insert_with(|| name.to_string());
                self.line_of_name.insert(name.to_string(), n);
                return;
            }
            self.other.push(line.to_string());
        } else if let Some(rest) = line.strip_prefix("FNDA:") {
            if let Some((cnt, name)) = rest.split_once(',')
                && let Ok(c) = cnt.parse::<u64>()
                && let Some(&l) = self.line_of_name.get(name)
            {
                *self.fnda_sum_at.entry(l).or_insert(0) += c;
                return;
            }
            self.other.push(line.to_string());
        } else if line.starts_with("FNF:") || line.starts_with("FNH:") {
            // Drop — recomputed from the deduplicated FN/FNDA set at emit.
        } else {
            self.other.push(line.to_string());
        }
    }
}

fn emit_block(output: &mut String, block: &SfBlock) {
    let _ = writeln!(output, "SF:{}", block.sf_path);
    for (line, name) in &block.fn_name_at {
        let _ = writeln!(output, "FN:{line},{name}");
    }
    for (line, name) in &block.fn_name_at {
        let count = block.fnda_sum_at.get(line).copied().unwrap_or(0);
        let _ = writeln!(output, "FNDA:{count},{name}");
    }
    if !block.fn_name_at.is_empty() {
        let fnf = block.fn_name_at.len();
        let fnh = block
            .fn_name_at
            .keys()
            .filter(|l| block.fnda_sum_at.get(l).copied().unwrap_or(0) > 0)
            .count();
        let _ = writeln!(output, "FNF:{fnf}");
        let _ = writeln!(output, "FNH:{fnh}");
    }
    for o in &block.other {
        output.push_str(o);
        output.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_duplicate_fn_fnda_pairs_in_one_block() {
        // The dual-compilation case: same (file, line), two distinct
        // build-hash-prefixed names. Lib build saw 5 hits; integration-test
        // build saw 0. Pre-dedup, crap4rs picks the 0 — manufacturing a fake
        // 0% row. Post-dedup, the line shows 5 hits (sum).
        let input = "\
TN:ci
SF:src/foo.rs
FN:10,_RNvCs1abc_foo
FN:10,_RNvCs2def_foo
FNDA:5,_RNvCs1abc_foo
FNDA:0,_RNvCs2def_foo
DA:10,5
DA:11,5
end_of_record
";
        let output = dedup(input);
        let expected = "\
TN:ci
SF:src/foo.rs
FN:10,_RNvCs1abc_foo
FNDA:5,_RNvCs1abc_foo
FNF:1
FNH:1
DA:10,5
DA:11,5
end_of_record
";
        assert_eq!(output, expected);
    }

    #[test]
    fn dedupes_independently_across_multiple_sf_blocks() {
        let input = "\
SF:src/a.rs
FN:1,a_v1
FN:1,a_v2
FNDA:3,a_v1
FNDA:0,a_v2
end_of_record
SF:src/b.rs
FN:5,b_v1
FN:5,b_v2
FNDA:0,b_v1
FNDA:7,b_v2
end_of_record
";
        let output = dedup(input);
        // Each SF block deduped on its own. Total FNDA is the sum across
        // build hashes regardless of which one had the hits.
        assert!(output.contains("FNDA:3,a_v1"));
        assert!(output.contains("FNDA:7,b_v1"));
        assert!(!output.contains("FNDA:0,"));
    }

    #[test]
    fn fnh_reflects_summed_hits_not_per_record() {
        // Pre-dedup `crap4rs` would see two FNDA records (one zero-hit) and
        // mark the function uncovered. After dedup the line has nonzero
        // hits, so FNH must count it as hit.
        let input = "\
SF:src/x.rs
FN:1,name_a
FN:1,name_b
FNDA:0,name_a
FNDA:4,name_b
end_of_record
";
        let output = dedup(input);
        assert!(output.contains("FNF:1\n"));
        assert!(output.contains("FNH:1\n"));
        assert!(output.contains("FNDA:4,name_a"));
    }

    #[test]
    fn passes_through_da_brda_and_other_lines_untouched() {
        let input = "\
SF:src/a.rs
FN:1,f
FNDA:1,f
DA:1,1
DA:2,0
BRDA:1,0,0,1
BRDA:1,0,1,0
BRF:2
BRH:1
LF:2
LH:1
end_of_record
";
        let output = dedup(input);
        assert!(output.contains("DA:1,1\n"));
        assert!(output.contains("DA:2,0\n"));
        assert!(output.contains("BRDA:1,0,0,1\n"));
        assert!(output.contains("BRF:2\n"));
        assert!(output.contains("LF:2\n"));
    }

    #[test]
    fn empty_sf_block_emits_no_fnf_fnh_pair() {
        // FNF/FNH are conventionally absent when no FN records are present.
        // Don't synthesise zero values — the absence is meaningful.
        let input = "\
SF:src/empty.rs
DA:1,1
end_of_record
";
        let output = dedup(input);
        assert!(!output.contains("FNF:"));
        assert!(!output.contains("FNH:"));
        assert!(output.contains("DA:1,1"));
    }

    #[test]
    fn header_lines_outside_any_sf_block_pass_through() {
        let input = "\
TN:test_run
SF:src/a.rs
FN:1,f
FNDA:1,f
end_of_record
";
        let output = dedup(input);
        assert!(output.starts_with("TN:test_run\n"));
    }

    #[test]
    fn malformed_fn_record_preserved_verbatim() {
        // If upstream emits something we don't recognise, prefer pass-through
        // over silent drop — the operator can diagnose by reading the file.
        let input = "\
SF:src/a.rs
FN:not-a-number,bogus
FN:1,real
FNDA:2,real
end_of_record
";
        let output = dedup(input);
        assert!(output.contains("FN:not-a-number,bogus"));
        assert!(output.contains("FN:1,real"));
        assert!(output.contains("FNDA:2,real"));
    }

    #[test]
    fn fnda_for_unknown_name_preserved_verbatim() {
        let input = "\
SF:src/a.rs
FN:1,real
FNDA:5,real
FNDA:9,never_declared
end_of_record
";
        let output = dedup(input);
        assert!(output.contains("FNDA:5,real"));
        assert!(output.contains("FNDA:9,never_declared"));
    }

    #[test]
    fn idempotent_on_already_deduped_input() {
        let input = "\
SF:src/a.rs
FN:1,f
FNDA:5,f
FNF:1
FNH:1
DA:1,5
end_of_record
";
        let output = dedup(input);
        // Running through dedup a second time must not change it — the only
        // tolerated change is FNF/FNH recomputation, which already matches.
        let twice = dedup(&output);
        assert_eq!(output, twice);
    }

    #[test]
    fn distinct_lines_preserved_with_independent_fnda_sums() {
        let input = "\
SF:src/a.rs
FN:1,fn1_lib
FN:1,fn1_int
FN:20,fn2_lib
FN:20,fn2_int
FNDA:3,fn1_lib
FNDA:0,fn1_int
FNDA:0,fn2_lib
FNDA:7,fn2_int
end_of_record
";
        let output = dedup(input);
        assert!(output.contains("FN:1,fn1_lib"));
        assert!(output.contains("FN:20,fn2_lib"));
        assert!(output.contains("FNDA:3,fn1_lib"));
        assert!(output.contains("FNDA:7,fn2_lib"));
        assert!(output.contains("FNF:2"));
        assert!(output.contains("FNH:2"));
    }
}
