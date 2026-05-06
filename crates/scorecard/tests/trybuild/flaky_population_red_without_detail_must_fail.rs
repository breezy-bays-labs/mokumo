//! Layer 1 typestate: a Red `FlakyPopulation` row without
//! `failure_detail_md` MUST fail to compile.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "flaky_population".into(),
        label: "Flaky tests".into(),
        anchor: "flaky-population".into(),
        tool: "flaky-scan".into(),
    };

    let _row = Row::flaky_population_red(common, 4, 0, "4 markers".to_string());
}
