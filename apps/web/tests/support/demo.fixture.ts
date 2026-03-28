import { test as base, type Page } from "@playwright/test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  getAvailablePort,
  buildHttpUrl,
  resolveWebRoot,
  startAxumServer,
  TEST_SERVER_HOST,
} from "./local-server";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const webRoot = resolveWebRoot(import.meta.url);

// demo.db location — TODO: update path after #153 merges and demo.db is created
// For S0.1 smoke test, we skip the copy and use a fresh empty DB.
const _DEMO_DB_SOURCE = resolve(__dirname, "../../../../fixtures/demo.db");

export const SCREENSHOT_BASE = resolve(__dirname, "../../../../docs/demo-guide/public/m0");

type DemoServerHandle = {
  process: import("node:child_process").ChildProcess | null;
  port: number;
  url: string;
  tmpDir: string;
  setupToken: string | null;
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
      const port = await getAvailablePort();
      const url = buildHttpUrl(TEST_SERVER_HOST, port);
      const tmpDir = mkdtempSync(join(tmpdir(), "mokumo-demo-"));

      // TODO: When #153 merges, uncomment to copy demo.db:
      // cpSync(DEMO_DB_SOURCE, join(tmpDir, "data.db"));

      const { server, setupToken } = await startAxumServer(webRoot, port, tmpDir);

      const handle: DemoServerHandle = {
        process: server,
        port,
        url,
        tmpDir,
        setupToken,
      };

      await use(handle);

      handle.process?.kill("SIGTERM");
      rmSync(tmpDir, { recursive: true, force: true });
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
