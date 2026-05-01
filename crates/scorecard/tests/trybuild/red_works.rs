//! Layer 1 typestate: a Red row with a valid `failure_detail_md: String`
//! constructs cleanly. Compile-pass partner to the three
//! `red_*_must_fail.rs` cases — without it, a refactor that changes the
//! Red constructor's parameter type (e.g. to `&str` or `Cow<'_, str>`)
//! would be caught only in the negative direction.

use scorecard::{Row, RowCommon};

fn main() {
    let common = RowCommon {
        id: "coverage".into(),
        label: "Coverage".into(),
        anchor: "coverage".into(),
    };

    let _row = Row::coverage_delta_red(
        common,
        "-4.2 pp".to_string(),
        "coverage dropped 4.2% on crate kikan".to_string(),
    );
}
