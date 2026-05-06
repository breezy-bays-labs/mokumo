//! Layer 1 typestate: a Yellow row constructs with optional
//! `failure_detail_md`. This file must compile cleanly.

use scorecard::{Breakouts, Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "coverage".into(),
        label: "Coverage".into(),
        anchor: "coverage".into(),
        tool: "coverage-rust".into(),
    };

    let _row = Row::coverage_delta_yellow(
        common,
        -0.6,
        "-0.6 pp".to_string(),
        Breakouts::default(),
        None,
    );
}
