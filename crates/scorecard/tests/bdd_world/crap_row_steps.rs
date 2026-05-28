//! Step definitions for the CrapDelta row ingestion scenarios in
//! `tests/features/crap_row_ingestion.feature`.
//!
//! These steps assert producer-side behavior at the ingestion boundary
//! — `scorecard::aggregate::read_crap_row_json` reads an artifact
//! emitted by `crap4rs --format scorecard-row` and either yields a
//! parsed [`Row::CrapDelta`] or fails loud at the boundary. The Layer 1
//! typestate ctors (which Red branch goes through) and the Layer 2
//! schema validator (which the full envelope goes through) are
//! exercised by Rust integration tests in
//! `tests/layer2_e2e.rs::crap_row_json_*`; the BDD scenarios pin the
//! ingestion-flag contract (Model P: producer mints status; aggregator
//! trusts via the `RowCommon::tool` serde default).

use std::io::Write as _;

use cucumber::{given, then, when};
use scorecard::Row;
use scorecard::aggregate::read_crap_row_json;
use serde_json::json;

use super::ThresholdWorld;

// ───────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────

/// Write `body` to `<tmp>/crap-row.json` and stash the path on the
/// world. The tempdir lives on `ThresholdWorld::tmp` so it's dropped
/// at the end of the scenario.
fn write_crap_row_artifact(world: &mut ThresholdWorld, body: &[u8]) {
    let tmp = world
        .tmp
        .get_or_insert_with(|| tempfile::tempdir().expect("scenario tempdir must allocate"));
    let path = tmp.path().join("crap-row.json");
    let mut f = std::fs::File::create(&path).expect("must be able to create crap-row.json");
    f.write_all(body)
        .expect("must be able to write the crap-row.json body");
    world.crap_row_path = Some(path);
}

// ───────────────────────────────────────────────────────────────────
// Givens — write the artifact
// ───────────────────────────────────────────────────────────────────

#[given(
    expr = "a --crap-row-json artifact whose wire JSON omits the tool field and reports a Green CrapDelta of {string} with threshold {int}"
)]
async fn given_well_formed_crap_row_artifact(
    world: &mut ThresholdWorld,
    delta_text: String,
    threshold: u32,
) {
    // Wire format per Model P (crap4rs `docs/scorecard-row-contract.md`):
    // `tool` field deliberately omitted so the aggregator's
    // `RowCommon::tool` serde default stamps "crap4rs".
    let body = json!({
        "type": "CrapDelta",
        "id": "crap_delta",
        "label": "CRAP Δ",
        "anchor": "crap-delta",
        "status": "Green",
        "threshold": threshold,
        "delta_count": 0,
        "delta_text": delta_text,
    });
    write_crap_row_artifact(world, body.to_string().as_bytes());
}

#[given(expr = "an empty --crap-row-json artifact")]
async fn given_empty_crap_row_artifact(world: &mut ThresholdWorld) {
    // The composite action emits an empty `outputs.row-json` when the
    // installed crap4rs lacks `--format scorecard-row` support
    // (graceful binstall-regression probe). Empty input must NOT
    // crash the aggregator — it falls through to the producer-pending
    // stub so a transient regression cannot block the merge queue.
    write_crap_row_artifact(world, b"");
}

#[given(
    expr = "a --crap-row-json artifact whose wire JSON contains a MutationSurvivors row instead of a CrapDelta row"
)]
async fn given_wrong_variant_crap_row_artifact(world: &mut ThresholdWorld) {
    // The flag is variant-specific. A non-CrapDelta row at this slot
    // means the upstream wired the wrong artifact; surface that loudly
    // instead of silently mis-stamping.
    let body = json!({
        "type": "MutationSurvivors",
        "id": "mutation_survivors",
        "label": "Mutation survivors",
        "anchor": "mutation-survivors",
        "status": "Green",
        "survivor_count": 0,
        "top_survivors": [],
        "delta_text": "0 survivors",
    });
    write_crap_row_artifact(world, body.to_string().as_bytes());
}

