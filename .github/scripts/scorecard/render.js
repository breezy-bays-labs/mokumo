// @ts-check
// Scorecard renderer + sticky-comment poster.
//
// Two surfaces:
//   - `renderScorecardMarkdown(scorecard)` builds the comment body
//     with status banner + 1-row table + the `<!-- ci-scorecard -->`
//     marker. Layer 3 defensive: if a Red row arrives without
//     `failure_detail_md`, render `(detail missing — see workflow logs)`
//     instead of throwing.
//   - `renderFailClosedMarkdown(prMeta, validationResult)` builds the
//     fail-closed body when the artifact fails schema validation.
//   - `postStickyComment({ octokit, owner, repo, prNumber, body })`
//     finds an existing comment containing the HTML marker and updates
//     it; otherwise creates a new one. No `marocchino`.
//
// Used by `.github/workflows/scorecard-comment.yml` via
// `actions/github-script`, and by vitest tests with a mocked octokit.

"use strict";

const STICKY_MARKER = "<!-- ci-scorecard -->";

/** Schema version this renderer is pinned to. Mirror of
 *  `scorecard::aggregate::SCHEMA_VERSION`. When an artifact reports a
 *  higher schema_version (a producer running ahead of the renderer
 *  rollout), the renderer prepends a degradation banner so reviewers
 *  know rows may be missing or rendered with stale logic.
 *
 *  Bumped in lockstep with the producer; the schema-drift CI step
 *  catches any divergence by validating the committed scorecard
 *  artifact against the embedded schema. */
const RENDERER_SCHEMA_VERSION = 2;

/** HTML marker the renderer emits when an artifact's schema_version
 *  exceeds the renderer's pinned version. Surfaced as an
 *  always-on-comment so operators can detect the drift even without
 *  reading the rendered Markdown. */
const FORWARD_COMPAT_MARKER = "<!-- forward-compat:degraded -->";

/** Visible italic preamble for forward-compat degradation. */
const FORWARD_COMPAT_PREAMBLE =
  "_Renderer pinned to an older schema_version than this artifact — some rows may render with stale logic. Update the renderer to catch up._";

// ── Fallback-threshold signals ─────────────────────────────────────────
//
// The renderer surfaces three byte-stable strings whenever the producer
// ran without an operator `quality.toml`. Both sides (this file and
// `crates/scorecard/src/threshold.rs`) declare these constants
// independently — a vitest snapshot in `__tests__/render.test.js`
// asserts byte-equality on the rendered markdown, and a cucumber-rs
// step-def asserts the Rust side stays in lockstep with the Gherkin
// literal in `crates/scorecard/tests/features/scorecard_display.feature`.
// If either constant changes, the matching test fails first.

/** HTML marker the renderer emits when fallback thresholds are active.
 *  Mirror of `scorecard::threshold::FALLBACK_MARKER`. */
const FALLBACK_MARKER = "<!-- fallback-thresholds:hardcoded -->";

/** Italic preamble the renderer prepends to the body when fallback
 *  thresholds are active. Mirror of
 *  `scorecard::threshold::STARTER_PREAMBLE`. */
const STARTER_PREAMBLE =
  "_Using starter-wheels fallback thresholds. Tune them in [`quality.toml`](QUALITY.md#threshold-tuning)._";

/** HTML comment pointing operators at the config path. Mirror of
 *  `scorecard::threshold::PATH_HINT_COMMENT`. */
const PATH_HINT_COMMENT =
  "<!-- tune at .config/scorecard/quality.toml — see QUALITY.md#threshold-tuning -->";

// ── Layer-3 stub fallback (producer-pending sentinel) ─────────────────
//
// Producers that have not yet shipped emit a Green stub row whose
// `delta_text` opens with [`PENDING_DELTA_PREFIX`] + the upstream issue
// reference. The renderer surfaces the row inline with a "⏳ pending"
// affordance and lets GitHub auto-link the issue ref (`crap4rs#111` →
// linked) so reviewers reach the upstream producer in one click.
//
// Mirror of `scorecard::aggregate::PENDING_TEXT_PREFIX`. Vitest pins
// byte-equality so a drift on either side fails first.

/** Prefix that flags a row's `delta_text` as a producer-pending stub.
 *  Mirror of `scorecard::aggregate::PENDING_TEXT_PREFIX`. */
const PENDING_DELTA_PREFIX = "(producer pending — see ";

