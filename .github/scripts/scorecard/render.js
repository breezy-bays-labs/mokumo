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

/** @type {Record<import("./types").Status, string>} */
const STATUS_ICON = {
  Green: "🟢",
  Yellow: "🟡",
  Red: "🔴",
};

/** Render a single row line as a markdown table row.
 *
 *  @param {import("./types").Row} row
 *  @returns {string}
 */
function renderRow(row) {
  const icon = STATUS_ICON[row.status] || "❔";
  const label = row.label || row.id;
  const delta = row.delta_text || "";
  return `| ${icon} ${row.status} | ${label} | ${delta} |`;
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
    return `\n> **${row.label || row.id}:** (detail missing — see workflow logs)\n`;
  }
  return `\n> **${row.label || row.id}:** ${detail}\n`;
}

/** Build the full sticky-comment body for a valid scorecard artifact.
 *
 *  @param {import("./types").Scorecard} scorecard
 *  @returns {string}
 */
function renderScorecardMarkdown(scorecard) {
  const banner = `${STATUS_ICON[scorecard.overall_status] || "❔"} **CI status: ${scorecard.overall_status}**`;
  const rows = (scorecard.rows || []).map(renderRow).join("\n");
  const detailBlocks = (scorecard.rows || [])
    .map(renderFailureDetail)
    .join("");
  const headerLine = `_PR #${scorecard.pr.pr_number} • head ${scorecard.pr.head_sha.slice(0, 7)}_`;

  return [
    STICKY_MARKER,
    banner,
    "",
    headerLine,
    "",
    "| Status | Gate | Delta |",
    "| --- | --- | --- |",
    rows,
    detailBlocks,
  ].join("\n");
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
  renderScorecardMarkdown,
  renderFailClosedMarkdown,
  postStickyComment,
  findStickyComment,
};
