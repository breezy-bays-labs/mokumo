//! Layer 1 typestate: a Red `CiWallClockDelta` row without
//! `failure_detail_md` MUST fail to compile.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "ci_wall_clock".into(),
        label: "CI wall-clock".into(),
        anchor: "ci-wall-clock".into(), tool: "gha-wall-clock".into(),
    };

    let _row = Row::ci_wall_clock_delta_red(common, 920.5, 92.0, "+92.0s".to_string());
}
