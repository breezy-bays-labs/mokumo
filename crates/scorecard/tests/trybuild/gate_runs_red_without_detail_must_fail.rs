//! Layer 1 typestate: a Red `GateRuns` row without `failure_detail_md`
//! MUST fail to compile.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "gate_runs".into(),
        label: "Gates".into(),
        anchor: "gate-runs".into(), tool: "gate-runs".into(),
    };

    let _row = Row::gate_runs_red(common, Vec::new(), "0/12 gates failing".to_string());
}
