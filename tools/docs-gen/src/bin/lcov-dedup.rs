//! `lcov-dedup` — collapse duplicate `FN`/`FNDA` records produced by
//! `cargo-llvm-cov nextest` dual-compilation, summing hits by `(file, line)`.
//! The bin is a CC=1 shim around [`docs_gen::lcov::dedup`] for the same
//! reason the `adr-validate` shim is — coverage credit lands on the library.
//!
//! Reads from `stdin` and writes to `stdout`. Exits 0 on success, 1 on I/O
//! failure. Intended to live between `cargo llvm-cov` and `crap4rs` in the
//! `crap` moon task; see `crates/mokumo-shop/moon.yml`.

use std::io::{self, Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut input = String::new();
    if let Err(err) = io::stdin().read_to_string(&mut input) {
        eprintln!("lcov-dedup: read stdin: {err}");
        return ExitCode::from(1);
    }
    let output = docs_gen::lcov::dedup(&input);
    if let Err(err) = io::stdout().write_all(output.as_bytes()) {
        eprintln!("lcov-dedup: write stdout: {err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
