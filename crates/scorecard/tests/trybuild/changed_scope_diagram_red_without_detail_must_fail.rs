//! Layer 1 typestate: a Red `ChangedScopeDiagram` row without
//! `failure_detail_md` MUST fail to compile.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "changed_scope_diagram".into(),
        label: "Changed scope".into(),
        anchor: "changed-scope-diagram".into(),
        tool: "changed-scope".into(),
    };

    let _row =
        Row::changed_scope_diagram_red(common, "graph LR\n".to_string(), 0, "0 nodes".to_string());
}
