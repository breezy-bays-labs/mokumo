// @ts-check
// Inject per-gate Check Run data into the scorecard envelope.
//
// Runs after the producer uploads `tmp/scorecard.json` and after the
// upstream Quality Loop completes. Pulls the full set of Check Runs
// for the PR head SHA, drops the rollup verdict by name match, sorts
// by worst-of severity, and writes the result back into the scorecard
// envelope:
//
//   - `top_failures`: top 3 failing Check Runs, populated from this
//     query for the renderer's "two-click rule" (each top failure
//     becomes a clickable status indicator on its row).
//   - A new `Row::GateRuns` row appended to `rows`: top 5 worst-of
//     Check Runs of any conclusion, including skipped/neutral, so the
//     row meaningfully reports "what gates ran and how badly".
//
// The producer (Rust aggregator) does NOT call the Check Runs API: it
// runs at producer-time, before all gates have settled. The poster
// runs `workflow_run.completed`, which is after Quality Loop concludes
// — gates are settled and `actions: read` covers the API surface. The
// producer/poster split routes producer output through the poster's
// trust boundary so untrusted PR data never reaches a write-capable
// runner.
//
// Pure function with `octokit` injected so vitest can mock cleanly;
// the workflow's `actions/github-script@v7` step is a thin wrapper
// that resolves env + invokes.

"use strict";

/**
 * @typedef {{ id: number; name: string; conclusion: string | null; html_url: string }} CheckRun
 * @typedef {"Green" | "Yellow" | "Red"} Status
 * @typedef {{ gate_name: string; run_id: number; url: string }} GateRun
 * @typedef {{
 *   type: string;
 *   id: string;
 *   label: string;
 *   anchor: string;
 *   status: Status;
 *   delta_text: string;
 *   gate_runs?: GateRun[];
 *   failure_detail_md?: string;
 *   [k: string]: unknown;
 * }} Row
 * @typedef {{
 *   schema_version: number;
 *   pr: { pr_number: number; head_sha: string; base_sha: string; is_fork: boolean };
 *   overall_status: Status;
 *   rows: Row[];
 *   top_failures: GateRun[];
 *   all_check_runs_url: string;
 *   fallback_thresholds_active: boolean;
 *   [k: string]: unknown;
 * }} Scorecard
 * @typedef {(args: { owner: string; repo: string; ref: string; per_page?: number }) => Promise<{ data: { check_runs: CheckRun[]; total_count?: number } }>} ListForRefFn
 * @typedef {{
 *   checks: { listForRef: ListForRefFn };
 *   paginate: (fn: ListForRefFn, params: { owner: string; repo: string; ref: string; per_page?: number }) => Promise<CheckRun[]>;
 * }} OctokitLike
 */

/** Names of the rollup verdict job to drop before slicing. The pair
 *  covers both the post-rename name ("Quality Loop (rollup)") and the
 *  pre-rename name ("Quality Loop") so a Check Run set produced before
 *  the rename lands is still filtered cleanly. */
const ROLLUP_NAMES = new Set(["Quality Loop (rollup)", "Quality Loop"]);

/** GitHub Check Run conclusions ranked worst → best. The `null`
 *  conclusion (run still in_progress) is treated as worse than skipped
 *  but better than action_required so a still-running gate doesn't get
 *  promoted into `top_failures`. Unknown / future conclusions sort to
 *  the end of the list. */
const CONCLUSION_RANK = Object.freeze({
  failure: 0,
  timed_out: 1,
  cancelled: 2,
  action_required: 3,
  null: 4,
  skipped: 5,
  neutral: 6,
  success: 7,
});

/** Conclusions that count as a row-level Red (per the existing row
 *  status grammar). `skipped` and `neutral` are NOT red — a skipped
 *  job is platform-emitted noise (it didn't run because its `if:`
 *  was false), and neutral is the GitHub-API equivalent of "OK but
 *  with a caveat". `null` (in_progress) is Yellow, not Red. */
const RED_CONCLUSIONS = new Set([
  "failure",
  "timed_out",
  "cancelled",
  "action_required",
]);

/** Slice sizes. `top_failures` is rendered inline below the banner;
 *  the schema doc-comment caps it at three (`lib.rs:67`). The
 *  `Row::GateRuns` row's own `gate_runs` list is the dedicated row
 *  drilldown — five entries balance signal density (you see what ran
 *  and how badly) against vertical sticky-comment real estate. */
const TOP_FAILURES_LIMIT = 3;
const GATE_RUNS_ROW_LIMIT = 5;

/** @param {CheckRun} run */
function rankCheckRun(run) {
  /** @type {keyof typeof CONCLUSION_RANK} */
  const conclusion = /** @type {any} */ (run.conclusion ?? "null");
  return CONCLUSION_RANK[conclusion] ?? Number.MAX_SAFE_INTEGER;
}

/** @param {CheckRun} a @param {CheckRun} b */
function compareWorstOf(a, b) {
  const diff = rankCheckRun(a) - rankCheckRun(b);
  if (diff !== 0) {
    return diff;
  }
  // Stable secondary key by name so a tie-break (e.g. two failing
  // jobs) renders deterministically — matters for golden snapshots.
  return a.name.localeCompare(b.name);
}

