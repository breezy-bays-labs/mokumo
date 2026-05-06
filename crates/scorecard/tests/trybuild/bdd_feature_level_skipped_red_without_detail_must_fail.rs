//! Layer 1 typestate: a Red `BddFeatureLevelSkipped` row without
//! `failure_detail_md` MUST fail to compile.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "bdd_feature_skip".into(),
        label: "WIP feature files".into(),
        anchor: "bdd-feature-skip".into(),
        tool: "bdd-lint".into(),
    };

    let _row = Row::bdd_feature_level_skipped_red(
        common,
        40,
        15,
        Vec::new(),
        Vec::new(),
        "15 / 40".to_string(),
    );
}
