import { expect, type Page } from "@playwright/test";
import type { ServerInfoResponse } from "../../src/lib/types/ServerInfoResponse";
import { Given, Then, When } from "../support/app.fixture";
import { mockSetupStatus } from "../support/setup-status.helpers";
import { buildHttpUrl, TEST_SERVER_HOST } from "../support/local-server";

const SHOP_SETTINGS_PATH = "/settings/shop";
const SERVER_INFO_ROUTE = "**/api/server-info";
const LAN_ACCESS_ROUTE = "**/api/settings/lan-access";

type LanPrefState = { enabled: boolean; lastWritten: boolean | null };
const lanPrefStates = new WeakMap<Page, LanPrefState>();

function getLanPrefState(page: Page): LanPrefState {
  let state = lanPrefStates.get(page);
  if (!state) {
    state = { enabled: false, lastWritten: null };
    lanPrefStates.set(page, state);
  }
  return state;
}

async function mockLanAccess(page: Page): Promise<void> {
  const state = getLanPrefState(page);
  await page.route(LAN_ACCESS_ROUTE, async (route) => {
    const req = route.request();
    if (req.method() === "PUT") {
      const body = (await req.postDataJSON()) as { enabled: boolean };
      state.enabled = body.enabled;
      state.lastWritten = body.enabled;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ enabled: body.enabled }),
      });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ enabled: state.enabled }),
    });
  });
}
const TEST_DEVICE_PORT = Number(process.env.PLAYWRIGHT_TEST_DEVICE_PORT ?? "3000");
const TEST_MDNS_HOST = process.env.PLAYWRIGHT_TEST_MDNS_HOST ?? "mokumo.local";
const TEST_IP_HOST = process.env.PLAYWRIGHT_TEST_IP_HOST ?? "192.168.1.42";

const ACTIVE_SERVER_INFO: ServerInfoResponse = {
  host: TEST_MDNS_HOST,
  ip_url: buildHttpUrl(TEST_IP_HOST, TEST_DEVICE_PORT),
  lan_url: buildHttpUrl(TEST_MDNS_HOST, TEST_DEVICE_PORT),
  mdns_active: true,
  port: TEST_DEVICE_PORT,
};

const UNAVAILABLE_SERVER_INFO: ServerInfoResponse = {
  host: TEST_IP_HOST,
  ip_url: buildHttpUrl(TEST_IP_HOST, TEST_DEVICE_PORT),
  lan_url: null,
  mdns_active: false,
  port: TEST_DEVICE_PORT,
};

const DISABLED_SERVER_INFO: ServerInfoResponse = {
  host: TEST_SERVER_HOST,
  ip_url: null,
  lan_url: null,
  mdns_active: false,
  port: TEST_DEVICE_PORT,
};

async function mockServerInfo(page: Page, serverInfo: ServerInfoResponse): Promise<void> {
  await page.route(SERVER_INFO_ROUTE, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(serverInfo),
    });
  });
}

async function loadShopSettingsPage(
  page: Page,
  appUrl: string,
  serverInfo: ServerInfoResponse,
): Promise<void> {
  await mockServerInfo(page, serverInfo);
  await page.goto(new URL(SHOP_SETTINGS_PATH, appUrl).toString());
  await expect(page.getByRole("heading", { name: "Shop Settings" })).toBeVisible();
  await expect(page.getByTestId("lan-status-badge")).toBeVisible();
}

function statusBadge(page: Page, status: string) {
  return page
    .getByTestId("lan-status-badge")
    .filter({ hasText: status })
    .or(page.locator("[data-slot='badge']").filter({ hasText: status }));
}

function requireServerInfoUrl(
  stepName: string,
  serverInfo: ServerInfoResponse | null,
  field: "ip_url" | "lan_url",
): string {
  expect(
    serverInfo,
    `Step "${stepName}" requires lanTestState.serverInfo before page.getByText/page.getByRole assertions.`,
  ).not.toBeNull();

  const url = serverInfo?.[field];
  expect(
    url,
    `Step "${stepName}" requires lanTestState.serverInfo.${field} before page.getByText/page.getByRole assertions.`,
  ).toBeTruthy();

  if (!url) {
    throw new Error(`Step "${stepName}" requires lanTestState.serverInfo.${field}.`);
  }

  return url;
}

Given("the server-info API returns LAN status", async ({ lanTestState, page }) => {
  lanTestState.serverInfo = ACTIVE_SERVER_INFO;
  await mockServerInfo(page, ACTIVE_SERVER_INFO);
});

Given("the server-info API returns an error", async ({ page }) => {
  await page.route(SERVER_INFO_ROUTE, async (route) => {
    await route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({
        code: "server_error",
        message: "Unable to fetch server info",
        details: null,
      }),
    });
  });
});

Given("the shop name is {string}", async ({ page }, shopName: string) => {
  await mockSetupStatus(page, { shop_name: shopName });
});

Given("no shop name is configured", async ({ page }) => {
  await mockSetupStatus(page, { shop_name: null });
});

