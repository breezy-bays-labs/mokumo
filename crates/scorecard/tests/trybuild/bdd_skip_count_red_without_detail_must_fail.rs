//! Layer 1 typestate: a Red `BddSkipCount` row without
//! `failure_detail_md` MUST fail to compile.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "bdd_skip_count".into(),
        label: "BDD scenarios".into(),
        anchor: "bdd-skip-count".into(),
    };

    let _row = Row::bdd_skip_count_red(common, 250, 12, Vec::new(), "12 skipped / 250".to_string());
}
