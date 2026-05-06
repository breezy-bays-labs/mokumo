import { describe, it, expect, vi } from "vitest";
import {
  injectCheckRuns,
  ROLLUP_NAMES,
  TOP_FAILURES_LIMIT,
  GATE_RUNS_ROW_LIMIT,
  GATE_RUNS_TOOL,
} from "../inject-check-runs.js";

function makeRun({ id, name, conclusion, html_url }) {
  return {
    id,
    name,
    conclusion,
    html_url: html_url ?? `https://github.com/x/y/runs/${id}`,
  };
}

function makeScorecard(overrides = {}) {
  return {
    schema_version: 0,
    pr: { pr_number: 1, head_sha: "abc", base_sha: "def", is_fork: false },
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
    fallback_thresholds_active: false,
    ...overrides,
  };
}

function mockOctokit(checkRuns) {
  const listForRef = vi.fn().mockResolvedValue({
    data: { check_runs: checkRuns, total_count: checkRuns.length },
  });
  // Mirrors the real Octokit pagination plugin: `paginate` recognizes
  // `checks.listForRef` and returns the flattened `check_runs` array
  // across all pages.
  const paginate = vi.fn().mockImplementation(async (fn, params) => {
    const res = await fn(params);
    return res.data.check_runs;
  });
  return { checks: { listForRef }, paginate };
}

