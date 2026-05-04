import { describe, it, expect, vi } from "vitest";
import { execFileSync } from "node:child_process";
import { readFileSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  STICKY_MARKER,
  FALLBACK_MARKER,
  STARTER_PREAMBLE,
  PATH_HINT_COMMENT,
  PENDING_DELTA_PREFIX,
  PENDING_ICON,
  RENDERER_SCHEMA_VERSION,
  FORWARD_COMPAT_MARKER,
  FORWARD_COMPAT_PREAMBLE,
  isPendingStubRow,
  renderScorecardMarkdown,
  renderFailClosedMarkdown,
  postStickyComment,
} from "../render.js";

const baseScorecard = {
  schema_version: 0,
  pr: {
    pr_number: 42,
    head_sha: "abcdef0123456789",
    base_sha: "0000000000000000",
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
      delta_pp: 0.3,
      delta_text: "+0.3 pp",
    },
  ],
  top_failures: [],
  all_check_runs_url: "https://github.com/breezy-bays-labs/mokumo/runs",
  fallback_thresholds_active: true,
};

describe("renderScorecardMarkdown", () => {
  it("includes the sticky comment HTML marker", () => {
    const md = renderScorecardMarkdown(baseScorecard);
    expect(md).toContain(STICKY_MARKER);
  });

  it("includes the status banner with overall_status", () => {
    const md = renderScorecardMarkdown(baseScorecard);
    expect(md).toContain("CI status: Green");
  });

  it("renders a green icon for green status", () => {
    const md = renderScorecardMarkdown(baseScorecard);
    expect(md).toContain("🟢");
    expect(md).not.toContain("🔴");
  });

  it("renders the row label and delta_text", () => {
    const md = renderScorecardMarkdown(baseScorecard);
    expect(md).toContain("Coverage");
    expect(md).toContain("+0.3 pp");
  });

  it("includes the abbreviated head SHA", () => {
    const md = renderScorecardMarkdown(baseScorecard);
    expect(md).toContain("abcdef0");
  });

  it("renders inline failure_detail_md below a Red row", () => {
    const sc = {
      ...baseScorecard,
      overall_status: "Red",
      rows: [
        {
          ...baseScorecard.rows[0],
          status: "Red",
          delta_text: "-4.2 pp",
          failure_detail_md: "coverage dropped 4.2% on crate kikan",
        },
      ],
    };
    const md = renderScorecardMarkdown(sc);
    expect(md).toContain("coverage dropped 4.2% on crate kikan");
    expect(md).toContain("🔴");
  });

  it("renders the Layer-3 placeholder when a Red row is missing failure_detail_md", () => {
    const sc = {
      ...baseScorecard,
      overall_status: "Red",
      rows: [
        {
          ...baseScorecard.rows[0],
          status: "Red",
          delta_text: "-4.2 pp",
          // failure_detail_md intentionally omitted
        },
      ],
    };
    const md = renderScorecardMarkdown(sc);
    expect(md).toContain("(detail missing — see workflow logs)");
  });

  // ── Fallback-threshold signals (doc-drift gate) ─────────────────────
  //
  // Every byte of the three fallback signals is pinned by these
  // assertions. Drift between the renderer's emitted markdown and the
  // exported constants — or between the constants and the producer-side
  // mirror in `crates/scorecard/src/threshold.rs` — surfaces as a test
  // diff on PR review.

  it("emits FALLBACK_MARKER + STARTER_PREAMBLE + PATH_HINT_COMMENT for fallback artifacts", () => {
    const sc = { ...baseScorecard, fallback_thresholds_active: true };
    const md = renderScorecardMarkdown(sc);
    expect(md).toContain(FALLBACK_MARKER);
    expect(md).toContain(STARTER_PREAMBLE);
    expect(md).toContain(PATH_HINT_COMMENT);
  });

  it("emits no fallback signals when fallback_thresholds_active is false", () => {
    const sc = { ...baseScorecard, fallback_thresholds_active: false };
    const md = renderScorecardMarkdown(sc);
    expect(md).not.toContain(FALLBACK_MARKER);
    expect(md).not.toContain(STARTER_PREAMBLE);
    expect(md).not.toContain(PATH_HINT_COMMENT);
  });

  it("frames a fallback body with the preamble before the banner and the marker after the table", () => {
    const sc = { ...baseScorecard, fallback_thresholds_active: true };
    const md = renderScorecardMarkdown(sc);
    const preambleIdx = md.indexOf(STARTER_PREAMBLE);
    const bannerIdx = md.indexOf("CI status:");
    const tableEnd = md.indexOf("+0.3 pp"); // last table row content
    const markerIdx = md.indexOf(FALLBACK_MARKER);
    const hintIdx = md.indexOf(PATH_HINT_COMMENT);
    expect(preambleIdx).toBeGreaterThan(-1);
    expect(preambleIdx).toBeLessThan(bannerIdx);
    expect(markerIdx).toBeGreaterThan(tableEnd);
    expect(hintIdx).toBeGreaterThan(markerIdx);
  });

  it("FALLBACK_MARKER + PATH_HINT_COMMENT are HTML comments (do not render visibly)", () => {
    expect(FALLBACK_MARKER).toMatch(/^<!--.*-->$/);
    expect(PATH_HINT_COMMENT).toMatch(/^<!--.*-->$/);
  });

  it("STARTER_PREAMBLE is italic markdown linking the operator config", () => {
    expect(STARTER_PREAMBLE.startsWith("_")).toBe(true);
    expect(STARTER_PREAMBLE.endsWith("_")).toBe(true);
    expect(STARTER_PREAMBLE).toContain("`quality.toml`");
    expect(STARTER_PREAMBLE).toContain("QUALITY.md#threshold-tuning");
  });

  // ── Layer-3 producer-pending stub rows ──────────────────────────────
  //
  // Producer-blocked rows ship as Green stubs with `delta_text` keyed
  // by the [`PENDING_DELTA_PREFIX`] sentinel. The renderer surfaces
  // the row inline with a [`PENDING_ICON`] in the status cell so a
  // reviewer can tell at a glance the row is awaiting an upstream
  // producer; GitHub auto-links the issue reference inside the cell.

  /** Build a synthetic stub row for the given variant.
   *  @param {string} type
   *  @param {string} producerRef
   *  @returns {Record<string, unknown>}
   */
  function pendingStubRow(type, producerRef) {
    return {
      type,
      id: type.toLowerCase(),
      label: type,
      anchor: type.toLowerCase(),
      status: "Green",
      delta_text: `${PENDING_DELTA_PREFIX}${producerRef})`,
    };
  }

  it("PENDING_DELTA_PREFIX matches the producer-side aggregate.rs constant byte-for-byte", () => {
    expect(PENDING_DELTA_PREFIX).toBe("(producer pending — see ");
  });

  it("isPendingStubRow recognizes the sentinel only on Green rows starting with the prefix", () => {
    expect(isPendingStubRow(pendingStubRow("CrapDelta", "crap4rs#111"))).toBe(true);
    // Wrong status: Yellow row carrying the same delta_text is not a stub.
    expect(
      isPendingStubRow({ ...pendingStubRow("CrapDelta", "crap4rs#111"), status: "Yellow" }),
    ).toBe(false);
    // Free-form delta_text without the prefix is not a stub.
    expect(
      isPendingStubRow({ ...pendingStubRow("CrapDelta", "crap4rs#111"), delta_text: "5 → 7" }),
    ).toBe(false);
  });

  it.each([
    ["CrapDelta", "crap4rs#111"],
    ["MutationSurvivors", "mokumo#748"],
    ["HandlerCoverageAxis", "mokumo#654, mokumo#655"],
  ])("renders a pending stub row for %s with the sentinel + producer ref autolinked", (type, ref) => {
    const sc = {
      ...baseScorecard,
      rows: [pendingStubRow(type, ref)],
    };
    const md = renderScorecardMarkdown(sc);
    // The pending icon stands in for the green icon so reviewers spot
    // the awaiting-producer state.
    expect(md).toContain(PENDING_ICON);
    expect(md).toContain("Pending");
    // The delta_text cell carries the issue reference verbatim so
    // GitHub auto-links it inside the sticky comment.
    expect(md).toContain(`${PENDING_DELTA_PREFIX}${ref})`);
  });

  it("does not stamp the pending icon on regular Green rows", () => {
    const md = renderScorecardMarkdown(baseScorecard);
    expect(md).not.toContain(PENDING_ICON);
    expect(md).toContain("🟢");
  });
});

