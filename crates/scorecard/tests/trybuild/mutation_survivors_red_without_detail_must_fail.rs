//! Layer 1 typestate: a Red `MutationSurvivors` row without
//! `failure_detail_md` MUST fail to compile.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "mutation_survivors".into(),
        label: "Mutation survivors".into(),
        anchor: "mutation-survivors".into(),
    };

    let _row = Row::mutation_survivors_red(common, 3, Vec::new(), "3 survivors".to_string());
}
