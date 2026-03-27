import { describe, it, expect } from "vitest";
import { formatReport } from "../src/report.ts";
import type { LintResult } from "../src/types.ts";

const emptyResult: LintResult = {
  deadSpecs: [],
  orphanDefs: [],
  stats: {
    featureFiles: 2,
    stepDefFiles: 3,
    totalScenarios: 5,
    totalStepDefs: 10,
    totalSteps: 20,
    matchedSteps: 20,
    unmatchedSteps: 0,
  },
};

const resultWithIssues: LintResult = {
  deadSpecs: [
    {
      featureFile: "auth.feature",
      scenario: "User logs in",
      scenarioLine: 3,
      unmatchedSteps: [
        { keyword: "When", text: "the user enters their password", line: 5 },
      ],
    },
  ],
  orphanDefs: [
    {
      file: "billing.steps.ts",
      pattern: "the refund is processed",
      line: 24,
    },
  ],
  stats: {
    featureFiles: 2,
    stepDefFiles: 3,
    totalScenarios: 5,
    totalStepDefs: 10,
    totalSteps: 20,
    matchedSteps: 19,
    unmatchedSteps: 1,
  },
};

describe("formatReport", () => {
  it("text format shows clean report", () => {
    const output = formatReport(emptyResult, "text");
    expect(output).toContain("BDD Staleness Lint Report");
    expect(output).toContain("Dead Specs: none");
    expect(output).toContain("Orphan Step Definitions: none");
    expect(output).toContain("Feature files:     2");
  });

  it("text format shows dead specs and orphans", () => {
    const output = formatReport(resultWithIssues, "text");
    expect(output).toContain("Dead Specs (1)");
    expect(output).toContain("auth.feature:3");
    expect(output).toContain("User logs in");
    expect(output).toContain("Orphan Step Definitions (1)");
    expect(output).toContain("billing.steps.ts:24");
  });

  it("json format returns valid JSON", () => {
    const output = formatReport(resultWithIssues, "json");
    const parsed = JSON.parse(output);
    expect(parsed.deadSpecs).toHaveLength(1);
    expect(parsed.orphanDefs).toHaveLength(1);
  });

  it("ci format uses GitHub annotations", () => {
    const output = formatReport(resultWithIssues, "ci");
    expect(output).toContain("::warning file=auth.feature,line=5::");
    expect(output).toContain("::warning file=billing.steps.ts,line=24::");
  });

  it("ci format shows clean message when no issues", () => {
    const output = formatReport(emptyResult, "ci");
    expect(output).toBe("BDD lint: all clean");
  });
});
