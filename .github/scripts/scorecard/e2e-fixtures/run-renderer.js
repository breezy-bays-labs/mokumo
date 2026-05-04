// E2E driver: render the enriched scorecard to markdown and write to
// `tmp/scorecard-rendered.md`.

"use strict";

const fs = require("node:fs");
const path = require("node:path");

const repoRoot = path.resolve(__dirname, "..", "..", "..", "..");
const { renderScorecardMarkdown } = require(
  path.join(repoRoot, ".github/scripts/scorecard/render.js"),
);

const scorecard = JSON.parse(
  fs.readFileSync(path.join(repoRoot, "tmp/scorecard.json"), "utf8"),
);
const md = renderScorecardMarkdown(scorecard);
fs.writeFileSync(path.join(repoRoot, "tmp/scorecard-rendered.md"), md);
process.stdout.write(md);
