import type { ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { APIRequestContext, Page } from "@playwright/test";
import { createBdd } from "playwright-bdd";
import type { CustomerResponse } from "../../src/lib/types/CustomerResponse";
import type { MeResponse } from "../../src/lib/types/MeResponse";
import type { ServerInfoResponse } from "../../src/lib/types/ServerInfoResponse";
import type { UserResponse } from "../../src/lib/types/UserResponse";
import { test as base } from "playwright-bdd";
import { TEST_ADMIN } from "./app-helpers";
import {
  login as apiLogin,
  runSetupWizard as apiRunSetupWizard,
  type SetupCredentials,
} from "./api-client";
import {
  buildHttpUrl,
  getAvailablePort,
  resolveWebRoot,
  startAxumServer,
  startPreviewServer,
  TEST_SERVER_HOST,
} from "./local-server";

export type AxumHandle = {
  process: ChildProcess | null;
  port: number;
  url: string;
  tmpDirs: string[];
  setupToken: string | null;
};

type WorkerFixtures = {
  appUrl: string;
  _axumServer: AxumHandle;
  axumUrl: string;
};

type TestFixtures = {
  lanTestState: {
    serverInfo: ServerInfoResponse | null;
  };
  freshBackend: void;
  apiContext: APIRequestContext;
  customerContext: {
    customers: CustomerResponse[];
    lastCustomer: CustomerResponse | null;
  };
};

const webRoot = resolveWebRoot(import.meta.url);
const SETUP_STATUS_ROUTE = "**/api/setup-status";
const AUTH_ME_ROUTE = "**/api/auth/me";

const DEFAULT_USER: UserResponse = {
  id: 1,
  email: TEST_ADMIN.email,
  name: TEST_ADMIN.name,
  role_name: "Admin",
  is_active: true,
  last_login_at: null,
  created_at: "2026-03-27T00:00:00Z",
};

const DEFAULT_ME_RESPONSE: MeResponse = {
  user: DEFAULT_USER,
  setup_complete: true,
  recovery_codes_remaining: 10,
};

async function mockAuthenticatedAppShell(page: Page): Promise<void> {
  await page.route(SETUP_STATUS_ROUTE, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ setup_complete: true }),
    });
  });

  await page.route(AUTH_ME_ROUTE, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(DEFAULT_ME_RESPONSE),
    });
  });
}

/** Build SetupCredentials from TEST_ADMIN constants. */
function buildSetupCredentials(setupToken: string): SetupCredentials {
  return {
    setupToken,
    adminEmail: TEST_ADMIN.email,
    adminName: TEST_ADMIN.name,
    adminPassword: TEST_ADMIN.password,
    shopName: TEST_ADMIN.shopName,
  };
}

/** Login via api-client and transfer session cookie to the browser context. */
async function loginAndTransferCookies(baseUrl: string, page: Page): Promise<void> {
  const { setCookie } = await apiLogin(baseUrl, TEST_ADMIN.email, TEST_ADMIN.password);

  // Parse Set-Cookie header into Playwright cookie format
  const cookieParts = setCookie.split(";")[0].split("=");
  const name = cookieParts[0].trim();
  const value = cookieParts.slice(1).join("=").trim();
  const url = new URL(baseUrl);

  await page.context().addCookies([
    {
      name,
      value,
      domain: url.hostname,
      path: "/",
    },
  ]);
}