/** @param {CheckRun} run @returns {GateRun} */
function projectGateRun(run) {
  return {
    gate_name: run.name,
    run_id: run.id,
    url: run.html_url,
  };
}

/** @param {CheckRun[]} checkRuns @returns {CheckRun[]} */
function rankAndFilter(checkRuns) {
  return [...checkRuns]
    .filter((run) => !ROLLUP_NAMES.has(run.name))
    .sort(compareWorstOf);
}

/** @param {CheckRun[]} rankedRuns @returns {GateRun[]} */
function selectTopFailures(rankedRuns) {
  return rankedRuns
    .filter((run) => run.conclusion !== null && RED_CONCLUSIONS.has(run.conclusion))
    .slice(0, TOP_FAILURES_LIMIT)
    .map(projectGateRun);
}

/** @param {CheckRun[]} rankedRuns @returns {GateRun[]} */
function selectGateRunsRow(rankedRuns) {
  return rankedRuns.slice(0, GATE_RUNS_ROW_LIMIT).map(projectGateRun);
}

/** @param {CheckRun[]} rankedRuns @returns {Status} */
function gateRunsRowStatus(rankedRuns) {
  if (rankedRuns.some((run) => run.conclusion !== null && RED_CONCLUSIONS.has(run.conclusion))) {
    return "Red";
  }
  if (rankedRuns.some((run) => run.conclusion == null)) {
    return "Yellow";
  }
  return "Green";
}

/** @param {CheckRun[]} rankedRuns */
function gateRunsRowDeltaText(rankedRuns) {
  const total = rankedRuns.length;
  if (total === 0) {
    return "no gates reported";
  }
  // "Green" matches the row's status logic: any non-failing settled
  // conclusion (success, skipped, neutral) keeps the row green. An
  // all-skipped run set should read "N/N gates green", not "0/N".
  const passing = rankedRuns.filter(
    (run) => run.conclusion !== null && !RED_CONCLUSIONS.has(run.conclusion),
  ).length;
  return `${passing}/${total} gates green`;
}

/** Producer slug stamped on the GateRuns row's `RowCommon.tool` field.
 *  The injector synthesizes the row from the GitHub Check Runs API,
 *  so it self-identifies as `"gate-runs"` rather than borrowing a
 *  vertical-tool name. */
const GATE_RUNS_TOOL = "gate-runs";

/** @param {CheckRun[]} rankedRuns @returns {Row} */
function buildGateRunsRow(rankedRuns) {
  const status = gateRunsRowStatus(rankedRuns);
  /** @type {Row} */
  const row = {
    type: "GateRuns",
    id: "gate_runs",
    label: "Gates",
    anchor: "gate-runs",
    tool: GATE_RUNS_TOOL,
    status,
    gate_runs: selectGateRunsRow(rankedRuns),
    delta_text: gateRunsRowDeltaText(rankedRuns),
  };
  if (status === "Red") {
    const failingNames = rankedRuns
      .filter((run) => run.conclusion !== null && RED_CONCLUSIONS.has(run.conclusion))
      .map((run) => run.name);
    const noun = failingNames.length === 1 ? "gate" : "gates";
    row.failure_detail_md = `${failingNames.length} ${noun} regressed: ${failingNames.join(", ")}`;
  }
  return row;
}

/** @type {Readonly<Record<Status, number>>} */
const STATUS_RANK = Object.freeze({ Green: 0, Yellow: 1, Red: 2 });

/** @param {Row[]} rows @returns {Status} */
function recomputeOverallStatus(rows) {
  /** @type {Status} */
  let worst = "Green";
  for (const row of rows) {
    if ((STATUS_RANK[row.status] ?? -1) > STATUS_RANK[worst]) {
      worst = row.status;
    }
  }
  return worst;
}

/**
 * Pull the full Check Run set for `headSha` and merge into the
 * scorecard envelope. Returns a new envelope object — the input
 * `scorecard` is not mutated.
 *
 * Uses `octokit.paginate` so workflows that emit more than 100 Check
 * Runs (matrix builds, fan-out gates) do not silently truncate.
 *
 * @param {{ octokit: OctokitLike; owner: string; repo: string; headSha: string; scorecard: Scorecard }} args
 * @returns {Promise<Scorecard>} enriched scorecard
 */
async function injectCheckRuns({ octokit, owner, repo, headSha, scorecard }) {
  const checkRuns = await octokit.paginate(octokit.checks.listForRef, {
    owner,
    repo,
    ref: headSha,
    per_page: 100,
  });
  const ranked = rankAndFilter(checkRuns ?? []);
  const gateRunsRow = buildGateRunsRow(ranked);
  const rows = [...scorecard.rows, gateRunsRow];
  return {
    ...scorecard,
    rows,
    top_failures: selectTopFailures(ranked),
    overall_status: recomputeOverallStatus(rows),
  };
}

module.exports = {
  ROLLUP_NAMES,
  CONCLUSION_RANK,
  RED_CONCLUSIONS,
  TOP_FAILURES_LIMIT,
  GATE_RUNS_ROW_LIMIT,
  GATE_RUNS_TOOL,
  injectCheckRuns,
};