/** Icon shown in the status column for a producer-pending stub row. */
const PENDING_ICON = "⏳";

/** Detect whether a row is a Layer-3 producer-pending stub.
 *
 *  @param {import("./types").Row} row
 *  @returns {boolean}
 */
function isPendingStubRow(row) {
  return (
    row.status === "Green" &&
    typeof row.delta_text === "string" &&
    row.delta_text.startsWith(PENDING_DELTA_PREFIX)
  );
}

/** @type {Record<import("./types").Status, string>} */
const STATUS_ICON = {
  Green: "🟢",
  Yellow: "🟡",
  Red: "🔴",
};

/** Stable map from scorecard row id → quality.yml job name (the
 *  Check Run name surfaced by the GitHub Check Runs API). Used by
 *  the two-click rule to resolve a row's status indicator to the
 *  specific Check Run that produced its verdict.
 *
 *  Entries are intentionally sparse: only rows that have a 1:1
 *  binding to a single Check Run job are listed. Composite rows
 *  (`flaky_population`, `changed_scope`, `ci_wall_clock`) and
 *  producer-pending stubs (`mutation_survivors`,
 *  `handler_coverage_axis`) fall through to the
 *  `all_check_runs_url` fallback — one click still gets the
 *  reviewer to the full Checks tab.
 *
 *  Drift detection is socially enforced: a new row id added to
 *  the producer without an entry here just renders with the
 *  fallback URL until the map is updated.
 */
const ROW_ID_TO_JOB_NAME = Object.freeze({
  coverage: "coverage-rust",
  crap_delta: "crap-delta",
  bdd_feature_skip: "bdd-lint",
  bdd_scenario_skip: "bdd-lint",
});

/** Resolve the Check Run URL for a row's status indicator.
 *
 *  The `Row::GateRuns` row carries its own `gate_runs[]` slice
 *  injected from the Check Runs API; its first entry is the worst-
 *  of gate, so we link directly to it.
 *
 *  Other rows resolve via `ROW_ID_TO_JOB_NAME` → match on
 *  `top_failures[].gate_name`. If the row's bound gate is not in
 *  `top_failures` (it passed, or the binding is absent), fall back
 *  to `all_check_runs_url` — the workflow's full Checks tab.
 *
 *  @param {import("./types").Row} row
 *  @param {import("./types").Scorecard} scorecard
 *  @returns {string}
 */
function getCheckRunUrlForRow(row, scorecard) {
  if (row.type === "GateRuns" && Array.isArray(row.gate_runs) && row.gate_runs.length > 0) {
    return row.gate_runs[0].url;
  }
  /** @type {string | undefined} */
  const jobName = ROW_ID_TO_JOB_NAME[/** @type {keyof typeof ROW_ID_TO_JOB_NAME} */ (row.id)];
  if (jobName !== undefined && Array.isArray(scorecard.top_failures)) {
    const hit = scorecard.top_failures.find((g) => g && g.gate_name === jobName);
    if (hit !== undefined) {
      return hit.url;
    }
  }
  return scorecard.all_check_runs_url;
}

/** Wire-format default for a row's `tool` field. Mirrors
 *  `scorecard::default_tool` so an artifact that pre-dates the
 *  `tool` field renders identically on both sides. */
const DEFAULT_TOOL = "crap4rs";

/** Render the tool slug as inline-monospace markdown. Falls back to
 *  the wire-format default when the field is absent on a legacy
 *  artifact (matches the producer-side `#[serde(default)]`).
 *
 *  @param {import("./types").Row} row
 *  @returns {string}
 */
function renderToolCell(row) {
  const tool = typeof row.tool === "string" && row.tool.length > 0 ? row.tool : DEFAULT_TOOL;
  return `\`${tool}\``;
}

/** Render a single row line as a markdown table row.
 *
 *  The status indicator is wrapped in a markdown link to the row's
 *  Check Run URL (or the workflow URL fallback) so a reviewer can
 *  reach the failing gate's logs in one additional click — the
 *  two-click rule. Pending stub rows keep the [`PENDING_ICON`] but
 *  still link to the workflow URL so the click is never a dead end.
 *
 *  The `Tool` column surfaces `RowCommon.tool` (the producer slug —
 *  e.g. `crap4rs`, `cargo-mutants`, `bdd-lint`) so reviewers can tell
 *  at a glance which upstream tool emitted the row when more than
 *  one producer contributes to the artifact.
 *
 *  @param {import("./types").Row} row
 *  @param {import("./types").Scorecard} scorecard
 *  @returns {string}
 */
