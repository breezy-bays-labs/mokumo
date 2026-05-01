//! Layer 1 typestate: a Green row constructs without `failure_detail_md`.
//! This file must compile cleanly.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "coverage".into(),
        label: "Coverage".into(),
        anchor: "coverage".into(),
    };

    let _row = Row::coverage_delta_green(common, "+0.3 pp".to_string());
}
