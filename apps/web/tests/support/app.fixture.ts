import type { ChildProcess } from "node:child_process";
import type { APIRequestContext, Page } from "@playwright/test";
import { createBdd } from "playwright-bdd";
import type { CustomerResponse } from "../../src/lib/types/kikan/CustomerResponse";
import type { MeResponse } from "../../src/lib/types/kikan/MeResponse";
import type { ServerInfoResponse } from "../../src/lib/types/kikan/ServerInfoResponse";
import type { UserResponse } from "../../src/lib/types/kikan/UserResponse";
import { test as base } from "playwright-bdd";
import { TEST_ADMIN } from "./app-helpers";
import {
  login as apiLogin,
  runSetupWizard as apiRunSetupWizard,
  type SetupCredentials,
} from "./api-client";
import { BackendHarness } from "./harness";
import { resolveWebRoot, startPreviewServer } from "./local-server";

/**
 * A10 — BackendHarness delegation.
 *
 * AxumHandle no longer owns raw mutable fields. All state comes from the
 * underlying BackendHarness via readonly getters. This prevents freshBackend
 * from using direct field mutations (which break once the fields are getters).
 */
export type AxumHandle = {
  harness: BackendHarness;
  readonly process: ChildProcess | null;
  readonly port: number;
  readonly url: string;
  readonly setupToken: string | null;
};

type WorkerFixtures = {
  appUrl: string;
  _axumServer: AxumHandle;
};

type TestFixtures = {
  axumUrl: string;
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
  updated_at: "2026-03-27T00:00:00Z",
  deleted_at: null,
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
      body: JSON.stringify({
        setup_complete: true,
        setup_mode: "production",
        is_first_launch: false,
        production_setup_complete: false,
        shop_name: null,
      }),
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

  // Worker-scoped Axum backend — now delegates to BackendHarness (A10).
  // All state is exposed via readonly getters on AxumHandle.
  _axumServer: [
    // oxlint-disable-next-line no-empty-pattern -- Playwright requires destructuring for fixture params
    async ({}, use) => {
      const harness = new BackendHarness(webRoot);
      await harness.start();

      const handle: AxumHandle = {
        harness,
        get process() {
          return harness.process;
        },
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

      await harness.stop();
      harness.cleanup();
    },
    { scope: "worker" },
  ],

  // Axum URL — test-scoped so it always reflects the current harness URL
  // (freshBackend may restart on a different port if the same port isn't free)
  axumUrl: async ({ _axumServer }, use) => {
    await use(_axumServer.url);
  },

  // Restart Axum with a fresh database + run setup wizard before each customer scenario.
  // Delegates stop/start to BackendHarness — no raw field mutations.
  freshBackend: async ({ _axumServer, page }, use) => {
    await _axumServer.harness.stop();

    // Let the harness allocate and track the fresh tmpdir — callers must not
    // pass a manually-created dir here or it will escape harness.cleanup().
    await _axumServer.harness.start(_axumServer.port);

    // Getters on AxumHandle now return updated values from the restarted harness.
    const { url, setupToken } = _axumServer;

    if (setupToken) {
      await apiRunSetupWizard(url, buildSetupCredentials(setupToken));
      await loginAndTransferCookies(url, page);
    } else {
      // Verify the server genuinely doesn't need setup (not a missed token capture)
      const statusRes = await fetch(`${url}/api/setup-status`);
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