function renderRow(row, scorecard) {
  const pending = isPendingStubRow(row);
  const icon = pending ? PENDING_ICON : STATUS_ICON[row.status] || "❔";
  const statusLabel = pending ? "Pending" : row.status;
  const label = row.label || row.id;
  const delta = row.delta_text || "";
  const url = getCheckRunUrlForRow(row, scorecard);
  const linkedIcon = `[${icon}](${url})`;
  const toolCell = renderToolCell(row);
  return `| ${linkedIcon} ${statusLabel} | ${label} | ${toolCell} | ${delta} |`;
}

/** Cosmetic thresholds the renderer uses to flag low-coverage handlers
 *  in the drill-down. Mirrors the defaults of `[rows.coverage_handler]`
 *  in `quality.toml` (warn 60.0, fail 40.0). Operators who tune those
 *  thresholds will see a slight visual misalignment with the gate's
 *  verdict — that's acceptable cosmetic drift; the gate's authoritative
 *  signal is the row status itself, not the icons in the drill-down.
 *
 *  Defined as constants rather than inlined so a future "render
 *  thresholds from artifact" upgrade has a single grep target.
 */
const HANDLER_FAIL_PCT = 40.0;
const HANDLER_WARN_PCT = 60.0;

/** Pick a small icon for one handler's branch-coverage % in the
 *  drill-down. Bracketed thresholds keep the math out of the rendering
 *  call site.
 *
 *  @param {number} pct
 *  @returns {string}
 */
function handlerCoverageIcon(pct) {
  if (typeof pct !== "number" || Number.isNaN(pct)) return "❔";
  if (pct <= HANDLER_FAIL_PCT) return "🔴";
  if (pct <= HANDLER_WARN_PCT) return "🟡";
  return "🟢";
}

/** Render the per-handler branch-coverage drill-down for a
 *  `Row::CoverageDelta` row. Two modes:
 *  - Handlers populated: collapsible `<details>` block with a per-crate
 *    sub-table sorted by ascending branch %, so the most-uncovered
 *    handlers float to the top of each crate's section.
 *  - Handlers absent (V4 default): small italic note pointing at the
 *    producer issue. The `<details>` form is intentionally avoided when
 *    there's no body — it would render as an empty disclosure.
 *
 *  Returns `""` for non-CoverageDelta rows so the caller can blanket-map
 *  over all rows without filtering.
 *
 *  @param {import("./types").Row} row
 *  @returns {string}
 */
function renderCoverageBreakouts(row) {
  if (row.type !== "CoverageDelta") return "";
  const breakouts = row.breakouts;
  const byCrate = breakouts && Array.isArray(breakouts.by_crate) ? breakouts.by_crate : [];
  const totalHandlers = byCrate.reduce(
    (n, c) => n + (Array.isArray(c.handlers) ? c.handlers.length : 0),
    0,
  );
  if (totalHandlers === 0) {
    return `\n> _Per-handler branch coverage: producer pending — see [#583](https://github.com/breezy-bays-labs/mokumo/issues/583)._\n`;
  }
  const crateBlocks = byCrate
    .filter((c) => Array.isArray(c.handlers) && c.handlers.length > 0)
    .map((c) => {
      const handlers = [...c.handlers].sort(
        (a, b) => a.branch_coverage_pct - b.branch_coverage_pct,
      );
      const tableRows = handlers
        .map(
          (h) =>
            `| ${handlerCoverageIcon(h.branch_coverage_pct)} | \`${h.handler}\` | ${h.branch_coverage_pct.toFixed(1)}% |`,
        )
        .join("\n");
      return [
        `**${c.crate_name}** — ${handlers.length} handler${handlers.length === 1 ? "" : "s"}`,
        "",
        "| | Handler | Branch coverage |",
        "| --- | --- | --- |",
        tableRows,
      ].join("\n");
    })
    .join("\n\n");
  const summary = `Per-handler branch coverage — ${totalHandlers} handler${totalHandlers === 1 ? "" : "s"} across ${byCrate.length} crate${byCrate.length === 1 ? "" : "s"}`;
  return `\n<details><summary>${summary}</summary>\n\n${crateBlocks}\n\n</details>\n`;
}

