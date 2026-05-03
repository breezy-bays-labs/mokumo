//! Layer 1 typestate: a Red `HandlerCoverageAxis` row without
//! `failure_detail_md` MUST fail to compile.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "handler_coverage_axis".into(),
        label: "Handler axes".into(),
        anchor: "handler-coverage-axis".into(),
    };

    let _row = Row::handler_coverage_axis_red(common, Vec::new(), "no axes covered".to_string());
}
