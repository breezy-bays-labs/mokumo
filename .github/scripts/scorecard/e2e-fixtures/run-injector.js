// E2E driver: run the injector against a committed fixture instead of
// the live Check Runs API. Reads the path of the fixture from
// `process.env.FIXTURE_PATH`, mocks octokit, writes the enriched
// scorecard back to `tmp/scorecard.json` in place.

"use strict";

const fs = require("node:fs");
const path = require("node:path");

const repoRoot = path.resolve(__dirname, "..", "..", "..", "..");
const { injectCheckRuns } = require(
  path.join(repoRoot, ".github/scripts/scorecard/inject-check-runs.js"),
);

const fixturePath = process.env.FIXTURE_PATH;
if (!fixturePath) {
  console.error("FIXTURE_PATH env var is required");
  process.exit(1);
}

const fixture = JSON.parse(fs.readFileSync(fixturePath, "utf8"));
const scorecardPath = path.join(repoRoot, "tmp/scorecard.json");
const scorecard = JSON.parse(fs.readFileSync(scorecardPath, "utf8"));

const listForRef = () => Promise.resolve({ data: fixture });
const octokit = {
  checks: { listForRef },
  // Mirrors real Octokit pagination semantics for the checks endpoint:
  // returns the flattened `check_runs` array across all pages.
  paginate: async (fn, params) => {
    const res = await fn(params);
    return res.data.check_runs;
  },
};

(async () => {
  const enriched = await injectCheckRuns({
    octokit,
    owner: "breezy-bays-labs",
    repo: "mokumo",
    headSha: scorecard.pr.head_sha,
    scorecard,
  });
  fs.writeFileSync(scorecardPath, JSON.stringify(enriched, null, 2) + "\n");
  const gateRunsRow = enriched.rows.find((r) => r.type === "GateRuns");
  if (!gateRunsRow) {
    console.error(
      `[scorecard] GateRuns row missing from enriched scorecard. ` +
        `rows=${enriched.rows.map((r) => r.type).join(",")}; ` +
        `top_failures=${enriched.top_failures.length}. ` +
        `Injection completed but did not append the GateRuns row.`,
    );
    process.exit(1);
  }
  console.log(
    `injected ${enriched.top_failures.length} top_failures + GateRuns row (status=${gateRunsRow.status})`,
  );
})().catch((err) => {
  console.error(err);
  process.exit(1);
});
