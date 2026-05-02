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

  // ── Fallback-threshold signals (V3 doc-drift gate) ──────────────────
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
  // The render-cli wrapper lets non-Node callers (BDD step-defs, the
  // V3-PR Red-smoke workflow, ad-hoc shell pipelines) feed a JSON
  // artifact through the same renderer the production sticky-comment
  // poster uses, without bouncing through `actions/github-script`.
  // These tests pin its behavior end-to-end via a child process so a
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
