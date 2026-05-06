//! Layer 1 typestate: a Red `CrapDelta` row without `failure_detail_md`
//! MUST fail to compile. Mirrors `red_without_detail_must_fail.rs` for
//! the V4 variant.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "crap_delta".into(),
        label: "CRAP Δ".into(),
        anchor: "crap-delta".into(),
        tool: "crap4rs".into(),
    };

    // Missing the required `failure_detail_md: String` argument.
    let _row = Row::crap_delta_red(common, 15, 2, "5 → 7 (+2)".to_string());
}