Given("the server-info API returns mDNS inactive", async ({ lanTestState, page }) => {
  lanTestState.serverInfo = UNAVAILABLE_SERVER_INFO;
  await mockServerInfo(page, UNAVAILABLE_SERVER_INFO);
});

Given(
  "the shop settings page loads with active LAN access",
  async ({ appUrl, lanTestState, page }) => {
    lanTestState.serverInfo = ACTIVE_SERVER_INFO;
    await loadShopSettingsPage(page, appUrl, ACTIVE_SERVER_INFO);
  },
);

Given(
  "the shop settings page loads with unavailable LAN access",
  async ({ appUrl, lanTestState, page }) => {
    lanTestState.serverInfo = UNAVAILABLE_SERVER_INFO;
    await loadShopSettingsPage(page, appUrl, UNAVAILABLE_SERVER_INFO);
  },
);

Given(
  "the shop settings page loads with disabled LAN access",
  async ({ appUrl, lanTestState, page }) => {
    lanTestState.serverInfo = DISABLED_SERVER_INFO;
    await loadShopSettingsPage(page, appUrl, DISABLED_SERVER_INFO);
  },
);

When("I navigate to the Shop settings page", async ({ page, appUrl }) => {
  await page.goto(new URL(SHOP_SETTINGS_PATH, appUrl).toString());
  await expect(page.getByText("LAN Access")).toBeVisible();
});

When("I navigate to the System settings page", async ({ page, appUrl }) => {
  await page.goto(new URL("/settings/system", appUrl).toString());
  await page.waitForLoadState("networkidle");
});

Then("I do not see {string}", async ({ page }, text: string) => {
  await expect(page.getByText(text)).not.toBeVisible();
});

Then("I see an {string} status badge", async ({ page }, status: string) => {
  await expect(statusBadge(page, status)).toBeVisible();
});

Then("I see a {string} status badge", async ({ page }, status: string) => {
  await expect(statusBadge(page, status)).toBeVisible();
});

Then("the LAN status badge shows {string}", async ({ page }, label: string) => {
  await expect(page.getByTestId("lan-status-badge")).toHaveText(label);
});

Then("the LAN URL is shown", async ({ lanTestState, page }) => {
  const lanUrl = requireServerInfoUrl("the LAN URL is shown", lanTestState.serverInfo, "lan_url");

  await expect(page.getByText(lanUrl)).toBeVisible();
  await expect(page.getByRole("button", { name: "Copy LAN URL to clipboard" })).toBeVisible();
});

Then("the LAN URL is not shown", async ({ page }) => {
  await expect(page.getByText("LAN URL")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Copy LAN URL to clipboard" })).toHaveCount(0);
});

Then("I see the IP address {string}", async ({ page }, ip: string) => {
  await expect(page.getByText(ip)).toBeVisible();
});

Then("the IP fallback URL is shown", async ({ lanTestState, page }) => {
  const ipUrl = requireServerInfoUrl(
    "the IP fallback URL is shown",
    lanTestState.serverInfo,
    "ip_url",
  );

  await expect(page.getByText(ipUrl)).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Copy IP address URL to clipboard" }),
  ).toBeVisible();
});

Then("the IP fallback URL is not shown", async ({ page }) => {
  await expect(page.getByText("IP Address")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Copy IP address URL to clipboard" })).toHaveCount(
    0,
  );
});

Then("the displayed URLs include port {string}", async ({ page }, port: string) => {
  await expect(
    page
      .locator("code")
      .filter({ hasText: `:${port}` })
      .first(),
  ).toBeVisible();
});

Then("the LAN URL contains the mDNS hostname {string}", async ({ page }, hostname: string) => {
  await expect(page.getByText(hostname)).toBeVisible();
});

Then("the LAN status helper text is {string}", async ({ page }, text: string) => {
  await expect(page.getByText(text)).toBeVisible();
});

Then("the clipboard contains the LAN URL", async ({ lanTestState, page }) => {
  await expect
    .poll(async () => page.evaluate(() => navigator.clipboard.readText()))
    .toBe(lanTestState.serverInfo?.lan_url);
});

// --- LAN access preference toggle ---

Given("the LAN access preference is enabled", async ({ page }) => {
  getLanPrefState(page).enabled = true;
  await mockLanAccess(page);
});

Given("the LAN access preference is disabled", async ({ page }) => {
  getLanPrefState(page).enabled = false;
  await mockLanAccess(page);
});

When("I toggle LAN access off", async ({ page }) => {
  await page.getByTestId("lan-access-toggle").click();
});

When("I toggle LAN access on", async ({ page }) => {
  await page.getByTestId("lan-access-toggle").click();
});

Then("the LAN access toggle is on", async ({ page }) => {
  await expect(page.getByTestId("lan-access-toggle")).toHaveAttribute("data-state", "checked");
});

Then("the LAN access toggle is off", async ({ page }) => {
  await expect(page.getByTestId("lan-access-toggle")).toHaveAttribute("data-state", "unchecked");
});