describe("two-click rule (renderRow link wrapping)", () => {
  /** Inline pending-stub fixture (the inner describe's helper isn't in scope here). */
  function makePendingStub(type, producerRef) {
    return {
      type,
      id: type.toLowerCase(),
      label: type,
      anchor: type.toLowerCase(),
      status: "Green",
      delta_text: `${PENDING_DELTA_PREFIX}${producerRef})`,
    };
  }

  it("wraps the status indicator in a markdown link to all_check_runs_url for un-mapped rows", () => {
    const md = renderScorecardMarkdown(baseScorecard);
    // CoverageDelta is mapped to coverage-rust, but baseScorecard's
    // top_failures is empty → fall back to all_check_runs_url.
    expect(md).toContain(`[🟢](${baseScorecard.all_check_runs_url})`);
  });

  it("links to the matching top_failures URL when the row id is in the mapping", () => {
    const sc = {
      ...baseScorecard,
      overall_status: "Red",
      top_failures: [
        {
          gate_name: "coverage-rust",
          run_id: 42,
          url: "https://github.com/x/y/runs/42",
        },
      ],
      rows: [
        {
          ...baseScorecard.rows[0],
          status: "Red",
          failure_detail_md: "regression",
        },
      ],
    };
    const md = renderScorecardMarkdown(sc);
    expect(md).toContain("[🔴](https://github.com/x/y/runs/42)");
    // The fallback URL should NOT appear as the wrapper for this row's
    // icon — the more-specific link took precedence.
    const lines = md.split("\n");
    const rowLine = lines.find((l) => l.includes(" Coverage "));
    expect(rowLine).toBeDefined();
    expect(rowLine).not.toContain(`[🔴](${baseScorecard.all_check_runs_url})`);
  });

  it("links a Row::GateRuns row to its first gate_runs entry", () => {
    const sc = {
      ...baseScorecard,
      rows: [
        ...baseScorecard.rows,
        {
          type: "GateRuns",
          id: "gate_runs",
          label: "Gates",
          anchor: "gate-runs",
          status: "Green",
          gate_runs: [
            { gate_name: "coverage-rust", run_id: 1, url: "https://github.com/x/y/runs/1" },
            { gate_name: "test-rust", run_id: 2, url: "https://github.com/x/y/runs/2" },
          ],
          delta_text: "5/5 gates green",
        },
      ],
    };
    const md = renderScorecardMarkdown(sc);
    expect(md).toContain("[🟢](https://github.com/x/y/runs/1)");
  });

  it("falls back to all_check_runs_url for a GateRuns row with no gate_runs", () => {
    const sc = {
      ...baseScorecard,
      rows: [
        {
          type: "GateRuns",
          id: "gate_runs",
          label: "Gates",
          anchor: "gate-runs",
          status: "Green",
          gate_runs: [],
          delta_text: "no gates reported",
        },
      ],
    };
    const md = renderScorecardMarkdown(sc);
    expect(md).toContain(`[🟢](${baseScorecard.all_check_runs_url})`);
  });

  it("preserves the pending icon and links to all_check_runs_url for stub rows", () => {
    const sc = {
      ...baseScorecard,
      rows: [makePendingStub("MutationSurvivors", "mokumo#748")],
    };
    const md = renderScorecardMarkdown(sc);
    expect(md).toContain(`[${PENDING_ICON}](${baseScorecard.all_check_runs_url})`);
  });

  it("fork-PR rendered URLs reference the fork's head SHA, never the base SHA", () => {
    // Trust-boundary pin: a fork-PR-supplied scorecard MUST surface
    // URLs scoped to the fork's HEAD commit (the SHA we actually ran
    // gates against), not the base branch. The producer constructs
    // all_check_runs_url from pr.head_sha, and the injector queries
    // the Check Runs API by head_sha — both are honored downstream
    // here, end-to-end.
    const forkSha = "f000000000000000000000000000000000000000";
    const mainSha = "ba00000000000000000000000000000000000000";
    const sc = {
      ...baseScorecard,
      pr: {
        pr_number: 99,
        head_sha: forkSha,
        base_sha: mainSha,
        is_fork: true,
      },
      overall_status: "Red",
      rows: [
        {
          ...baseScorecard.rows[0],
          status: "Red",
          failure_detail_md: "regression in coverage gate",
        },
        // A second row whose id is not in ROW_ID_TO_JOB_NAME; it falls
        // back to all_check_runs_url so the full head SHA flows into
        // the rendered output (the URL embeds it as
        // commit/{head_sha}/checks).
        {
          type: "FlakyPopulation",
          id: "flaky_population",
          label: "Flaky tests",
          anchor: "flaky-population",
          status: "Green",
          delta_text: "0 flaky markers",
          flaky_count: 0,
        },
      ],
      // Producer's `all_check_runs_url` shape: commit/{head_sha}/checks.
      all_check_runs_url: `https://github.com/x/y/commit/${forkSha}/checks`,
      top_failures: [
        {
          gate_name: "coverage-rust",
          run_id: 42,
          url: "https://github.com/x/y/runs/42",
        },
      ],
    };
    const md = renderScorecardMarkdown(sc);
    // Positive: both the abbreviated header SHA and the full SHA in
    // the all_check_runs_url end up in the rendered comment.
    expect(md).toContain(forkSha.slice(0, 7));
    expect(md).toContain(forkSha);
    // Negative: the base SHA must never leak into the rendered
    // output, even partially (the abbreviated form would be a
    // 7-char substring match).
    expect(md).not.toContain(mainSha);
    expect(md).not.toContain(mainSha.slice(0, 7));
  });

  it("emits at least one Check Run URL per row in the output", () => {
    const sc = {
      ...baseScorecard,
      rows: [
        { ...baseScorecard.rows[0] }, // CoverageDelta
        makePendingStub("CrapDelta", "crap4rs#111"),
        {
          type: "GateRuns",
          id: "gate_runs",
          label: "Gates",
          anchor: "gate-runs",
          status: "Green",
          gate_runs: [
            { gate_name: "coverage-rust", run_id: 1, url: "https://github.com/x/y/runs/1" },
          ],
          delta_text: "3/3 gates green",
        },
      ],
    };
    const md = renderScorecardMarkdown(sc);
    // Three rows → three rendered table lines starting with `| [`. The
    // anchor is the markdown-link prefix that proves the icon was wrapped.
    const linkedRowCount = md.split("\n").filter((l) => l.startsWith("| [")).length;
    expect(linkedRowCount).toBe(3);
  });
});

