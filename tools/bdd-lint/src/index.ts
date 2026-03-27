import { parseArgs } from "node:util";
import { resolve } from "node:path";
import { discoverFeatureFiles, discoverStepDefFiles } from "./discover.ts";
import { parseFeatures } from "./parse.ts";
import { extractStepDefs } from "./extract.ts";
import { matchStepsToDefinitions } from "./match.ts";
import { findDeadSpecs, findOrphanStepDefs } from "./detect.ts";
import { formatReport, formatWarnings } from "./report.ts";
import { lint } from "./lint.ts";
import type { LintOptions } from "./types.ts";

const { values } = parseArgs({
  options: {
    format: { type: "string", default: "text" },
    "exclude-tags": { type: "string", default: "@wip" },
    "base-dir": { type: "string", default: "." },
  },
});

const format = (values.format ?? "text") as "text" | "json" | "ci";
const excludeTags = (values["exclude-tags"] ?? "@wip")
  .split(",")
  .map((t) => t.trim())
  .filter(Boolean);
const baseDir = resolve(values["base-dir"] ?? ".");

const options: LintOptions = {
  featureGlobs: [
    "apps/web/tests/features/**/*.feature",
    "crates/*/tests/features/**/*.feature",
    "services/*/tests/features/**/*.feature",
  ],
  stepDefGlobs: [
    "apps/web/tests/steps/**/*.steps.ts",
  ],
  sharedStepPattern: "*-shared.steps.ts",
  excludeTags,
  format,
};

const result = await lint(baseDir, options);

// Surface warnings to stderr (text/ci) or included in JSON output
const warningOutput = formatWarnings(result.warnings, format);
if (warningOutput) {
  console.error(warningOutput);
}

const output = formatReport(result, format);
console.log(output);

process.exit(0);
