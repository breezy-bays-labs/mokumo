//! Trybuild harness for Layer 1 typestate enforcement of the scorecard schema.
//!
//! These UI tests guard the invariant that a `Row` with `status: Red` cannot
//! be constructed without a `failure_detail_md: String` — at compile time,
//! not runtime. See `decisions/mokumo/adr-scorecard-crate-shape.md` (Layer
//! 1) and the .feature scenario "The Rust typestate API forbids constructing
//!    a Red row without failure_detail_md at compile time".
//!
//! Three angles are exercised:
//!
//! - **Arity** — calling `coverage_delta_red` with the failure_detail_md
//!   argument missing is rejected (E0061).
//! - **Type** — calling `coverage_delta_red` with `None` instead of a
//!   `String` is rejected (E0308); guards against signature drift to
//!   `Option<String>`.
//! - **Struct literal** — constructing `Row::CoverageDelta { ... }` directly
//!   from outside the crate is rejected because the variant is
//!   `#[non_exhaustive]` (E0639); guards against bypassing the constructor
//!   surface.
//!
//! Trybuild snapshots rustc's stderr by name, which drifts across toolchain
//! releases. The toolchain pin in `rust-toolchain.toml` (workspace root)
//! is the stability baseline — see this crate's README for the bump
//! runbook. Toolchain bumps are a synchronized change with the .stderr
//! files in `tests/trybuild/*.stderr`.

#[test]
fn trybuild_layer1_typestate() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/trybuild/red_without_detail_must_fail.rs");
    t.compile_fail("tests/trybuild/red_with_none_must_fail.rs");
    t.compile_fail("tests/trybuild/red_struct_literal_must_fail.rs");
    t.pass("tests/trybuild/green_works.rs");
    t.pass("tests/trybuild/yellow_works.rs");
    t.pass("tests/trybuild/red_works.rs");

    // Per-variant Layer-1 coverage for the V4 row inventory. Each new
    // Red ctor demands `failure_detail_md: String`; calling it without
    // the argument must be a compile error so the typestate guarantee
    // tracks every variant, not just `CoverageDelta`.
    t.compile_fail("tests/trybuild/crap_delta_red_without_detail_must_fail.rs");
    t.compile_fail("tests/trybuild/mutation_survivors_red_without_detail_must_fail.rs");
    t.compile_fail("tests/trybuild/bdd_skip_count_red_without_detail_must_fail.rs");
    t.compile_fail("tests/trybuild/gate_runs_red_without_detail_must_fail.rs");
    t.compile_fail("tests/trybuild/flaky_population_red_without_detail_must_fail.rs");
    t.compile_fail("tests/trybuild/ci_wall_clock_delta_red_without_detail_must_fail.rs");
    t.compile_fail("tests/trybuild/handler_coverage_axis_red_without_detail_must_fail.rs");
    t.compile_fail("tests/trybuild/changed_scope_diagram_red_without_detail_must_fail.rs");
}