describe("renderFailClosedMarkdown", () => {
  it("contains the sticky marker so it overwrites the same comment", () => {
    const md = renderFailClosedMarkdown(
      { pr_number: 42 },
      {
        valid: false,
        pointer: "/rows/0/failure_detail_md",
        value: undefined,
        message: "must have required property 'failure_detail_md'",
      },
    );
    expect(md).toContain(STICKY_MARKER);
  });

  it("contains the JSON Pointer of the failing field", () => {
    const md = renderFailClosedMarkdown(
      { pr_number: 42 },
      {
        valid: false,
        pointer: "/rows/0/failure_detail_md",
        value: undefined,
        message: "must have required property 'failure_detail_md'",
      },
    );
    expect(md).toContain("/rows/0/failure_detail_md");
  });

  it("contains the offending value", () => {
    const md = renderFailClosedMarkdown(
      { pr_number: 42 },
      {
        valid: false,
        pointer: "/overall_status",
        value: "Magenta",
        message: "must be equal to one of the allowed values",
      },
    );
    expect(md).toContain('"Magenta"');
  });

  it("explains the failure in plain prose, no green status", () => {
    const md = renderFailClosedMarkdown(
      { pr_number: 42 },
      { valid: false, pointer: "/foo", value: 1, message: "bad" },
    );
    expect(md).toContain("did not match the committed schema");
    expect(md).not.toContain("CI status: Green");
  });
});

