//! Layer 1 typestate: passing `None` (or any `Option<String>`) for
//! `failure_detail_md` MUST fail to compile. This guards against signature
//! drift: if `coverage_delta_red`'s third parameter ever weakens to
//! `Option<String>`, this test starts compiling and the typestate gate
//! trips. The arity-only test (`red_without_detail_must_fail.rs`) cannot
//! distinguish that drift on its own.

use scorecard::{Breakouts, Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "coverage".into(),
        label: "Coverage".into(),
        anchor: "coverage".into(), tool: "coverage-rust".into(),
    };

    // Fifth argument is `Option<String>::None`; the constructor takes
    // `String`. rustc must reject with E0308 (mismatched types).
    let _row = Row::coverage_delta_red(
        common,
        -4.2,
        "-4.2 pp".to_string(),
        Breakouts::default(),
        None,
    );
}
