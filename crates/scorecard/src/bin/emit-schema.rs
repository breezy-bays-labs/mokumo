//! `emit-schema` — generate `.config/scorecard/schema.json` from the Rust
//! source of truth.
//!
//! This binary uses ONLY the lib's deps (serde + schemars + serde_json) so
//! it can run on the drift-check workflow without `--features cli`. Heavier
//! producer binaries are gated under the optional `cli` feature.
//!
//! Usage:
//!   emit-schema --out <path>
//!
//! All testable logic (argument parsing, schema rendering, file writing,
//! exit-code routing) lives in `scorecard::emit_schema`. The bin is a
//! one-line wrapper so its CC stays at 1 — there is nothing here to test.

fn main() -> std::process::ExitCode {
    scorecard::emit_schema::main_entry(std::env::args_os().skip(1))
}