describe("postStickyComment", () => {
  function makeOctokit({ existing = [] } = {}) {
    const updateComment = vi.fn().mockResolvedValue({ data: {} });
    const createComment = vi.fn().mockResolvedValue({ data: { id: 999 } });
    const listComments = vi.fn();
    const paginate = vi.fn().mockResolvedValue(existing);
    return {
      rest: {
        issues: {
          listComments,
          createComment,
          updateComment,
        },
      },
      paginate,
      _spies: { updateComment, createComment, paginate },
    };
  }

  it("creates a new comment when none exists with the marker", async () => {
    const octokit = makeOctokit({ existing: [] });
    const result = await postStickyComment({
      octokit,
      owner: "breezy-bays-labs",
      repo: "mokumo",
      prNumber: 42,
      body: `${STICKY_MARKER}\nbody`,
    });
    expect(result.action).toBe("created");
    expect(octokit._spies.createComment).toHaveBeenCalledTimes(1);
    expect(octokit._spies.updateComment).not.toHaveBeenCalled();
  });

  it("updates the existing marker-bearing comment instead of creating a duplicate", async () => {
    const octokit = makeOctokit({
      existing: [
        { id: 11, body: "unrelated" },
        { id: 22, body: `${STICKY_MARKER}\nold body` },
      ],
    });
    const result = await postStickyComment({
      octokit,
      owner: "breezy-bays-labs",
      repo: "mokumo",
      prNumber: 42,
      body: `${STICKY_MARKER}\nnew body`,
    });
    expect(result.action).toBe("updated");
    expect(result.comment_id).toBe(22);
    expect(octokit._spies.updateComment).toHaveBeenCalledTimes(1);
    expect(octokit._spies.createComment).not.toHaveBeenCalled();
  });

  it("is idempotent: a second invocation with the same marker still updates, never creates", async () => {
    const octokit = makeOctokit({
      existing: [{ id: 33, body: `${STICKY_MARKER}\nfirst body` }],
    });
    await postStickyComment({
      octokit,
      owner: "x",
      repo: "y",
      prNumber: 1,
      body: `${STICKY_MARKER}\nsecond body`,
    });
    await postStickyComment({
      octokit,
      owner: "x",
      repo: "y",
      prNumber: 1,
      body: `${STICKY_MARKER}\nthird body`,
    });
    expect(octokit._spies.updateComment).toHaveBeenCalledTimes(2);
    expect(octokit._spies.createComment).not.toHaveBeenCalled();
  });

  it("ignores user comments that quote the marker mid-body (anchored startsWith match)", async () => {
    // A user replied to the bot quoting the marker text inline. The
    // sticky poster must NOT update the user's comment — only update or
    // create one that *starts* with the marker.
    const octokit = makeOctokit({
      existing: [
        {
          id: 77,
          body: `> ${STICKY_MARKER}\n> our scorecard says we're red — fixing now`,
        },
      ],
    });
    const result = await postStickyComment({
      octokit,
      owner: "x",
      repo: "y",
      prNumber: 1,
      body: `${STICKY_MARKER}\nnew body`,
    });
    expect(result.action).toBe("created");
    expect(octokit._spies.createComment).toHaveBeenCalledTimes(1);
    expect(octokit._spies.updateComment).not.toHaveBeenCalled();
  });

  it("re-checks before create: if a sticky lands in the TOCTOU window, route to update", async () => {
    // Simulate two paginate calls: the first returns nothing, the
    // second returns a sticky (i.e. a racing run created it between
    // our list and our intended create).
    const updateComment = vi.fn().mockResolvedValue({ data: {} });
    const createComment = vi.fn().mockResolvedValue({ data: { id: 999 } });
    const listComments = vi.fn();
    const paginate = vi
      .fn()
      .mockResolvedValueOnce([{ id: 1, body: "unrelated" }])
      .mockResolvedValueOnce([
        { id: 88, body: `${STICKY_MARKER}\nracer-posted body` },
      ]);
    const octokit = {
      rest: { issues: { listComments, createComment, updateComment } },
      paginate,
    };
    const result = await postStickyComment({
      octokit,
      owner: "x",
      repo: "y",
      prNumber: 1,
      body: `${STICKY_MARKER}\nour body`,
    });
    expect(result.action).toBe("updated");
    expect(result.comment_id).toBe(88);
    expect(updateComment).toHaveBeenCalledTimes(1);
    expect(createComment).not.toHaveBeenCalled();
    expect(paginate).toHaveBeenCalledTimes(2);
  });
});

