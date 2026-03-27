import { expect, type Page } from "@playwright/test";
import type { ServerInfoResponse } from "../../src/lib/types/ServerInfoResponse";
import { Given, Then, When } from "../support/app.fixture";
import { buildHttpUrl, TEST_SERVER_HOST } from "../support/local-server";

const SHOP_SETTINGS_PATH = "/settings/shop";
const SERVER_INFO_ROUTE = "**/api/server-info";
const TOAST_SELECTOR = "[data-sonner-toast]";
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

Given("the server-info API returns LAN status", async ({ lanTestState, page }) => {
  lanTestState.serverInfo = ACTIVE_SERVER_INFO;
  await mockServerInfo(page, ACTIVE_SERVER_INFO);
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

When("I copy the LAN URL", async ({ page }) => {
  await page.getByRole("button", { name: "Copy LAN URL to clipboard" }).click();
});

Then("I see the {string} card", async ({ page }, cardTitle: string) => {
  await expect(page.getByText(cardTitle)).toBeVisible();
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

Then("I see the LAN URL {string}", async ({ page }, url: string) => {
  await expect(page.getByText(url)).toBeVisible();
});

Then("the LAN URL is shown", async ({ lanTestState, page }) => {
  await expect(page.getByText(lanTestState.serverInfo?.lan_url ?? "")).toBeVisible();
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
  await expect(page.getByText(lanTestState.serverInfo?.ip_url ?? "")).toBeVisible();
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
  const codeBlocks = page.locator("code");
  const count = await codeBlocks.count();
  let foundPort = false;

  for (let i = 0; i < count; i += 1) {
    const text = await codeBlocks.nth(i).textContent();
    if (text?.includes(`:${port}`)) {
      foundPort = true;
      break;
    }
  }

  expect(foundPort, `Expected at least one URL containing port ${port}`).toBe(true);
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

Then("I see a {string} toast message", async ({ page }, message: string) => {
  await expect(page.locator(TOAST_SELECTOR).filter({ hasText: message }).first()).toBeVisible();
});
