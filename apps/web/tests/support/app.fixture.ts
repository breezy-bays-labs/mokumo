import type { Page } from "@playwright/test";
import { createBdd } from "playwright-bdd";
import type { MeResponse } from "../../src/lib/types/MeResponse";
import type { ServerInfoResponse } from "../../src/lib/types/ServerInfoResponse";
import type { UserResponse } from "../../src/lib/types/UserResponse";
import { test as base } from "./storybook.fixture";
import { resolveWebRoot, startPreviewServer } from "./local-server";

type WorkerFixtures = {
  appUrl: string;
};

type TestFixtures = {
  lanTestState: {
    serverInfo: ServerInfoResponse | null;
  };
};

const webRoot = resolveWebRoot(import.meta.url);
const SETUP_STATUS_ROUTE = "**/api/setup-status";
const AUTH_ME_ROUTE = "**/api/auth/me";

const DEFAULT_USER: UserResponse = {
  id: 1,
  email: "admin@shop.local",
  name: "Admin",
  role_name: "Admin",
  is_active: true,
  last_login_at: null,
  created_at: "2026-03-27T00:00:00Z",
};

const DEFAULT_ME_RESPONSE: MeResponse = {
  user: DEFAULT_USER,
  setup_complete: true,
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

export const test = base.extend<TestFixtures, WorkerFixtures>({
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