describe("renderer dependency hygiene", () => {
  // The producer (Rust) owns TOML parsing — see ADR §Threshold
  // resolution lives in the producer. The renderer must NEVER pick up
  // a TOML parser as a dep, even transitively. This regex enumerates
  // the parsers that have appeared in the npm ecosystem; extend if a
  // new one surfaces.
  const FORBIDDEN_TOML_PARSERS =
    /^(@iarna\/toml|@ltd\/j-toml|toml|smol-toml|js-toml|toml-js|tomlify|tomlify-j0\.4)$/;

  it("renderer package.json declares no TOML parser", () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const pkgPath = join(here, "..", "package.json");
    const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
    const allDeps = Object.keys({
      ...(pkg.dependencies ?? {}),
      ...(pkg.devDependencies ?? {}),
      ...(pkg.peerDependencies ?? {}),
      ...(pkg.optionalDependencies ?? {}),
    });
    const hits = allDeps.filter((d) => FORBIDDEN_TOML_PARSERS.test(d));
    expect(hits).toEqual([]);
  });
});

describe("bin/render-cli.js", () => {
  // The render-cli wrapper lets non-Node callers (BDD step-defs,
  // smoke workflows, ad-hoc shell pipelines) feed a JSON artifact
  // through the same renderer the production sticky-comment poster
  // uses, without bouncing through `actions/github-script`. These
  // tests pin its behavior end-to-end via a child process so a
  // regression in stdin reading or stdout writing surfaces immediately.

  function runCli(jsonString) {
    const here = dirname(fileURLToPath(import.meta.url));
    const cliPath = join(here, "..", "bin", "render-cli.js");
    const tmp = mkdtempSync(join(tmpdir(), "render-cli-"));
    const inputPath = join(tmp, "input.json");
    writeFileSync(inputPath, jsonString);
    const stdout = execFileSync("node", [cliPath], {
      input: readFileSync(inputPath),
      encoding: "utf8",
    });
    return stdout;
  }

  function fixture(extra) {
    return JSON.stringify({
      schema_version: 0,
      pr: {
        pr_number: 768,
        head_sha: "deadbeef0000000",
        base_sha: "cafefeed0000000",
        is_fork: false,
      },
      overall_status: "Yellow",
      rows: [
        {
          type: "CoverageDelta",
          id: "coverage",
          label: "Coverage",
          anchor: "coverage",
          status: "Yellow",
          delta_pp: -2.5,
          delta_text: "-2.5 pp",
        },
      ],
      top_failures: [],
      all_check_runs_url: "https://example.test/checks",
      fallback_thresholds_active: true,
      ...extra,
    });
  }

  it("reads JSON stdin and writes the rendered markdown to stdout", () => {
    const stdout = runCli(fixture());
    expect(stdout).toContain(STICKY_MARKER);
    expect(stdout).toContain("CI status: Yellow");
  });

  it("matches the in-process renderer output byte-for-byte", () => {
    const json = fixture();
    const stdout = runCli(json);
    const inProcess = renderScorecardMarkdown(JSON.parse(json));
    expect(stdout).toBe(inProcess);
  });

  it("emits the fallback signals when fallback_thresholds_active is true", () => {
    const stdout = runCli(fixture({ fallback_thresholds_active: true }));
    expect(stdout).toContain(STARTER_PREAMBLE);
    expect(stdout).toContain(FALLBACK_MARKER);
    expect(stdout).toContain(PATH_HINT_COMMENT);
  });

  it("omits the fallback signals when fallback_thresholds_active is false", () => {
    const stdout = runCli(fixture({ fallback_thresholds_active: false }));
    expect(stdout).not.toContain(STARTER_PREAMBLE);
    expect(stdout).not.toContain(FALLBACK_MARKER);
    expect(stdout).not.toContain(PATH_HINT_COMMENT);
  });
});

