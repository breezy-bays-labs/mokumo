//! Layer 1 typestate: a Red `BddScenarioLevelSkipped` row without
//! `failure_detail_md` MUST fail to compile.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "bdd_scenario_skip".into(),
        label: "WIP scenarios".into(),
        anchor: "bdd-scenario-skip".into(), tool: "bdd-lint".into(),
    };

    let _row = Row::bdd_scenario_level_skipped_red(
        common,
        900,
        50,
        Vec::new(),
        Vec::new(),
        "50 / 900".to_string(),
    );
}