/** Render inline failure detail block for a Red row.
 *  Layer 3 defensive — if the producer somehow shipped a Red row without
 *  `failure_detail_md` (Layers 1 + 2 should have caught this), we render
 *  a placeholder pointing at the workflow logs rather than crashing.
 *
 *  @param {import("./types").Row} row
 *  @returns {string}
 */
function renderFailureDetail(row) {
  if (row.status !== "Red") return "";
  const detail = row.failure_detail_md;
  if (typeof detail !== "string" || detail.length === 0) {
    // Layer 3 defensive: producer + schema validator should have
    // already rejected this. Log loudly so operators see the breach
    // even when the comment renders cleanly.
    // eslint-disable-next-line no-console
    console.warn(
      `[scorecard] Red row '${row.label || row.id}' missing failure_detail_md — falling back to placeholder. Layers 1+2 should have prevented this.`,
    );
    return `\n> **${row.label || row.id}:** (detail missing — see workflow logs)\n`;
  }
  return `\n> **${row.label || row.id}:** ${detail}\n`;
}

/** Build the full sticky-comment body for a valid scorecard artifact.
 *
 *  When `scorecard.fallback_thresholds_active` is `true`, the body is
 *  framed with `STARTER_PREAMBLE` (visible italic line above the
 *  banner) and trailed by `FALLBACK_MARKER` + `PATH_HINT_COMMENT`
 *  (HTML comments after the row table) so operators can tell at a
 *  glance the verdict came from starter-wheel defaults rather than
 *  their tuned thresholds.
 *
 *  @param {import("./types").Scorecard} scorecard
 *  @returns {string}
 */
function renderScorecardMarkdown(scorecard) {
  const banner = `${STATUS_ICON[scorecard.overall_status] || "❔"} **CI status: ${scorecard.overall_status}**`;
  const rows = (scorecard.rows || []).map((row) => renderRow(row, scorecard)).join("\n");
  const detailBlocks = (scorecard.rows || [])
    .map((row) => `${renderCoverageBreakouts(row)}${renderFailureDetail(row)}`)
    .join("");
  const headerLine = `_PR #${scorecard.pr.pr_number} • head ${scorecard.pr.head_sha.slice(0, 7)}_`;

  const fallback = scorecard.fallback_thresholds_active === true;
  const forwardCompat =
    typeof scorecard.schema_version === "number" &&
    scorecard.schema_version > RENDERER_SCHEMA_VERSION;
  const lines = [STICKY_MARKER];
  if (forwardCompat) {
    // eslint-disable-next-line no-console
    console.warn(
      `[scorecard] artifact at schema_version=${scorecard.schema_version} — renderer pinned to ${RENDERER_SCHEMA_VERSION}; rendering with degradation notice.`,
    );
    lines.push(FORWARD_COMPAT_PREAMBLE, "");
  }
  if (fallback) {
    lines.push(STARTER_PREAMBLE, "");
  }
  lines.push(
    banner,
    "",
    headerLine,
    "",
    "| Status | Gate | Tool | Delta |",
    "| --- | --- | --- | --- |",
    rows,
    detailBlocks,
  );
  if (forwardCompat) {
    lines.push(FORWARD_COMPAT_MARKER);
  }
  if (fallback) {
    lines.push(FALLBACK_MARKER, PATH_HINT_COMMENT);
  }
  return lines.join("\n");
}

/** Build the fail-closed comment body when the artifact fails schema
 *  validation. Per the .feature spec: explains in plain prose, contains
 *  the JSON Pointer of the failing field, and contains the offending
 *  value.
 *
 *  @param {Pick<import("./types").PrMeta, "pr_number"> | null | undefined} prMeta
 *  @param {{ pointer: string; value: unknown; message: string }} result
 *  @returns {string}
 */
function renderFailClosedMarkdown(prMeta, result) {
  const valueRendered =
    result.value === undefined ? "(undefined)" : JSON.stringify(result.value);
  const prNumber = prMeta && prMeta.pr_number ? prMeta.pr_number : "?";
  return [
    STICKY_MARKER,
    "🔴 **CI scorecard failed to render**",
    "",
    `The scorecard artifact for PR #${prNumber} did not match the committed schema, so the renderer is failing closed: no green status will be shown until this is fixed.`,
    "",
    `- **JSON Pointer:** \`${result.pointer}\``,
    `- **Validator message:** ${result.message}`,
    `- **Offending value:** \`${valueRendered}\``,
    "",
    "See the workflow logs for the full validator output.",
  ].join("\n");
}

