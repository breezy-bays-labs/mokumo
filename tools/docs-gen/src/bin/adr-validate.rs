//! `adr-validate` — resolve every ADR `enforced-by:` reference to a real
//! workspace artifact (file, workflow, lint script). The bin is a CC=1
//! shim around [`docs_gen::validate::execute`] so coverage credit lands on
//! the library code (CRAP gate measures bin code as 0% covered because
//! `cargo-llvm-cov nextest` only sees lib + integration tests).
//!
//! Designed to be called from `lefthook` and from local dev shells; the
//! CI gate (`adr-registry` in `quality.yml`) is intentionally
//! syntactic-only and does not invoke this binary.
//!
//! Exits 0 on success, 1 on any unresolved reference, 2 on parse error.

fn main() {
    std::process::exit(docs_gen::validate::execute(
        std::env::args().skip(1).collect(),
    ));
}