export const test = base.extend<TestFixtures, WorkerFixtures>({
  // Existing: Vite preview server for settings tests
  appUrl: [
    async ({ browserName: _browserName }, use) => {
      const { server, url } = await startPreviewServer(webRoot);

      try {
        await use(url);
      } finally {
        server.kill("SIGTERM");
      }
    },
    { auto: true, scope: "worker" },
  ],

  // Worker-scoped Axum backend handle (internal — use axumUrl instead)
  _axumServer: [
    // oxlint-disable-next-line no-empty-pattern -- Playwright requires destructuring for fixture params
    async ({}, use) => {
      const port = await getAvailablePort();
      const url = buildHttpUrl(TEST_SERVER_HOST, port);
      const firstTmpDir = mkdtempSync(join(tmpdir(), "mokumo-test-"));
      const { server, setupToken } = await startAxumServer(webRoot, port, firstTmpDir);

      const handle: AxumHandle = {
        process: server,
        port,
        url,
        tmpDirs: [firstTmpDir],
        setupToken,
      };

      await use(handle);

      // Cleanup: kill process and remove all tmpdirs
      handle.process?.kill("SIGTERM");
      for (const dir of handle.tmpDirs) {
        rmSync(dir, { recursive: true, force: true });
      }
    },
    { scope: "worker" },
  ],

  // Stable Axum URL (worker-scoped, port doesn't change across restarts)
  axumUrl: [
    async ({ _axumServer }, use) => {
      await use(_axumServer.url);
    },
    { scope: "worker" },
  ],

  // Restart Axum with a fresh database + run setup wizard before each customer scenario
  freshBackend: async ({ _axumServer, page }, use) => {
    // Kill current Axum process
    if (_axumServer.process && _axumServer.process.exitCode === null) {
      _axumServer.process.kill("SIGTERM");
      await new Promise<void>((resolve) => {
        const proc = _axumServer.process;
        if (!proc || proc.exitCode !== null) {
          resolve();
          return;
        }
        proc.on("exit", () => resolve());
        setTimeout(() => resolve(), 5_000);
      });
    }

    // Create new tmpdir with fresh database
    const newTmpDir = mkdtempSync(join(tmpdir(), "mokumo-test-"));
    _axumServer.tmpDirs.push(newTmpDir);

    // Respawn Axum with same port, new data directory
    const { server, setupToken } = await startAxumServer(webRoot, _axumServer.port, newTmpDir);
    _axumServer.process = server;
    _axumServer.setupToken = setupToken;

    // Run setup wizard + login so both API and browser are authenticated
    if (setupToken) {
      await apiRunSetupWizard(_axumServer.url, buildSetupCredentials(setupToken));
      await loginAndTransferCookies(_axumServer.url, page);
    } else {
      // Verify the server genuinely doesn't need setup (not a missed token capture)
      const statusRes = await fetch(`${_axumServer.url}/api/setup-status`);
      const status = await statusRes.json();
      if (!status.setup_complete) {
        throw new Error(
          "Axum server requires setup but no setup token was captured from stdout. " +
            "The server log format may have changed — check startAxumServer token regex.",
        );
      }
    }

    await use();
  },

  // Playwright request context — authenticated via login (setup already done by freshBackend)
  apiContext: async ({ freshBackend: _fb, axumUrl, playwright }, use) => {
    void _fb;
    const ctx = await playwright.request.newContext({ baseURL: axumUrl });
    // Login to get session cookie
    const loginRes = await ctx.post("/api/auth/login", {
      data: { email: TEST_ADMIN.email, password: TEST_ADMIN.password },
    });
    if (!loginRes.ok()) {
      const body = await loginRes.text();
      throw new Error(`apiContext login failed (${loginRes.status()}): ${body}`);
    }
    await use(ctx);
    await ctx.dispose();
  },

  // Shared state for customer test data between steps
  // oxlint-disable-next-line no-empty-pattern -- Playwright requires destructuring for fixture params
  customerContext: async ({}, use) => {
    await use({ customers: [], lastCustomer: null });
  },

  lanTestState: async ({ browserName: _browserName }, use) => {
    await use({ serverInfo: null });
  },

  page: async ({ appUrl, page }, use) => {
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"], {
      origin: appUrl,
    });

    await mockAuthenticatedAppShell(page);
    await use(page);
  },
});

export const { Given, When, Then } = createBdd(test);
