//! In-process drift check: regenerate the schema using
//! [`scorecard::schema_postprocess::inject_red_requires_detail`] (the
//! same helper the `emit-schema` binary uses) and assert byte-identity
//! against the committed `.config/scorecard/schema.json`.
//!
//! This test is the regression gate for the producer→renderer trust
//! boundary. Any change that breaks the Layer 2 if/then injection or
//! changes the wire format without regenerating the committed schema
//! will fail this test before merge.

use schemars::schema_for;
use scorecard::{
    Scorecard,
    schema_postprocess::{inject_red_requires_detail, tighten_url_fields},
};

const COMMITTED_SCHEMA: &str = include_str!("../../../.config/scorecard/schema.json");

#[test]
fn committed_schema_matches_regenerated_output() {
    let regenerated = regenerate_schema();
    if regenerated != COMMITTED_SCHEMA {
        panic!(
            "scorecard schema drift detected.\n\
             The committed `.config/scorecard/schema.json` does not match \
             the schema regenerated from the Rust source.\n\n\
             To fix: \n\
             \tcargo run -p scorecard --bin emit-schema -- --out .config/scorecard/schema.json\n\n\
             First diff:\n{}",
            first_diff(&regenerated, COMMITTED_SCHEMA)
        );
    }
}

fn regenerate_schema() -> String {
    let mut schema = schema_for!(Scorecard);
    inject_red_requires_detail(&mut schema);
    tighten_url_fields(&mut schema);
    let mut content = serde_json::to_string_pretty(&schema).expect("serialize schema");
    content.push('\n');
    content
}

fn first_diff(a: &str, b: &str) -> String {
    for (idx, (line_a, line_b)) in a.lines().zip(b.lines()).enumerate() {
        if line_a != line_b {
            return format!(
                "  line {n}:\n    regenerated: {line_a}\n    committed:   {line_b}",
                n = idx + 1
            );
        }
    }
    if a.lines().count() != b.lines().count() {
        return format!(
            "  files have different line counts (regenerated: {}, committed: {})",
            a.lines().count(),
            b.lines().count()
        );
    }
    "  (no per-line diff; check trailing whitespace or newline)".into()
}
