Feature: CrapDelta row ingestion from crap4rs --format scorecard-row artifact

  The scorecard aggregator reads a `--crap-row-json` artifact emitted by
  `crap4rs --format scorecard-row` (Model P — producer mints status,
  aggregator trusts). The wire format deliberately omits the `tool`
  field; the aggregator stamps it via the `RowCommon::tool` serde default
  so cross-repo schema bumps stay decoupled. This spec pins both halves
  of that contract plus the empty-file graceful fallthrough that lets a
  binstall regression on the consuming composite action degrade to the
  producer-pending stub instead of failing CI hard.

  # Canonical step-phrase vocabulary:
  #   - "the --crap-row-json artifact at <path>" — a temp file the
  #     scenario writes representing the action's captured stdout from
  #     `crap4rs --format scorecard-row`
  #   - "the aggregator reads the --crap-row-json artifact" — the
  #     producer call site invokes `scorecard::aggregate::read_crap_row_json`
  #   - "the parsed CrapDelta row" — the `Row::CrapDelta` returned by
  #     `read_crap_row_json` when the wire JSON is well-formed
  #
  # Out of scope (covered by Rust integration tests in tests/layer2_e2e.rs):
  #   - Red row without failure_detail_md fail-loud (Model P contract)
  #   - Whitespace-only file fallthrough (sibling of empty-file branch)
  #   - Schema-validator (Layer 2) cross-checks on the full envelope

  Rule: A well-formed producer artifact deserializes via the RowCommon serde default

    Scenario: Producer wire JSON without the tool field is stamped "crap4rs"
      Given a --crap-row-json artifact whose wire JSON omits the tool field and reports a Green CrapDelta of "5 → 5 (no change)" with threshold 15
      When the aggregator reads the --crap-row-json artifact
      Then the parsed CrapDelta row carries tool "crap4rs"
      And the parsed CrapDelta row carries status Green
      And the parsed CrapDelta row carries threshold 15
      And the parsed CrapDelta row carries delta_count 0

  Rule: The empty-file graceful-probe path falls through to the producer-pending stub

    Scenario: An empty --crap-row-json artifact yields a soft-None for stub fallback
      Given an empty --crap-row-json artifact
      When the aggregator reads the --crap-row-json artifact
      Then the aggregator returns a soft-None so the caller falls through to the producer-pending stub

  Rule: A wrong-variant artifact fails loud at the ingestion boundary

    Scenario: A --crap-row-json artifact containing a non-CrapDelta variant is rejected
      Given a --crap-row-json artifact whose wire JSON contains a MutationSurvivors row instead of a CrapDelta row
      When the aggregator reads the --crap-row-json artifact
      Then the aggregator returns an error naming the expected CrapDelta variant