describe("injectCheckRuns", () => {
  it("returns an empty top_failures list when every gate succeeded", async () => {
    const octokit = mockOctokit([
      makeRun({ id: 1, name: "coverage-rust", conclusion: "success" }),
      makeRun({ id: 2, name: "lint-rust", conclusion: "success" }),
    ]);
    const result = await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard: makeScorecard(),
    });
    expect(result.top_failures).toEqual([]);
    expect(result.overall_status).toBe("Green");
    const gateRow = result.rows.find((r) => r.type === "GateRuns");
    expect(gateRow.status).toBe("Green");
    expect(gateRow.gate_runs).toHaveLength(2);
  });

  it("populates top_failures with the worst failing gates and emits a Red GateRuns row", async () => {
    const octokit = mockOctokit([
      makeRun({ id: 1, name: "coverage-rust", conclusion: "failure" }),
      makeRun({ id: 2, name: "lint-rust", conclusion: "success" }),
      makeRun({ id: 3, name: "test-rust", conclusion: "timed_out" }),
      makeRun({ id: 4, name: "bdd-lint", conclusion: "skipped" }),
    ]);
    const result = await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard: makeScorecard(),
    });
    // failure + timed_out → top_failures (worst-of order)
    expect(result.top_failures).toHaveLength(2);
    expect(result.top_failures[0].gate_name).toBe("coverage-rust");
    expect(result.top_failures[1].gate_name).toBe("test-rust");
    // GateRuns row carries failure_detail_md (Red row, schema-required)
    const gateRow = result.rows.find((r) => r.type === "GateRuns");
    expect(gateRow.status).toBe("Red");
    expect(gateRow.failure_detail_md).toMatch(/2 gates regressed/);
    expect(gateRow.failure_detail_md).toContain("coverage-rust");
    expect(gateRow.failure_detail_md).toContain("test-rust");
    // Overall rolled up to Red
    expect(result.overall_status).toBe("Red");
  });

  it("treats an all-skipped run set as Green (skipped is platform noise, not failure)", async () => {
    const octokit = mockOctokit([
      makeRun({ id: 1, name: "coverage-rust", conclusion: "skipped" }),
      makeRun({ id: 2, name: "test-rust", conclusion: "skipped" }),
      makeRun({ id: 3, name: "lint-rust", conclusion: "skipped" }),
    ]);
    const result = await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard: makeScorecard(),
    });
    expect(result.top_failures).toEqual([]);
    const gateRow = result.rows.find((r) => r.type === "GateRuns");
    expect(gateRow.status).toBe("Green");
    expect(gateRow.failure_detail_md).toBeUndefined();
  });

  it("produces a Yellow GateRuns row when a gate is still in-progress (conclusion: null)", async () => {
    const octokit = mockOctokit([
      makeRun({ id: 1, name: "coverage-rust", conclusion: "success" }),
      makeRun({ id: 2, name: "test-rust", conclusion: null }),
    ]);
    const result = await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard: makeScorecard(),
    });
    const gateRow = result.rows.find((r) => r.type === "GateRuns");
    expect(gateRow.status).toBe("Yellow");
    expect(result.top_failures).toEqual([]);
  });

  it("excludes the rollup verdict by name match (post-rename + pre-rename forward-compat)", async () => {
    for (const rollupName of ROLLUP_NAMES) {
      const octokit = mockOctokit([
        makeRun({ id: 1, name: "coverage-rust", conclusion: "success" }),
        makeRun({ id: 99, name: rollupName, conclusion: "success" }),
      ]);
      const result = await injectCheckRuns({
        octokit,
        owner: "x",
        repo: "y",
        headSha: "abc",
        scorecard: makeScorecard(),
      });
      const gateRow = result.rows.find((r) => r.type === "GateRuns");
      // Only the non-rollup gate should be in the row
      expect(gateRow.gate_runs.map((g) => g.gate_name)).toEqual(["coverage-rust"]);
    }
  });

  it("caps top_failures at three entries and the GateRuns row at five", async () => {
    const runs = [];
    for (let i = 1; i <= 8; i++) {
      runs.push(makeRun({ id: i, name: `gate-${i}`, conclusion: "failure" }));
    }
    const octokit = mockOctokit(runs);
    const result = await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard: makeScorecard(),
    });
    expect(result.top_failures).toHaveLength(TOP_FAILURES_LIMIT);
    const gateRow = result.rows.find((r) => r.type === "GateRuns");
    expect(gateRow.gate_runs).toHaveLength(GATE_RUNS_ROW_LIMIT);
  });

  it("queries the API with the supplied head_sha (fork-PR boundary)", async () => {
    const octokit = mockOctokit([
      makeRun({ id: 1, name: "coverage-rust", conclusion: "success" }),
    ]);
    await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "forkSha",
      scorecard: makeScorecard({
        pr: { pr_number: 9, head_sha: "forkSha", base_sha: "mainSha", is_fork: true },
      }),
    });
    expect(octokit.checks.listForRef).toHaveBeenCalledWith(
      expect.objectContaining({ ref: "forkSha", owner: "x", repo: "y" }),
    );
  });

  it("does not mutate the input scorecard", async () => {
    const scorecard = makeScorecard();
    const before = JSON.parse(JSON.stringify(scorecard));
    const octokit = mockOctokit([
      makeRun({ id: 1, name: "coverage-rust", conclusion: "failure" }),
    ]);
    await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard,
    });
    expect(scorecard).toEqual(before);
  });

  it("emits a 'no gates reported' delta_text when every Check Run was the rollup", async () => {
    const octokit = mockOctokit([
      makeRun({ id: 1, name: "Quality Loop", conclusion: "success" }),
    ]);
    const result = await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard: makeScorecard(),
    });
    const gateRow = result.rows.find((r) => r.type === "GateRuns");
    expect(gateRow.gate_runs).toEqual([]);
    expect(gateRow.delta_text).toBe("no gates reported");
    expect(gateRow.status).toBe("Green");
  });

  it("uses octokit.paginate so workflows with >100 Check Runs are not truncated", async () => {
    // Synthesize a >100-page run set by handing paginate a stub that
    // returns 150 entries; the real Octokit plugin would assemble these
    // from multiple pages. The contract we pin here is "the injector
    // hands the full flattened array to rankAndFilter", not the page
    // mechanics — those are Octokit's responsibility.
    const runs = [];
    for (let i = 1; i <= 150; i++) {
      runs.push(makeRun({ id: i, name: `gate-${i}`, conclusion: "success" }));
    }
    const listForRef = vi.fn();
    const paginate = vi.fn().mockResolvedValue(runs);
    const octokit = { checks: { listForRef }, paginate };
    const result = await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard: makeScorecard(),
    });
    expect(paginate).toHaveBeenCalledWith(
      listForRef,
      expect.objectContaining({ ref: "abc", per_page: 100 }),
    );
    const gateRow = result.rows.find((r) => r.type === "GateRuns");
    expect(gateRow.delta_text).toBe("150/150 gates green");
  });

  it("counts skipped and neutral conclusions toward the green count", async () => {
    const octokit = mockOctokit([
      makeRun({ id: 1, name: "coverage-rust", conclusion: "success" }),
      makeRun({ id: 2, name: "test-rust", conclusion: "skipped" }),
      makeRun({ id: 3, name: "lint-rust", conclusion: "neutral" }),
    ]);
    const result = await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard: makeScorecard(),
    });
    const gateRow = result.rows.find((r) => r.type === "GateRuns");
    expect(gateRow.status).toBe("Green");
    // All three settled non-failing → 3/3 green, not 1/3.
    expect(gateRow.delta_text).toBe("3/3 gates green");
  });

  it("stamps the synthesized GateRuns row with tool='gate-runs' (#802)", async () => {
    const octokit = mockOctokit([
      makeRun({ id: 1, name: "coverage-rust", conclusion: "success" }),
    ]);
    const result = await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard: makeScorecard(),
    });
    const gateRow = result.rows.find((r) => r.type === "GateRuns");
    expect(gateRow.tool).toBe(GATE_RUNS_TOOL);
    expect(GATE_RUNS_TOOL).toBe("gate-runs");
  });

  it("emits the singular 'gate' for exactly one regression", async () => {
    const octokit = mockOctokit([
      makeRun({ id: 1, name: "coverage-rust", conclusion: "failure" }),
      makeRun({ id: 2, name: "test-rust", conclusion: "success" }),
    ]);
    const result = await injectCheckRuns({
      octokit,
      owner: "x",
      repo: "y",
      headSha: "abc",
      scorecard: makeScorecard(),
    });
    const gateRow = result.rows.find((r) => r.type === "GateRuns");
    expect(gateRow.failure_detail_md).toMatch(/^1 gate regressed:/);
  });
});
