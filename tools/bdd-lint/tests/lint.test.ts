import { describe, it, expect } from "vitest";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { lint } from "../src/lint.ts";
import type { LintOptions } from "../src/types.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));

function fixtureOptions(
  fixture: string,
  overrides?: Partial<LintOptions>,
): { baseDir: string; options: LintOptions } {
  const baseDir = resolve(__dirname, "fixtures", fixture);
  return {
    baseDir,
    options: {
      featureGlobs: ["**/*.feature"],
      stepDefGlobs: ["**/*.steps.ts"],
      sharedStepPattern: "*-shared.steps.ts",
      excludeTags: ["@wip"],
      format: "text",
      ...overrides,
    },
  };
}

describe("clean fixture", () => {
  it("reports zero dead specs and zero orphans", async () => {
    const { baseDir, options } = fixtureOptions("clean");
    const result = await lint(baseDir, options);

    expect(result.deadSpecs).toHaveLength(0);
    expect(result.orphanDefs).toHaveLength(0);
    expect(result.stats.featureFiles).toBe(1);
    expect(result.stats.stepDefFiles).toBe(1);
    expect(result.stats.matchedSteps).toBeGreaterThan(0);
    expect(result.stats.unmatchedSteps).toBe(0);
  });
});

describe("dead-spec fixture", () => {
  it("detects scenarios with unmatched steps", async () => {
    const { baseDir, options } = fixtureOptions("dead-spec");
    const result = await lint(baseDir, options);

    expect(result.deadSpecs).toHaveLength(1);
    expect(result.deadSpecs[0].scenario).toBe("User completes checkout");
    expect(result.deadSpecs[0].unmatchedSteps).toHaveLength(2);

    const stepTexts = result.deadSpecs[0].unmatchedSteps.map((s) => s.text);
    expect(stepTexts).toContain("the user enters their shipping address");
    expect(stepTexts).toContain("the order is placed");
  });

  it("includes file path and line numbers in dead spec", async () => {
    const { baseDir, options } = fixtureOptions("dead-spec");
    const result = await lint(baseDir, options);

    expect(result.deadSpecs[0].featureFile).toContain("checkout.feature");
    expect(result.deadSpecs[0].scenarioLine).toBeGreaterThan(0);
    for (const step of result.deadSpecs[0].unmatchedSteps) {
      expect(step.line).toBeGreaterThan(0);
    }
  });
});

describe("orphan-def fixture", () => {
  it("detects step definitions with no matching scenario", async () => {
    const { baseDir, options } = fixtureOptions("orphan-def");
    const result = await lint(baseDir, options);

    expect(result.deadSpecs).toHaveLength(0);
    expect(result.orphanDefs).toHaveLength(3);

    const orphanPatterns = result.orphanDefs.map((o) => o.pattern);
    expect(orphanPatterns).toContain("the user has a payment method on file");
    expect(orphanPatterns).toContain("the user cancels their subscription");
    expect(orphanPatterns).toContain("the refund is processed");
  });

  it("includes file path and line in orphan report", async () => {
    const { baseDir, options } = fixtureOptions("orphan-def");
    const result = await lint(baseDir, options);

    for (const orphan of result.orphanDefs) {
      expect(orphan.file).toContain("billing.steps.ts");
      expect(orphan.line).toBeGreaterThan(0);
    }
  });
});

describe("wip fixture", () => {
  it("excludes @wip scenarios from dead spec detection", async () => {
    const { baseDir, options } = fixtureOptions("wip");
    const result = await lint(baseDir, options);

    // The @wip scenario "Future login flow" should NOT appear as dead spec
    // The non-wip scenario "Current login flow" should be fully matched
    expect(result.deadSpecs).toHaveLength(0);
  });

  it("detects step defs whose only match is a @wip scenario as orphan", async () => {
    const { baseDir, options } = fixtureOptions("wip");
    const result = await lint(baseDir, options);

    // The SSO-related defs only match @wip scenario → orphans
    expect(result.orphanDefs.length).toBe(3);
    const orphanPatterns = result.orphanDefs.map((o) => o.pattern);
    expect(orphanPatterns).toContain("a user with SSO enabled");
    expect(orphanPatterns).toContain("the user authenticates via SSO");
    expect(orphanPatterns).toContain("the user is redirected to the dashboard");
  });

  it("supports custom exclude tags", async () => {
    const { baseDir, options } = fixtureOptions("wip", {
      excludeTags: ["@custom-tag"],
    });
    const result = await lint(baseDir, options);

    // With @custom-tag (not @wip), the @wip scenario IS analyzed
    // SSO defs now match → not orphans
    // But SSO steps have no definitions... wait, they DO have defs
    // So actually everything should match and no orphans
    expect(result.orphanDefs).toHaveLength(0);
    expect(result.deadSpecs).toHaveLength(0);
  });
});

describe("shared fixture", () => {
  it("shared step def matching across features is not orphan", async () => {
    const { baseDir, options } = fixtureOptions("shared");
    const result = await lint(baseDir, options);

    // "the user is authenticated" is shared, matches auth.feature and billing.feature
    const orphanPatterns = result.orphanDefs.map((o) => o.pattern);
    expect(orphanPatterns).not.toContain("the user is authenticated");
  });

  it("shared step def whose only match is @wip is orphan", async () => {
    const { baseDir, options } = fixtureOptions("shared");
    const result = await lint(baseDir, options);

    // "the system is under maintenance" only matches billing @wip scenario → orphan
    const orphanPatterns = result.orphanDefs.map((o) => o.pattern);
    expect(orphanPatterns).toContain("the system is under maintenance");
  });

  it("billing @wip step defs that only match wip scenario are orphans", async () => {
    const { baseDir, options } = fixtureOptions("shared");
    const result = await lint(baseDir, options);

    // "a maintenance message is shown" only matches @wip scenario → orphan
    const orphanPatterns = result.orphanDefs.map((o) => o.pattern);
    expect(orphanPatterns).toContain("a maintenance message is shown");
  });

  it("reports zero dead specs for shared fixture", async () => {
    const { baseDir, options } = fixtureOptions("shared");
    const result = await lint(baseDir, options);

    expect(result.deadSpecs).toHaveLength(0);
  });
});

describe("mixed fixture", () => {
  it("detects both dead specs and orphan defs", async () => {
    const { baseDir, options } = fixtureOptions("mixed");
    const result = await lint(baseDir, options);

    // Dead spec: "User tracks order" has 3 unmatched steps
    expect(result.deadSpecs).toHaveLength(1);
    expect(result.deadSpecs[0].scenario).toBe("User tracks order");
    expect(result.deadSpecs[0].unmatchedSteps).toHaveLength(3);

    // Orphans: "the user has store credit" and "the user applies store credit"
    expect(result.orphanDefs).toHaveLength(2);
    const orphanPatterns = result.orphanDefs.map((o) => o.pattern);
    expect(orphanPatterns).toContain("the user has store credit");
    expect(orphanPatterns).toContain("the user applies store credit");
  });
});
