import type { LintResult } from "./types.ts";

export function formatReport(result: LintResult, format: "text" | "json" | "ci"): string {
  switch (format) {
    case "json":
      return JSON.stringify(result, null, 2);
    case "ci":
      return formatCi(result);
    case "text":
    default:
      return formatText(result);
  }
}

export function formatWarnings(warnings: string[], format: "text" | "json" | "ci"): string {
  if (warnings.length === 0) return "";
  switch (format) {
    case "json":
      return ""; // warnings are included in the JSON output directly
    case "ci":
      return warnings.map((w) => `::warning::${w}`).join("\n");
    case "text":
    default:
      return warnings.map((w) => `[warn] ${w}`).join("\n");
  }
}

function formatText(result: LintResult): string {
  const lines: string[] = [];

  lines.push("BDD Staleness Lint Report");
  lines.push("=".repeat(50));
  lines.push("");

  // Stats
  lines.push(`Feature files:     ${result.stats.featureFiles}`);
  lines.push(`Step def files:    ${result.stats.stepDefFiles}`);
  lines.push(`Total scenarios:   ${result.stats.totalScenarios}`);
  lines.push(`Total steps:       ${result.stats.totalSteps}`);
  lines.push(`Matched steps:     ${result.stats.matchedSteps}`);
  lines.push(`Unmatched steps:   ${result.stats.unmatchedSteps}`);
  lines.push(`Step definitions:  ${result.stats.totalStepDefs}`);
  lines.push(`@wip scenarios:    ${result.stats.wipScenarios}`);
  lines.push(`Stale @wip:        ${result.stats.staleWipScenarios}`);
  lines.push("");

  // Dead specs
  if (result.deadSpecs.length > 0) {
    lines.push(`Dead Specs (${result.deadSpecs.length})`);
    lines.push("-".repeat(40));
    for (const spec of result.deadSpecs) {
      lines.push(`  ${spec.featureFile}:${spec.scenarioLine} — ${spec.scenario}`);
      for (const step of spec.unmatchedSteps) {
        lines.push(`    ${step.keyword} ${step.text} (line ${step.line})`);
      }
    }
    lines.push("");
  } else {
    lines.push("Dead Specs: none");
    lines.push("");
  }

  // Orphan defs
  if (result.orphanDefs.length > 0) {
    lines.push(`Orphan Step Definitions (${result.orphanDefs.length})`);
    lines.push("-".repeat(40));
    for (const orphan of result.orphanDefs) {
      lines.push(`  ${orphan.file}:${orphan.line} — "${orphan.pattern}"`);
    }
    lines.push("");
  } else {
    lines.push("Orphan Step Definitions: none");
    lines.push("");
  }

  // Stale @wip scenarios
  if (result.staleWipScenarios.length > 0) {
    lines.push(`Stale @wip Scenarios (${result.staleWipScenarios.length})`);
    lines.push("-".repeat(40));
    for (const stale of result.staleWipScenarios) {
      lines.push(`  ${stale.featureFile}:${stale.scenarioLine} — ${stale.scenario} (${stale.stepCount} steps, all matched)`);
    }
    lines.push("");
  } else {
    lines.push("Stale @wip Scenarios: none");
    lines.push("");
  }

  return lines.join("\n");
}

function formatCi(result: LintResult): string {
  const lines: string[] = [];

  for (const spec of result.deadSpecs) {
    for (const step of spec.unmatchedSteps) {
      lines.push(
        `::warning file=${spec.featureFile},line=${step.line}::Dead spec: ${step.keyword} ${step.text} (scenario: ${spec.scenario})`,
      );
    }
  }

  for (const orphan of result.orphanDefs) {
    lines.push(
      `::warning file=${orphan.file},line=${orphan.line}::Orphan step def: "${orphan.pattern}"`,
    );
  }

  for (const stale of result.staleWipScenarios) {
    lines.push(
      `::warning file=${stale.featureFile},line=${stale.scenarioLine}::Stale @wip: "${stale.scenario}" has all ${stale.stepCount} steps implemented — remove @wip tag`,
    );
  }

  if (lines.length === 0) {
    lines.push("BDD lint: all clean");
  }

  return lines.join("\n");
}
