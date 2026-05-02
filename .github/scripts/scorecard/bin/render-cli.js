#!/usr/bin/env node
// Tiny stdin → stdout wrapper around `renderScorecardMarkdown`.
//
// Reads a scorecard artifact JSON from stdin (file descriptor 0),
// renders the sticky-comment markdown body, and writes it to stdout.
// The wrapper exists so CI steps and ad-hoc scripts can pipe a JSON
// artifact through without bouncing through `actions/github-script`'s
// inline JavaScript or starting a Node REPL.
//
// The renderer module's `module.exports` surface is intentionally
// unchanged; this wrapper is additive.
//
// Usage:
//   node .github/scripts/scorecard/bin/render-cli.js < scorecard.json > comment.md

"use strict";

const { renderScorecardMarkdown } = require("../render.js");
const fs = require("node:fs");

const data = JSON.parse(fs.readFileSync(0, "utf8"));
process.stdout.write(renderScorecardMarkdown(data));
