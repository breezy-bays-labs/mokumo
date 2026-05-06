//! Layer 1 typestate: bypassing the constructor surface by struct-literal
//! construction of `Row::CoverageDelta` MUST fail to compile from outside
//! the crate. The variant is marked `#[non_exhaustive]`, which makes
//! external struct-literal construction an E0639. Without this guard,
//! a caller could write `Row::CoverageDelta { status: Red, ...,
//! failure_detail_md: None }` and produce the exact wire-shape Layer 1
//! is supposed to forbid.

use scorecard::{Breakouts, Row, RowCommon, Status};

fn main() {
    let common = RowCommon {
        id: "coverage".into(),
        label: "Coverage".into(),
        anchor: "coverage".into(),
        tool: "coverage-rust".into(),
    };

    // External crate attempting to construct the variant directly.
    // rustc must reject with E0639 (cannot create non-exhaustive variant
    // outside its defining crate).
    let _row = Row::CoverageDelta {
        common,
        status: Status::Red,
        delta_pp: -4.2,
        delta_text: "-4.2 pp".to_string(),
        breakouts: Breakouts::default(),
        failure_detail_md: None,
    };
}