// ───────────────────────────────────────────────────────────────────
// When — invoke the ingestion fn
// ───────────────────────────────────────────────────────────────────

#[when(expr = "the aggregator reads the --crap-row-json artifact")]
async fn when_aggregator_reads_artifact(world: &mut ThresholdWorld) {
    let path = world
        .crap_row_path
        .clone()
        .expect("a Given step must have written the --crap-row-json artifact");
    world.crap_row_result = Some(read_crap_row_json(Some(path.as_path())));
}

// ───────────────────────────────────────────────────────────────────
// Thens — assert on the parsed Row / Ok(None) / Err shape
// ───────────────────────────────────────────────────────────────────

fn parsed_crap_delta(world: &ThresholdWorld) -> &Row {
    let result = world
        .crap_row_result
        .as_ref()
        .expect("a When step must have invoked read_crap_row_json");
    let row_opt = result
        .as_ref()
        .expect("expected Ok(_) from read_crap_row_json on a well-formed artifact");
    row_opt
        .as_ref()
        .expect("expected Some(Row) — the artifact body was non-empty")
}

#[then(expr = "the parsed CrapDelta row carries tool {string}")]
async fn then_parsed_row_carries_tool(world: &mut ThresholdWorld, expected_tool: String) {
    let Row::CrapDelta { common, .. } = parsed_crap_delta(world) else {
        panic!("expected Row::CrapDelta variant");
    };
    assert_eq!(
        common.tool, expected_tool,
        "the RowCommon::tool serde default must stamp `{expected_tool}` when the wire format omits the field",
    );
}

#[then(expr = "the parsed CrapDelta row carries status Green")]
async fn then_parsed_row_status_green(world: &mut ThresholdWorld) {
    let Row::CrapDelta { status, .. } = parsed_crap_delta(world) else {
        panic!("expected Row::CrapDelta variant");
    };
    assert_eq!(*status, scorecard::Status::Green);
}

#[then(expr = "the parsed CrapDelta row carries threshold {int}")]
async fn then_parsed_row_threshold(world: &mut ThresholdWorld, expected: u32) {
    let Row::CrapDelta { threshold, .. } = parsed_crap_delta(world) else {
        panic!("expected Row::CrapDelta variant");
    };
    assert_eq!(*threshold, expected);
}

#[then(expr = "the parsed CrapDelta row carries delta_count {int}")]
async fn then_parsed_row_delta_count(world: &mut ThresholdWorld, expected: i32) {
    let Row::CrapDelta { delta_count, .. } = parsed_crap_delta(world) else {
        panic!("expected Row::CrapDelta variant");
    };
    assert_eq!(*delta_count, expected);
}

#[then(
    expr = "the aggregator returns a soft-None so the caller falls through to the producer-pending stub"
)]
async fn then_aggregator_returns_ok_none(world: &mut ThresholdWorld) {
    let result = world
        .crap_row_result
        .as_ref()
        .expect("the When step must have invoked read_crap_row_json");
    let row_opt = result
        .as_ref()
        .expect("empty file must yield Ok(_), not Err — the binstall-regression probe is soft");
    assert!(
        row_opt.is_none(),
        "empty --crap-row-json artifact must yield Ok(None) so the caller falls through to the stub",
    );
}

#[then(expr = "the aggregator returns an error naming the expected CrapDelta variant")]
async fn then_aggregator_returns_err(world: &mut ThresholdWorld) {
    let result = world
        .crap_row_result
        .as_ref()
        .expect("the When step must have invoked read_crap_row_json");
    let err = result
        .as_ref()
        .expect_err("wrong-variant artifact must surface Err at the ingestion boundary");
    assert!(
        err.contains("CrapDelta"),
        "the error must name the expected CrapDelta variant so a CI failure points the operator at the wired-wrong-artifact bug — got: {err}",
    );
}