describe("forward-compat degradation banner", () => {
  it("RENDERER_SCHEMA_VERSION is a non-negative integer", () => {
    expect(Number.isInteger(RENDERER_SCHEMA_VERSION)).toBe(true);
    expect(RENDERER_SCHEMA_VERSION).toBeGreaterThanOrEqual(0);
  });

  it("emits the forward-compat marker when artifact version exceeds renderer", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      const md = renderScorecardMarkdown({
        ...baseScorecard,
        schema_version: RENDERER_SCHEMA_VERSION + 1,
      });
      expect(md).toContain(FORWARD_COMPAT_MARKER);
      expect(md).toContain(FORWARD_COMPAT_PREAMBLE);
      expect(warn).toHaveBeenCalled();
    } finally {
      warn.mockRestore();
    }
  });

  it("does not emit the forward-compat banner at the renderer's pinned version", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      const md = renderScorecardMarkdown({
        ...baseScorecard,
        schema_version: RENDERER_SCHEMA_VERSION,
      });
      expect(md).not.toContain(FORWARD_COMPAT_MARKER);
      expect(md).not.toContain(FORWARD_COMPAT_PREAMBLE);
      expect(warn).not.toHaveBeenCalled();
    } finally {
      warn.mockRestore();
    }
  });

  it("does not emit the forward-compat banner at older schema versions", () => {
    const md = renderScorecardMarkdown({
      ...baseScorecard,
      schema_version: 0,
    });
    expect(md).not.toContain(FORWARD_COMPAT_MARKER);
  });
});

