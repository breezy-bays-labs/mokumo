import { expect, type Page } from "@playwright/test";
import { Given, When, Then } from "../support/app.fixture";

const SETUP_STATUS_ROUTE = "**/api/setup-status";
const PROFILE_SWITCH_ROUTE = "**/api/profile/switch";

// ────────────────────────────────────────────────────────────────────────────
// Per-test state
// ────────────────────────────────────────────────────────────────────────────

type ProfileTestState = {
  setupMode: "demo" | "production";
  productionSetupComplete: boolean;
  shopName: string | null;
  urlBeforeClick: string | null;
  switchRequests: Array<unknown>;
};

const testState = new WeakMap<Page, ProfileTestState>();

function getState(page: Page): ProfileTestState {
  if (!testState.has(page)) {
    testState.set(page, {
      setupMode: "demo",
      productionSetupComplete: false,
      shopName: null,
      urlBeforeClick: null,
      switchRequests: [],
    });
  }
  return testState.get(page)!;
}

async function interceptSwitchRoute(
  page: Page,
  state: ProfileTestState,
  responseProfile: "demo" | "production",
  delayMs = 0,
): Promise<void> {
  await page.route(PROFILE_SWITCH_ROUTE, async (route) => {
    state.switchRequests.push(route.request().postDataJSON() as unknown);
    if (delayMs > 0) await new Promise<void>((r) => setTimeout(r, delayMs));
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ profile: responseProfile }),
    });
  });
}

function makeSetupStatusBody(s: ProfileTestState): string {
  return JSON.stringify({
    setup_complete: true,
    setup_mode: s.setupMode,
    is_first_launch: false,
    production_setup_complete: s.productionSetupComplete,
    shop_name: s.shopName,
  });
}

async function applySetupStatusMock(page: Page): Promise<void> {
  const s = getState(page);
  await page.route(SETUP_STATUS_ROUTE, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: makeSetupStatusBody(s),
    }),
  );
}

async function navigateToApp(page: Page): Promise<void> {
  await applySetupStatusMock(page);
  await page.goto("/");
  await page.waitForLoadState("networkidle");
}

// ────────────────────────────────────────────────────────────────────────────
// Givens — profile state
// ────────────────────────────────────────────────────────────────────────────

Given("the server is running in demo mode", async ({ page }) => {
  getState(page).setupMode = "demo";
});

Given("the server is running in production mode", async ({ page }) => {
  getState(page).setupMode = "production";
});

Given("I am on the demo profile", async ({ page }) => {
  const s = getState(page);
  s.setupMode = "demo";
  s.productionSetupComplete = false;
});

Given(
  "I am on the production profile with shop name {string}",
  async ({ page }, shopName: string) => {
    const s = getState(page);
    s.setupMode = "production";
    s.productionSetupComplete = true;
    s.shopName = shopName;
  },
);

Given("production setup has not been completed", async ({ page }) => {
  getState(page).productionSetupComplete = false;
});

Given("production setup has been completed", async ({ page }) => {
  getState(page).productionSetupComplete = true;
});

Given(
  "production setup has been completed with shop name {string}",
  async ({ page }, shopName: string) => {
    const s = getState(page);
    s.productionSetupComplete = true;
    s.shopName = shopName;
  },
);

// ────────────────────────────────────────────────────────────────────────────
// Givens — navigation state
// ────────────────────────────────────────────────────────────────────────────

Given("the app shell is loaded", async ({ page }) => {
  await navigateToApp(page);
  await expect(page.getByTestId("profile-switcher-trigger")).toBeVisible();
});

Given("the demo banner is visible", async ({ page }) => {
  const s = getState(page);
  s.setupMode = "demo";
  await navigateToApp(page);
  await expect(page.getByTestId("demo-banner")).toBeVisible();
});

Given("the profile dropdown is open", async ({ page }) => {
  await navigateToApp(page);
  await page.getByTestId("profile-switcher-trigger").click();
  await expect(page.getByTestId("profile-dropdown")).toBeVisible();
});

Given(/^the sidebar is in collapsed\/icon-only mode$/, async ({ page }) => {
  await navigateToApp(page);
  const trigger = page.getByRole("button", { name: /toggle sidebar/i });
  if (await trigger.isVisible()) {
    await trigger.click();
    await expect(page.getByTestId("profile-switcher-text")).not.toBeVisible();
  }
});

