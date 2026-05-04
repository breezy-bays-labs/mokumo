Feature: Sticky PR scorecard display contract

  Every pull request gets a single sticky comment that summarizes the quality
  signals affected by the change. The comment is the developer's at-a-glance
  view of CI: status banner, an 8-row summary table, inline failure details
  for any red row, and direct links to the underlying gate Check Runs. This
  spec defines the behaviors the scorecard MUST exhibit — both the comment
  surface itself and the build-time invariants that protect it.

  # Canonical step-phrase vocabulary:
  #   - "the ci-scorecard comment" — the single sticky comment identified by
  #     the HTML marker <!-- ci-scorecard -->
  #   - "CI completes on <ref>" — the whole pipeline trigger
  #   - "the drift-check job runs" — single job within the pipeline
  #   - "the scorecard artifact" — the producer-emitted scorecard.json + pr-meta.json
  #
  # Out of scope (deferred — see ADR for current disposition):
  #   - PR rebase/squash-merge comment migration
  #   - Renderer retry under GitHub API rate-limit

  # --- Sticky comment lifecycle ---

  Rule: The scorecard appears as exactly one sticky comment per PR

    @future
    Scenario: A new PR receives one ci-scorecard comment after CI completes
      Given a developer opens a pull request
      And no ci-scorecard comment yet exists on the PR
      When CI completes on the head commit
      Then exactly one comment containing the HTML marker "<!-- ci-scorecard -->" appears on the PR
      And the comment is authored by the github-actions bot

    @future
    Scenario: Subsequent pushes update the ci-scorecard comment in place
      Given a pull request with an existing ci-scorecard comment
      And the developer has pushed three subsequent commits
      When CI completes on the third subsequent head commit
      Then the PR has exactly one comment containing the HTML marker "<!-- ci-scorecard -->"
      And the comment body contains the SHA of the third subsequent head commit

    @future
    Scenario: Reopening a closed PR continues to update the same comment
      Given a pull request was closed with an existing ci-scorecard comment
      When the developer reopens the PR and pushes a new commit
      And CI completes on the new head commit
      Then the PR still has exactly one comment containing the HTML marker "<!-- ci-scorecard -->"
      And the comment body contains the SHA of the new head commit

    @future
    Scenario: Draft PRs receive the ci-scorecard comment identically to ready PRs
      Given a developer opens a pull request as draft
      When CI completes on the head commit
      Then exactly one comment containing the HTML marker "<!-- ci-scorecard -->" appears on the PR

  # --- Status communication: what the developer sees ---

  Rule: The status banner and row icons accurately reflect the worst status across all gates

    @future
    Scenario: A PR that passes every gate shows a green banner and 8 green rows
      Given a developer opens a pull request
      When CI completes and every quality gate passes
      Then the ci-scorecard comment shows a green status banner
      And the 8-row summary table shows a green status icon on every row
      And no inline failure detail blocks are present

    @future
    Scenario: A PR with a mix of green, yellow, and red rows surfaces the red as the banner status
      Given a pull request where one gate fails, two gates regress, and the rest pass
      When CI completes
      Then the ci-scorecard comment shows a red status banner
      And the failing row appears with a red status icon and an inline failure detail
      And each regressing row appears with a yellow status icon
      And the passing rows appear with green status icons in the same summary table

  # --- Trust boundary: the renderer only renders valid artifacts ---

  Rule: A malformed scorecard artifact must surface as a fail-closed comment, not be silently dropped

    @future
    Scenario: A scorecard artifact that fails schema validation triggers fail-closed rendering
      Given the producer emits a scorecard artifact that does not match the committed schema
      When the renderer runs against that artifact
      Then the ci-scorecard comment is replaced with a fail-closed message
      And the comment explains in plain prose that the scorecard artifact was malformed
      And the comment body contains the JSON Pointer of the failing field
      And the comment body contains the offending value
      And the workflow run is marked as failed
      And no green status is shown to the developer

  # --- Build-time invariants: the committed artifacts must mirror the Rust source ---

  Rule: The committed schema and TypeScript types must match the Rust source of truth

    @future
    Scenario: Modifying the scorecard's Rust schema without regenerating committed artifacts fails CI
      Given a developer modifies the scorecard's Rust schema definition
      And the developer did not regenerate the committed schema and types artifacts
      When the drift-check job runs
      Then the drift-check job fails
      And the failure message names the regeneration command the developer must run

    @future
    Scenario: Modifying the scorecard's Rust schema and regenerating both committed artifacts passes CI
      Given a developer modifies the scorecard's Rust schema definition
      And the developer regenerated both the committed schema artifact and the committed types artifact
      When the drift-check job runs
      Then the drift-check job passes

  # --- Schema invariant: a Red row must carry an inline failure detail ---

  Rule: A row with status Red must show an inline human-readable failure detail (three layers of enforcement)

    @future
    Scenario: A Red row displays its failure detail inline below the summary row
      Given the producer emits a row with status Red
      And the row's failure_detail_md is "coverage dropped 4.2% on crate kikan"
      When the ci-scorecard comment is rendered
      Then the row appears in the summary table with a red status icon
      And the line "coverage dropped 4.2% on crate kikan" appears inline directly below the row

    @future
    Scenario: The committed schema rejects a Red row that omits failure_detail_md (Layer 2)
      Given a candidate scorecard artifact contains a row with status Red and no failure_detail_md
      When the renderer validates the artifact against the committed schema
      Then validation fails citing the failure_detail_md requirement on Red rows
      And the renderer posts a fail-closed comment naming the violation

    @future
    Scenario: The Rust typestate API forbids constructing a Red row without failure_detail_md at compile time (Layer 1)
      Given the scorecard crate's typestate Row API
      When code attempts to construct a Row with status Red and no failure_detail_md
      Then the construction does not compile
      And the compiler error names the missing failure_detail_md argument

  # --- Operator surface: thresholds are tunable without code changes ---

  Rule: Threshold tuning round-trips through quality.toml without code changes

    Scenario: An operator tightens a threshold and a row flips from green to yellow
      Given a row reports a coverage delta of -0.8 percentage points
      And the row is currently shown as green because the warn threshold is -1.0 percentage points
      When the operator edits quality.toml to tighten the warn threshold to -0.5 percentage points
      And CI completes again on the same head commit with no other changes
      Then the row is shown as yellow
      And no Rust source files were modified between the two CI runs

    # Test-split (V3 covers Yellow + marker via two layers):
    # - Producer behavior (`fallback_thresholds_active=true` + Yellow status) is
    #   asserted by step-defs in `tests/features/steps/threshold_steps.rs`.
    # - Renderer byte-equality (FALLBACK_MARKER + STARTER_PREAMBLE +
    #   PATH_HINT_COMMENT) is asserted by vitest snapshot tests in
    #   `.github/scripts/scorecard/__tests__/render.test.js`.
    # - Red branch (any new gate failure → red) is unit-tested in
    #   `crates/scorecard/src/threshold.rs::tests` via synthetic
    #   `resolve_coverage_delta(-7.5, &fallback().rows.coverage) == Status::Red`
    #   (council C5 — V3 has only the CoverageDelta row variant; absolute-
    #   coverage row variant lands V4 or later).
    Scenario: An empty quality.toml falls back to hardcoded thresholds with a visible marker
      Given quality.toml is empty or absent
      When CI completes
      Then any metric that regressed compared to the base branch is shown as yellow
      And any new gate failure is shown as red
      And the ci-scorecard comment contains the HTML marker "<!-- fallback-thresholds:hardcoded -->"
      And the comment opens with the italic preamble "_Using starter-wheels fallback thresholds. Tune them in [`quality.toml`](QUALITY.md#threshold-tuning)._"
      And the comment ends with the path-hint comment "<!-- tune at .config/scorecard/quality.toml — see QUALITY.md#threshold-tuning -->"
      And the comment displays a visible note that hardcoded fallback thresholds are in use

  # --- Forward compatibility: older renderers tolerate newer producers ---

  Rule: The renderer tolerates unknown row variants from future producer versions

    @future
    Scenario: A future producer emits a row variant the current renderer does not recognize
      Given the renderer recognizes the v0 row variants
      And the producer emits a scorecard at schema_version 1 containing one row of an unrecognized variant
      When the ci-scorecard comment is rendered
      Then the recognized rows are displayed normally in the summary table
      And the unrecognized row is replaced with a placeholder pointing the developer to the workflow logs
      And the workflow run does not fail
      And the workflow logs name the unrecognized row variant

    @future
    Scenario: A scorecard artifact at a higher schema_version is rendered with a degradation notice
      Given the renderer is built against schema_version 1
      And the producer emits a scorecard at schema_version 2
      When the ci-scorecard comment is rendered
      Then the comment displays a visible note that the scorecard was produced at a newer schema version
      And the comment renders the rows the renderer can recognize
      And the workflow run does not fail

  # --- Two-click rule: a failing gate is reachable from the sticky comment ---

  Rule: Every failing gate is reachable from the ci-scorecard comment in one click

    @future
    Scenario: The top failing gates are linked as Check Runs from the comment
      Given a pull request with five failing gates
      When the ci-scorecard comment is rendered
      Then the top three failing gates each appear as a Check Run link in the comment body
      And every Check Run link in the comment body is an absolute https:// URL pointing at github.com/{owner}/{repo}/runs/{check_run_id}
      And the comment also contains an "all checks" link to the full Check Runs list for the head commit

    Scenario: Fork-PR Check Run links resolve against the fork's head commit
      Given a pull request opened from a fork
      When CI completes and the ci-scorecard comment is posted
      Then each per-gate Check Run link in the comment resolves against the fork's head commit
      And the developer can navigate from the comment directly to each gate's Check Run page
