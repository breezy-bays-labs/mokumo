/**
 * Fixtures for the e2e-lan Playwright project.
 *
 * This file must NOT import from app.fixture.ts — the LAN fixture tree is
 * intentionally separate (no auth mocks, no Vite preview server, no BDD wiring).
 *
 * Three fixtures:
 *   _lanBackend   — worker-scoped: one shared BackendHarness per worker.
 *                   Use for non-kill specs (frontend-state, port-management).
 *   lanBackend    — test-scoped passthrough to _lanBackend.
 *   freshLanBackend — test-scoped: a fresh BackendHarness per test.
 *                   MANDATORY for kill-observe specs (SMOKE-01/02/03/04) where
 *                   the backend is killed or stopped mid-test — the worker-scoped
 *                   handle is dead after kill and cannot be reused.
 */
import { test as base } from "@playwright/test";
import { BackendHarness } from "./harness";
import { resolveWebRoot } from "./local-server";

const webRoot = resolveWebRoot(import.meta.url);

type WorkerFixtures = {
  _lanBackend: BackendHarness;
};

type TestFixtures = {
  lanBackend: BackendHarness;
  freshLanBackend: BackendHarness;
};

export const test = base.extend<TestFixtures, WorkerFixtures>({
  // ── Worker-scoped: one shared backend per worker ──────────────────────────
  _lanBackend: [
    // oxlint-disable-next-line no-empty-pattern -- Playwright requires destructuring for fixture params
    async ({}, use) => {
      const h = new BackendHarness(webRoot);
      await h.start();
      try {
        await use(h);
      } finally {
        await h.stop();
        h.cleanup();
      }
    },
    { scope: "worker" },
  ],

  // ── Test-scoped passthrough ────────────────────────────────────────────────
  lanBackend: async ({ _lanBackend }, use) => {
    await use(_lanBackend);
  },

  // ── Test-scoped: fresh harness per test ───────────────────────────────────
  // Use this for every spec that kills or stops the backend mid-test.
  // The worker-scoped _lanBackend handle is permanently dead after kill/stop;
  // only a test-scoped fresh instance can restart cleanly.
  freshLanBackend: async (
    // oxlint-disable-next-line no-empty-pattern -- Playwright requires destructuring for fixture params
    {},
    use,
  ) => {
    const h = new BackendHarness(webRoot);
    await h.start();
    try {
      await use(h);
    } finally {
      await h.stop();
      h.cleanup();
    }
  },
});

export { expect } from "@playwright/test";