Given("I triggered a profile switch from the dropdown", async ({ page }) => {
  const s = getState(page);
  s.setupMode = "demo";
  s.productionSetupComplete = true;
  await applySetupStatusMock(page);
  await interceptSwitchRoute(page, s, "production");
  await page.goto("/");
  await page.waitForLoadState("networkidle");
  await page.getByTestId("profile-switcher-trigger").click();
  await expect(page.getByTestId("profile-dropdown")).toBeVisible();
  // Click production entry to trigger switch
  s.setupMode = "production";
  await applySetupStatusMock(page);
  await page.getByTestId("profile-entry-production").click();
});

// ────────────────────────────────────────────────────────────────────────────
// Whens
// ────────────────────────────────────────────────────────────────────────────

When("the app shell loads", async ({ page }) => {
  await navigateToApp(page);
});

When("I click the sidebar header trigger", async ({ page }) => {
  await page.getByTestId("profile-switcher-trigger").click();
});

When("I click the {string} production entry", async ({ page }, _label: string) => {
  const s = getState(page);
  await interceptSwitchRoute(page, s, "production");
  s.setupMode = "production";
  s.productionSetupComplete = true;
  s.shopName = s.shopName ?? "Gary's Printing Co";
  await applySetupStatusMock(page);
  await page.getByTestId("profile-entry-production").click();
});

When("I click the {string} demo entry", async ({ page }, _label: string) => {
  const s = getState(page);
  // Record any unexpected switch POST — clicking the active demo entry should not fire one
  await page.route(PROFILE_SWITCH_ROUTE, async (route) => {
    s.switchRequests.push(route.request().postDataJSON() as unknown);
    await route.fulfill({
      status: 403,
      contentType: "application/json",
      body: JSON.stringify({ error: "unexpected" }),
    });
  });
  await page.getByTestId("profile-entry-demo").click();
});

When("I click a profile entry", async ({ page }) => {
  // Click the inactive profile entry and intercept the switch to observe spinner
  const s = getState(page);
  const targetId = s.setupMode === "demo" ? "profile-entry-production" : "profile-entry-demo";
  if (targetId === "profile-entry-production") {
    s.productionSetupComplete = true;
    await applySetupStatusMock(page);
  }
  await interceptSwitchRoute(page, s, "production", 1000);
  await page.getByTestId(targetId).click();
});

When("the switch completes successfully", async ({ page }) => {
  await page.waitForLoadState("networkidle");
});

When("I reload the page", async ({ page }) => {
  await applySetupStatusMock(page);
  await page.reload();
  await page.waitForLoadState("networkidle");
});

When("I click the banner CTA button", async ({ page }) => {
  getState(page).urlBeforeClick = page.url();
  await page.getByTestId("demo-banner-cta").click();
});

When("I click the banner CTA", async ({ page }) => {
  await page.getByTestId("demo-banner-cta").click();
});

// ────────────────────────────────────────────────────────────────────────────
// Thens — banner
// ────────────────────────────────────────────────────────────────────────────

Then("no demo banner is visible", async ({ page }) => {
  await expect(page.getByTestId("demo-banner")).not.toBeVisible();
});

Then("there is no dismiss or close button on the banner", async ({ page }) => {
  const banner = page.getByTestId("demo-banner");
  await expect(
    banner.locator('button[aria-label*="ismiss"], button[aria-label*="lose"]'),
  ).not.toBeAttached();
});

Then("the demo banner is still visible", async ({ page }) => {
  await expect(page.getByTestId("demo-banner")).toBeVisible();
});

Then("the banner CTA reads {string}", async ({ page }, text: string) => {
  await expect(page.getByTestId("demo-banner-cta")).toHaveText(text);
});

Then("I am still on the same page", async ({ page }) => {
  const { urlBeforeClick } = getState(page);
  if (urlBeforeClick) {
    expect(page.url()).toBe(urlBeforeClick);
  }
});

Then("I have not been navigated to Settings", async ({ page }) => {
  expect(page.url()).not.toContain("/settings");
});