describe("Layer 3 missing-detail defensive fallback", () => {
  it("renders placeholder + console.warns when a Red row lacks failure_detail_md", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      const md = renderScorecardMarkdown({
        ...baseScorecard,
        overall_status: "Red",
        rows: [
          {
            type: "CoverageDelta",
            id: "coverage",
            label: "Coverage",
            anchor: "coverage",
            status: "Red",
            delta_pp: -7.5,
            delta_text: "-7.5 pp",
            // failure_detail_md OMITTED — Layer 3 defensive path.
          },
        ],
      });
      expect(md).toContain("(detail missing — see workflow logs)");
      expect(warn).toHaveBeenCalled();
      const call = warn.mock.calls.find((args) =>
        String(args[0]).includes("missing failure_detail_md"),
      );
      expect(call).toBeDefined();
    } finally {
      warn.mockRestore();
    }
  });

  it("renders the supplied detail when failure_detail_md is present", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      const md = renderScorecardMarkdown({
        ...baseScorecard,
        overall_status: "Red",
        rows: [
          {
            type: "CoverageDelta",
            id: "coverage",
            label: "Coverage",
            anchor: "coverage",
            status: "Red",
            delta_pp: -7.5,
            delta_text: "-7.5 pp",
            failure_detail_md: "Coverage dropped 7.5 pp.",
          },
        ],
      });
      expect(md).toContain("Coverage dropped 7.5 pp.");
      expect(md).not.toContain("(detail missing");
      expect(warn).not.toHaveBeenCalled();
    } finally {
      warn.mockRestore();
    }
  });
});
