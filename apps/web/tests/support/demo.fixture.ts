import { test as base, type Page } from "@playwright/test";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { BackendHarness } from "./harness";
import { resolveWebRoot } from "./local-server";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const webRoot = resolveWebRoot(import.meta.url);

// demo.db location — TODO: update path after #153 merges and demo.db is created
// For S0.1 smoke test, we skip the copy and use a fresh empty DB.
const _DEMO_DB_SOURCE = resolve(__dirname, "../../../../fixtures/demo.db");

export const SCREENSHOT_BASE = resolve(__dirname, "../../../../docs/demo-guide/public/m0");

type DemoServerHandle = {
  harness: BackendHarness;
  readonly port: number;
  readonly url: string;
  readonly setupToken: string | null;
};

type DemoWorkerFixtures = {
  _demoServer: DemoServerHandle;
  demoPage: Page;
  setupToken: string;
};

export const test = base.extend<object, DemoWorkerFixtures>({
  _demoServer: [
    // oxlint-disable-next-line no-empty-pattern -- Playwright requires destructuring for fixture params
    async ({}, use) => {
      // TODO: When #153 merges, uncomment to copy demo.db:
      // cpSync(DEMO_DB_SOURCE, join(tmpDir, "data.db"));
      void _DEMO_DB_SOURCE;

      const harness = new BackendHarness(webRoot);
      await harness.start();

      const handle: DemoServerHandle = {
        harness,
        get port() {
          return harness.port;
        },
        get url() {
          return harness.url;
        },
        get setupToken() {
          return harness.setupToken;
        },
      };

      await use(handle);

      harness.kill();
      harness.cleanup();
    },
    { scope: "worker", timeout: 60_000 },
  ],

  demoPage: [
    async ({ _demoServer, browser }, use) => {
      const context = await browser.newContext({
        baseURL: _demoServer.url,
        viewport: { width: 1280, height: 720 },
        colorScheme: "dark",
        deviceScaleFactor: 1,
      });

      const page = await context.newPage();

      // Belt-and-suspenders: set mode-watcher localStorage before any navigation
      await page.addInitScript(() => {
        localStorage.setItem("mode-watcher-mode", "dark");
      });

      await use(page);

      await context.close();
    },
    { scope: "worker" },
  ],

  setupToken: [
    async ({ _demoServer }, use) => {
      if (!_demoServer.setupToken) {
        throw new Error(
          "Setup token was not captured from Axum stdout. " +
            "Check startAxumServer token regex matches the server log format.",
        );
      }
      await use(_demoServer.setupToken);
    },
    { scope: "worker" },
  ],
});

export { expect } from "@playwright/test";