// ────────────────────────────────────────────────────────────────────────────
// Thens — profile switcher
// ────────────────────────────────────────────────────────────────────────────

Then("the sidebar header shows {string}", async ({ page }, text: string) => {
  await expect(page.getByTestId("profile-switcher-text")).toHaveText(text);
});

Then("the sidebar header now shows {string}", async ({ page }, text: string) => {
  await expect(page.getByTestId("profile-switcher-text")).toHaveText(text);
});

Then("the profile dropdown opens", async ({ page }) => {
  await expect(page.getByTestId("profile-dropdown")).toBeVisible();
});

Then("the profile dropdown closes", async ({ page }) => {
  await expect(page.getByTestId("profile-dropdown")).not.toBeVisible();
});

Then("the sidebar profile switcher dropdown opens", async ({ page }) => {
  await expect(page.getByTestId("profile-dropdown")).toBeVisible();
});

Then("the sidebar profile switcher dropdown opens automatically", async ({ page }) => {
  await expect(page.getByTestId("profile-dropdown")).toBeVisible();
});

Then("the profile switcher trigger text and chevron are hidden", async ({ page }) => {
  await expect(page.getByTestId("profile-switcher-text")).not.toBeVisible();
  await expect(page.getByTestId("profile-switcher-chevron")).not.toBeVisible();
});

Then("only the logo icon is visible", async ({ page }) => {
  await expect(page.getByTestId("profile-switcher-trigger").locator("img").first()).toBeVisible();
});

Then("I see an entry for {string}", async ({ page }, name: string) => {
  const entry =
    name.toLowerCase().includes("mokumo") || name === "Mokumo Software"
      ? page.getByTestId("profile-entry-demo")
      : page.getByTestId("profile-entry-production");
  await expect(entry).toBeVisible();
  await expect(entry).toContainText(name);
});

Then("that entry has a {string} badge", async ({ page }, _text: string) => {
  await expect(page.getByTestId("demo-badge")).toBeVisible();
});

Then("that entry has no badge", async ({ page }) => {
  await expect(
    page.getByTestId("profile-entry-production").getByTestId("demo-badge"),
  ).not.toBeAttached();
});

Then("I see a {string} entry instead of a production shop name", async ({ page }, text: string) => {
  await expect(page.getByTestId("profile-entry-production")).toContainText(text);
});

Then("the {string} entry has a checkmark indicator", async ({ page }, entryName: string) => {
  const testId = entryName.toLowerCase().includes("mokumo")
    ? "profile-entry-checkmark-demo"
    : "profile-entry-checkmark-production";
  await expect(page.getByTestId(testId)).toBeVisible();
});

Then("the production entry has no checkmark", async ({ page }) => {
  await expect(page.getByTestId("profile-entry-checkmark-production")).not.toBeAttached();
});

Then("a profile switch request is sent for the production profile", async ({ page }) => {
  const s = getState(page);
  expect(s.switchRequests).toHaveLength(1);
  expect((s.switchRequests[0] as { profile?: unknown })?.profile).toBe("production");
  await page.waitForURL((url) => url.pathname === "/", { timeout: 5000 });
});

Then("the app reloads to {string}", async ({ page }, path: string) => {
  await page.waitForURL((url) => url.pathname === path, { timeout: 5000 });
});

Then("no profile switch request is sent", async ({ page }) => {
  const s = getState(page);
  expect(s.switchRequests).toHaveLength(0);
  await expect(page.getByTestId("profile-dropdown")).not.toBeVisible();
});

Then("the dropdown closes", async ({ page }) => {
  await expect(page.getByTestId("profile-dropdown")).not.toBeVisible();
});

Then("a spinner appears on that entry", async ({ page }) => {
  await expect(page.getByTestId("profile-switch-spinner")).toBeVisible();
});

Then("both entries are disabled", async ({ page }) => {
  await expect(page.getByTestId("profile-entry-demo")).toHaveAttribute("data-disabled", "");
  await expect(page.getByTestId("profile-entry-production")).toHaveAttribute("data-disabled", "");
});

Then("the dropdown is closed", async ({ page }) => {
  await expect(page.getByTestId("profile-dropdown")).not.toBeVisible();
});
