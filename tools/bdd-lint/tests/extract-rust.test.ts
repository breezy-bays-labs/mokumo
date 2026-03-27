import { describe, it, expect } from "vitest";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { extractRustStepDefs, lineNumberAt } from "../src/extract-rust.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));

describe("lineNumberAt", () => {
  it("returns 1 for offset 0", () => {
    expect(lineNumberAt("hello\nworld", 0)).toBe(1);
  });

  it("counts newlines before offset", () => {
    expect(lineNumberAt("a\nb\nc\nd", 4)).toBe(3);
  });
});

describe("extractRustStepDefs", () => {
  it("extracts step defs from a Rust fixture", () => {
    const file = resolve(__dirname, "fixtures/stale-wip-rust/inventory_steps.rs");
    const result = extractRustStepDefs([file]);

    expect(result.stepDefs).toHaveLength(3);
    expect(result.warnings).toHaveLength(0);

    const patterns = result.stepDefs.map((d) => d.pattern);
    expect(patterns).toContain("an empty warehouse");
    expect(patterns).toContain("an item {string} is added with quantity {int}");
    expect(patterns).toContain("the inventory should contain {string}");
  });

  it("builds expression links for matching", () => {
    const file = resolve(__dirname, "fixtures/stale-wip-rust/inventory_steps.rs");
    const result = extractRustStepDefs([file]);

    expect(result.expressionLinks).toHaveLength(3);
    for (const link of result.expressionLinks) {
      expect(link.expression.source).toBeTruthy();
      expect(link.locationLink.targetUri).toContain("inventory_steps.rs");
    }
  });

  it("warns on unreadable file", () => {
    const result = extractRustStepDefs(["/nonexistent/file.rs"]);

    expect(result.stepDefs).toHaveLength(0);
    expect(result.warnings).toHaveLength(1);
    expect(result.warnings[0]).toContain("Could not read");
  });

  it("warns on regex step defs and excludes them from results", () => {
    const file = resolve(__dirname, "fixtures/regex-step/regex_steps.rs");
    const result = extractRustStepDefs([file]);

    expect(result.stepDefs).toHaveLength(0);
    expect(result.expressionLinks).toHaveLength(0);
    const regexWarnings = result.warnings.filter((w) => w.includes("regex patterns"));
    expect(regexWarnings).toHaveLength(1);
  });

  it("warns on unparseable Cucumber Expression", () => {
    const file = resolve(__dirname, "fixtures/bad-expr/bad_steps.rs");
    const result = extractRustStepDefs([file]);

    expect(result.stepDefs).toHaveLength(0); // unparseable expressions are excluded
    expect(result.expressionLinks).toHaveLength(0);
    const parseWarnings = result.warnings.filter((w) => w.includes("Could not parse"));
    expect(parseWarnings).toHaveLength(1);
  });

  it("returns correct line numbers", () => {
    const file = resolve(__dirname, "fixtures/stale-wip-rust/inventory_steps.rs");
    const result = extractRustStepDefs([file]);

    // All step defs should have line > 0
    for (const def of result.stepDefs) {
      expect(def.line).toBeGreaterThan(0);
    }
  });
});