/** List PR comments and return the first marker-anchored sticky comment.
 *  Returns `undefined` if no sticky comment exists yet.
 *
 *  Marker matching is anchored to the start of the body (`startsWith`),
 *  not `includes`, so a user comment that *quotes* the marker text in
 *  prose cannot hijack the sticky slot.
 *
 *  @param {{
 *    octokit: any;
 *    owner: string;
 *    repo: string;
 *    prNumber: number;
 *    marker: string;
 *  }} args
 *  @returns {Promise<{ id: number; body?: string } | undefined>}
 */
async function findStickyComment({ octokit, owner, repo, prNumber, marker }) {
  // Pagination: PRs can accumulate >100 comments on long-lived branches.
  const comments = await octokit.paginate(octokit.rest.issues.listComments, {
    owner,
    repo,
    issue_number: prNumber,
    per_page: 100,
  });
  return comments.find(
    /** @param {{ body?: string }} c */ (c) => c.body && c.body.startsWith(marker),
  );
}

/** Sticky-comment poster. List comments on the PR, find one starting
 *  with the marker, update it; otherwise create a new comment.
 *
 *  `octokit` must be the `actions/github-script` octokit shape (or a
 *  test mock). The function is idempotent under the common case: a
 *  second invocation with the same marker hits `update`, never `create`.
 *
 *  Concurrency: between the initial list and the create call there is a
 *  TOCTOU window — two `scorecard-comment` runs racing for the same PR
 *  could both observe "no sticky" and both call `createComment`. We
 *  defend with a re-check immediately before create; if a sticky landed
 *  in the interim we route to update instead. The producer workflow
 *  (`Quality Loop`) sets `cancel-in-progress: true`, so racing upstream
 *  runs are rare in practice; this guard handles the edge case. The
 *  workflow also sets a per-branch `concurrency` group on the comment
 *  workflow itself so two scorecard-comment jobs never run in parallel
 *  for the same PR head. */
/**
 * @param {{
 *   octokit: any;
 *   owner: string;
 *   repo: string;
 *   prNumber: number;
 *   body: string;
 *   marker?: string;
 * }} args
 * @returns {Promise<{ action: "created" | "updated"; comment_id: number | undefined }>}
 */
async function postStickyComment({
  octokit,
  owner,
  repo,
  prNumber,
  body,
  marker = STICKY_MARKER,
}) {
  const existing = await findStickyComment({
    octokit,
    owner,
    repo,
    prNumber,
    marker,
  });
  if (existing) {
    await octokit.rest.issues.updateComment({
      owner,
      repo,
      comment_id: existing.id,
      body,
    });
    return { action: "updated", comment_id: existing.id };
  }

  // Re-check immediately before create: closes the read-then-write
  // window from the initial list. Cheap (one paginated list per
  // post) and bounds the duplicate-comment risk to one extra API call
  // landing in the same millisecond.
  const recheck = await findStickyComment({
    octokit,
    owner,
    repo,
    prNumber,
    marker,
  });
  if (recheck) {
    await octokit.rest.issues.updateComment({
      owner,
      repo,
      comment_id: recheck.id,
      body,
    });
    return { action: "updated", comment_id: recheck.id };
  }

  const created = await octokit.rest.issues.createComment({
    owner,
    repo,
    issue_number: prNumber,
    body,
  });
  return { action: "created", comment_id: created.data && created.data.id };
}

module.exports = {
  STICKY_MARKER,
  FALLBACK_MARKER,
  STARTER_PREAMBLE,
  PATH_HINT_COMMENT,
  PENDING_DELTA_PREFIX,
  PENDING_ICON,
  RENDERER_SCHEMA_VERSION,
  FORWARD_COMPAT_MARKER,
  FORWARD_COMPAT_PREAMBLE,
  ROW_ID_TO_JOB_NAME,
  HANDLER_FAIL_PCT,
  HANDLER_WARN_PCT,
  DEFAULT_TOOL,
  isPendingStubRow,
  getCheckRunUrlForRow,
  renderCoverageBreakouts,
  renderToolCell,
  renderScorecardMarkdown,
  renderFailClosedMarkdown,
  postStickyComment,
  findStickyComment,
};
