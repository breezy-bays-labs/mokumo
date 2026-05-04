//! `adr-validate` — resolve every ADR `enforced-by:` reference to a real
//! workspace artifact (file, workflow, lint script). The bin is a thin
//! shim around [`docs_gen::validate`] so coverage credit lands on the
//! library code.
//!
//! Designed to be called from `lefthook` and from local dev shells; the
//! CI gate (`adr-registry` in `quality.yml`) is intentionally
//! syntactic-only and does not invoke this binary.
//!
//! Exits 0 on success, 1 on any unresolved reference, 2 on parse error.

use docs_gen::validate::{HELP_TEXT, ParseOutcome, parse_args, run};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (args, outcome) = match parse_args(argv) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("adr-validate: {e:#}");
            std::process::exit(2);
        }
    };
    if outcome == ParseOutcome::ShowHelp {
        println!("{HELP_TEXT}");
        return;
    }
    let report = match run(&args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("adr-validate: {e:#}");
            std::process::exit(2);
        }
    };
    if report.ok() {
        eprintln!(
            "adr-validate: {} ADR(s) checked, all references resolve",
            report.checked
        );
        return;
    }
    eprintln!("adr-validate: unresolved references:");
    for f in &report.failures {
        eprintln!("  - {f}");
    }
    std::process::exit(1);
}
