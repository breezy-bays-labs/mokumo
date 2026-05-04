import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { validateScorecard, resolvePointer } from "../validate.js";

const here = dirname(fileURLToPath(import.meta.url));
const SCHEMA_PATH = join(here, "..", "..", "..", "..", ".config", "scorecard", "schema.json");
const schema = JSON.parse(readFileSync(SCHEMA_PATH, "utf8"));

const validScorecard = {
  schema_version: 0,
  pr: {
    pr_number: 1,
    head_sha: "abc",
    base_sha: "def",
    is_fork: false,
  },
  overall_status: "Green",
  rows: [
    {
      type: "CoverageDelta",
      id: "coverage",
      label: "Coverage",
      anchor: "coverage",
      status: "Green",
      delta_pp: 0.0,
      delta_text: "+0.0 pp",
    },
  ],
  top_failures: [],
  all_check_runs_url: "https://github.com/x/y/runs",
  fallback_thresholds_active: true,
};

describe("validateScorecard", () => {
  it("accepts a valid scorecard payload", () => {
    const r = validateScorecard(schema, validScorecard);
    expect(r.valid).toBe(true);
  });

  it("rejects a Red row with no failure_detail_md and surfaces a JSON Pointer", () => {
    const bad = {
      ...validScorecard,
      overall_status: "Red",
      rows: [
        {
          ...validScorecard.rows[0],
          status: "Red",
        },
      ],
    };
    const r = validateScorecard(schema, bad);
    expect(r.valid).toBe(false);
    // The Layer 2 invariant fires somewhere inside /rows/0
    expect(r.pointer).toMatch(/^\/rows\/0/);
    expect(typeof r.message).toBe("string");
  });

  it("rejects failure_detail_md containing a <script> payload", () => {
    // Layer 2 defense in depth: even on a valid Red row, a producer (or
    // a fork-PR-supplied payload that flowed through the producer) cannot
    // smuggle a raw <script> tag into the inline failure detail.
    const bad = {
      ...validScorecard,
      overall_status: "Red",
      rows: [
        {
          ...validScorecard.rows[0],
          status: "Red",
          failure_detail_md: "<script>alert(1)</script>",
        },
      ],
    };
    const r = validateScorecard(schema, bad);
    expect(r.valid).toBe(false);
    expect(r.pointer).toMatch(/^\/rows\/0\/failure_detail_md/);
  });

  it("rejects an unknown overall_status with a pointer at /overall_status", () => {
    const bad = { ...validScorecard, overall_status: "Magenta" };
    const r = validateScorecard(schema, bad);
    expect(r.valid).toBe(false);
    expect(r.pointer).toBe("/overall_status");
    expect(r.value).toBe("Magenta");
  });

  it("rejects a top-level missing required field", () => {
    const { all_check_runs_url, ...bad } = validScorecard;
    const r = validateScorecard(schema, bad);
    expect(r.valid).toBe(false);
    expect(r.message).toMatch(/all_check_runs_url|required/);
  });

  it("returns the JSON Pointer of the failure, not a stack trace", () => {
    const bad = { ...validScorecard, overall_status: "Magenta" };
    const r = validateScorecard(schema, bad);
    expect(r.valid).toBe(false);
    // Stack traces contain "at " from Node frames; ensure we did not
    // bubble one up into the result.
    expect(JSON.stringify(r)).not.toMatch(/\bat .+:\d+:\d+\)/);
  });
});

describe("resolvePointer", () => {
  it("returns the root for empty pointer", () => {
    expect(resolvePointer({ a: 1 }, "")).toEqual({ a: 1 });
  });

  it("walks objects and arrays", () => {
    expect(resolvePointer({ a: [10, 20] }, "/a/1")).toBe(20);
  });

  it("decodes ~1 as / and ~0 as ~ per RFC 6901", () => {
    expect(resolvePointer({ "a/b": 1 }, "/a~1b")).toBe(1);
    expect(resolvePointer({ "a~b": 2 }, "/a~0b")).toBe(2);
  });

  it("returns undefined for an unresolvable path", () => {
    expect(resolvePointer({ a: 1 }, "/missing")).toBeUndefined();
  });
});
