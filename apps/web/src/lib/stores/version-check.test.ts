// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, it, expect } from "vitest";
import { ADMIN_UI_BUILT_FOR, versionCheck } from "./version-check.svelte";

describe("version-check drift guard", () => {
  it("ADMIN_UI_BUILT_FOR matches kikan_types::API_VERSION", () => {
    // Pin: Vite define (__KIKAN_ADMIN_UI_BUILT_FOR__) must stay in
    // lockstep with the Rust-side const. If this test fails, bump both
    // `crates/kikan-types/src/lib.rs` API_VERSION and
    // `apps/web/vite.config.ts` KIKAN_ADMIN_UI_BUILT_FOR together.
    const libRs = readFileSync(
      resolve(__dirname, "../../../../../crates/kikan-types/src/lib.rs"),
      "utf8",
    );
    const match = libRs.match(/pub\s+const\s+API_VERSION\s*:\s*&str\s*=\s*"([^"]+)"/);
    expect(match, "API_VERSION literal not found in kikan-types/src/lib.rs").not.toBeNull();
    expect(ADMIN_UI_BUILT_FOR).toBe(match![1]);
  });
});

describe("version-check runtime behaviour", () => {
  it("reports match when server api_version equals baked UI value", async () => {
    await versionCheck.run(async () => ({
      ok: true,
      status: 200,
      data: {
        api_version: ADMIN_UI_BUILT_FOR,
        engine_version: "0.1.0",
        engine_commit: "abc123",
        schema_versions: {},
      },
    }));
    expect(versionCheck.state.status).toBe("match");
  });

  it("reports mismatch when server api_version diverges", async () => {
    await versionCheck.run(async () => ({
      ok: true,
      status: 200,
      data: {
        api_version: "99.0.0",
        engine_version: "0.1.0",
        engine_commit: "abc123",
        schema_versions: {},
      },
    }));
    expect(versionCheck.state).toMatchObject({
      status: "mismatch",
      serverVersion: "99.0.0",
    });
  });

  it("reports unreachable on network failure (never shows false-positive banner)", async () => {
    await versionCheck.run(async () => ({
      ok: false,
      status: 0,
      error: { code: "network_error", message: "failed", details: null },
    }));
    expect(versionCheck.state.status).toBe("unreachable");
  });
});
