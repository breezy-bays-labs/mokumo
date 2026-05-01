//! Layer 1 typestate: constructing a Red row without `failure_detail_md`
//! MUST fail to compile. This file calls `Row::coverage_delta_red` with
//! the `failure_detail_md` argument missing — rustc must reject it.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "coverage".into(),
        label: "Coverage".into(),
        anchor: "coverage".into(),
    };

    // Missing the required `failure_detail_md: String` argument — Layer 1
    // typestate makes this a compile-time error, not a runtime check.
    let _row = Row::coverage_delta_red(common, "-4.2 pp".to_string());
}
