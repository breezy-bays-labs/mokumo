import { parseArgs } from "node:util";
import { resolve } from "node:path";
import { formatReport, formatWarnings } from "./report.ts";
import { lint } from "./lint.ts";
import type { LintOptions } from "./types.ts";

const { values } = parseArgs({
  options: {
    format: { type: "string", default: "text" },
    "exclude-tags": { type: "string", default: "@wip" },
    "base-dir": { type: "string", default: "." },
    "max-dead-specs": { type: "string", default: "" },
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
  rustStepDefGlobs: [
    "services/*/tests/bdd_world/**/*.rs",
    "crates/*/tests/bdd_world/**/*.rs",
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

// Dead specs are blocking errors; orphan defs and stale WIP are advisory warnings.
// --max-dead-specs ratchets the count: fail only if dead specs exceed the threshold.
// This tolerates known false positives from matcher limitations (unsupported regex
// patterns, unparseable Cucumber expressions) while preventing regressions.
const rawMaxDeadSpecs = values["max-dead-specs"];
const maxDeadSpecs = rawMaxDeadSpecs ? parseInt(rawMaxDeadSpecs, 10) : 0;
if (rawMaxDeadSpecs && Number.isNaN(maxDeadSpecs)) {
  console.error(`\nFAIL: --max-dead-specs must be a number, got "${rawMaxDeadSpecs}"`);
  process.exit(1);
}
const hasErrors = result.deadSpecs.length > maxDeadSpecs;
if (hasErrors) {
  console.error(
    `\nFAIL: ${result.deadSpecs.length} dead specs found (max allowed: ${maxDeadSpecs})`
  );
}
process.exit(hasErrors ? 1 : 0);
