import { describe, it, expect } from "vitest";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { extractStepDefs } from "../src/extract.ts";
import { matchStepsToDefinitions } from "../src/match.ts";
import type { StepInfo } from "../src/types.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));

function makeStep(text: string, overrides?: Partial<StepInfo>): StepInfo {
  return {
    featureFile: "test.feature",
    featureName: "Test",
    scenario: "Test scenario",
    scenarioLine: 1,
    keyword: "Given",
    text,
    line: 1,
    tags: [],
    ...overrides,
  };
}

describe("matchStepsToDefinitions", () => {
  it("matches string pattern step definitions", async () => {
    const fixtureDir = resolve(__dirname, "fixtures/clean");
    const stepDefFile = resolve(fixtureDir, "auth.steps.ts");
    const { expressionLinks } = await extractStepDefs([stepDefFile], "*-shared.steps.ts");

    const steps = [
      makeStep("the user enters their password"),
      makeStep("the user is logged in"),
    ];

    const result = matchStepsToDefinitions(steps, expressionLinks);
    expect(result.matchedSteps).toHaveLength(2);
    expect(result.unmatchedSteps).toHaveLength(0);
  });

  it("matches parameterized cucumber expressions", async () => {
    const fixtureDir = resolve(__dirname, "fixtures/clean");
    const stepDefFile = resolve(fixtureDir, "auth.steps.ts");
    const { expressionLinks } = await extractStepDefs([stepDefFile], "*-shared.steps.ts");

    const steps = [
      makeStep('a user with email "alice@example.com"'),
      makeStep('a user with email "bob@test.org"'),
    ];

    const result = matchStepsToDefinitions(steps, expressionLinks);
    expect(result.matchedSteps).toHaveLength(2);
    expect(result.unmatchedSteps).toHaveLength(0);
  });

  it("reports unmatched steps when no definition exists", async () => {
    const fixtureDir = resolve(__dirname, "fixtures/clean");
    const stepDefFile = resolve(fixtureDir, "auth.steps.ts");
    const { expressionLinks } = await extractStepDefs([stepDefFile], "*-shared.steps.ts");

    const steps = [
      makeStep("the user enters their password"),
      makeStep("this step has no definition"),
    ];

    const result = matchStepsToDefinitions(steps, expressionLinks);
    expect(result.matchedSteps).toHaveLength(1);
    expect(result.unmatchedSteps).toHaveLength(1);
    expect(result.unmatchedSteps[0].text).toBe("this step has no definition");
  });
});
