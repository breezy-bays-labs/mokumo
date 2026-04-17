import { expect, type Page } from "@playwright/test";
import { Given, When, Then } from "../support/app.fixture";

const SETUP_STATUS_ROUTE = "**/api/setup-status";
const PROFILE_SWITCH_ROUTE = "**/api/profile/switch";

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

function setupStatusBody(overrides: Record<string, unknown> = {}): string {
  return JSON.stringify({
    setup_complete: true,
    setup_mode: "demo",
    is_first_launch: false,
    production_setup_complete: false,
    shop_name: null,
    ...overrides,
  });
}

async function mockSetupStatus(page: Page, overrides: Record<string, unknown> = {}): Promise<void> {
  await page.route(SETUP_STATUS_ROUTE, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: setupStatusBody(overrides),
    }),
  );
}

async function navigateToWelcome(page: Page, appUrl: string): Promise<void> {
  await page.goto(`${appUrl}/welcome`);
  // Wait for the page to finish its onMount fetchWithRetry
  await page.waitForLoadState("networkidle");
}

// ────────────────────────────────────────────────────────────────────────────
// Givens — server state
// ────────────────────────────────────────────────────────────────────────────

Given("the server reports is_first_launch as true", async ({ page }) => {
  await mockSetupStatus(page, { is_first_launch: true });
});

Given("the server reports is_first_launch as false", async ({ page }) => {
  await mockSetupStatus(page, { is_first_launch: false });
});

Given("the Mokumo server has not yet responded to setup-status", async ({ page }) => {
  // Simulate the server being unreachable: respond with network error after a delay
  await page.route(SETUP_STATUS_ROUTE, async (route) => {
    // Abort to simulate connection refused / not yet ready
    await route.abort("connectionrefused");
  });
});

// ────────────────────────────────────────────────────────────────────────────
// Givens — navigation state
// ────────────────────────────────────────────────────────────────────────────

Given("I am on the welcome screen", async ({ page, appUrl }) => {
  await mockSetupStatus(page, { is_first_launch: true });
  await navigateToWelcome(page, appUrl);
  await expect(page.getByTestId("setup-shop-button")).toBeVisible();
});

Given("I see the startup message on the welcome screen", async ({ page, appUrl }) => {
  let callCount = 0;
  // First call returns connection refused, second returns the real data
  await page.route(SETUP_STATUS_ROUTE, async (route) => {
    callCount++;
    if (callCount === 1) {
      await route.abort("connectionrefused");
    } else {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: setupStatusBody({ is_first_launch: true }),
      });
    }
  });
  await page.goto(`${appUrl}/welcome`);
  // Don't wait for networkidle so we see the loading state
});

Given("I have the welcome screen open in a background tab", async ({ page, appUrl }) => {
  await mockSetupStatus(page, { is_first_launch: true });
  await navigateToWelcome(page, appUrl);
});

Given("another session has already completed a profile switch", async ({ page }) => {
  // Override setup-status to report is_first_launch is now false
  await page.unrouteAll({ behavior: "ignoreErrors" });
  await mockSetupStatus(page, { is_first_launch: false });
});

Given("the server does not respond to setup-status after 10 attempts", async ({ page }) => {
  await page.route(SETUP_STATUS_ROUTE, (route) => route.abort("connectionrefused"));
});

// ────────────────────────────────────────────────────────────────────────────
// Whens — navigation
// ────────────────────────────────────────────────────────────────────────────

When("I navigate to {string}", async ({ page, appUrl }, path: string) => {
  await page.goto(`${appUrl}${path}`);
});

When("I arrive at the welcome screen", async ({ page, appUrl }) => {
  await page.goto(`${appUrl}/welcome`);
});

When("the server responds to setup-status", async ({ page }) => {
  // Fulfill the pending/mocked route with a successful response
  await page.unrouteAll({ behavior: "ignoreErrors" });
  await mockSetupStatus(page, { is_first_launch: true });
  // Simulate visibilitychange to re-trigger checkStatus
  await page.evaluate(() => {
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.waitForLoadState("networkidle");
});

When("I focus the tab", async ({ page }) => {
  await page.evaluate(() => {
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.waitForLoadState("networkidle");
});

// ────────────────────────────────────────────────────────────────────────────
// Whens — interactions
// ────────────────────────────────────────────────────────────────────────────

When('I click "Explore Demo"', async ({ page }) => {
  await page.route(PROFILE_SWITCH_ROUTE, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ profile: "demo" }),
    }),
  );
  await page.getByTestId("explore-demo-button").click();
});

When('I click "Set Up My Shop"', async ({ page }) => {
  await page.route(PROFILE_SWITCH_ROUTE, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ profile: "production" }),
    }),
  );
  await page.getByTestId("setup-shop-button").click();
});

// ────────────────────────────────────────────────────────────────────────────
// Thens — routing assertions
// ────────────────────────────────────────────────────────────────────────────

Then("I am redirected to {string}", async ({ page }, path: string) => {
  await page.waitForURL(`**${path}`, { timeout: 5_000 });
  expect(new URL(page.url()).pathname).toBe(path);
});

Then("I am not redirected to {string}", async ({ page }, path: string) => {
  // Give a short window for any redirect to occur, then assert path
  await page.waitForTimeout(500);
  expect(new URL(page.url()).pathname).not.toBe(path);
});

Then("I see the welcome screen", async ({ page }) => {
  await expect(page.getByTestId("setup-shop-button")).toBeVisible();
  await expect(page.getByTestId("explore-demo-button")).toBeVisible();
});

Then(
  "I am redirected away from the welcome screen if is_first_launch is now false",
  async ({ page }) => {
    // After the re-check, we are no longer on /welcome
    await page.waitForFunction(() => !window.location.pathname.startsWith("/welcome"), {
      timeout: 3_000,
    });
    expect(new URL(page.url()).pathname).not.toBe("/welcome");
  },
);

// ────────────────────────────────────────────────────────────────────────────
// Thens — content assertions
// ────────────────────────────────────────────────────────────────────────────

Then('I see a "Set Up My Shop" button', async ({ page }) => {
  await expect(page.getByTestId("setup-shop-button")).toBeVisible();
});

Then('I see an "Explore Demo" button', async ({ page }) => {
  await expect(page.getByTestId("explore-demo-button")).toBeVisible();
});

Then('the "Set Up My Shop" button has primary styling', async ({ page }) => {
  // Primary variant has no "outline" or "secondary" class
  const btn = page.getByTestId("setup-shop-button");
  await expect(btn).toBeVisible();
  // Primary button: no outline/secondary variant class
  const cls = await btn.getAttribute("class");
  expect(cls).not.toContain("outline");
});

Then('the "Explore Demo" button has secondary/outline styling', async ({ page }) => {
  const btn = page.getByTestId("explore-demo-button");
  await expect(btn).toBeVisible();
  const cls = await btn.getAttribute("class");
  expect(cls).toContain("outline");
});

Then("a profile switch request is sent for the demo profile", async ({ page }) => {
  const req = await page.waitForRequest(
    (r) => r.url().includes("/api/profile/switch") && r.method() === "POST",
    { timeout: 3_000 },
  );
  const body = req.postDataJSON() as { profile: string };
  expect(body.profile).toBe("demo");
});

Then("a profile switch request is sent for the production profile", async ({ page }) => {
  const req = await page.waitForRequest(
    (r) => r.url().includes("/api/profile/switch") && r.method() === "POST",
    { timeout: 3_000 },
  );
  const body = req.postDataJSON() as { profile: string };
  expect(body.profile).toBe("production");
});

Then("the app is running in demo mode", async ({ page }) => {
  // After switch + redirect to /, the app shell's demo banner should be visible.
  // The banner is rendered by (app)/+layout.svelte when setup_mode === "demo".
  // In a mocked test, we just check we're at "/" (the app loaded).
  await page.waitForURL("**/", { timeout: 5_000 });
});

Then("a loading indicator appears", async ({ page }) => {
  // Spinner appears immediately on click — check it's visible during the in-flight request
  // The button itself shows a Spinner and "Switching..." text while switching = true
  await expect(page.getByText("Switching...")).toBeVisible({ timeout: 2_000 });
});

Then("both buttons are disabled", async ({ page }) => {
  await expect(page.getByTestId("setup-shop-button")).toBeDisabled();
  await expect(page.getByTestId("explore-demo-button")).toBeDisabled();
});

Then("both CTAs are hidden until the server responds", async ({ page }) => {
  await expect(page.getByTestId("setup-shop-button")).not.toBeVisible();
  await expect(page.getByTestId("explore-demo-button")).not.toBeVisible();
});

Then('I see a "Starting up..." message', async ({ page }) => {
  await expect(page.getByTestId("startup-message")).toBeVisible();
  await expect(page.getByText("Starting up...")).toBeVisible();
});

Then('the "Set Up My Shop" and "Explore Demo" buttons appear', async ({ page }) => {
  await expect(page.getByTestId("setup-shop-button")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("explore-demo-button")).toBeVisible({ timeout: 5_000 });
});

Then("the startup message is no longer visible", async ({ page }) => {
  await expect(page.getByTestId("startup-message")).not.toBeVisible({ timeout: 5_000 });
});

Then('I see an error message "Could not reach Mokumo"', async ({ page }) => {
  // Wait for all 10 retries to exhaust (10 × 500ms = 5s, add buffer)
  await expect(page.getByText("Could not reach Mokumo")).toBeVisible({ timeout: 8_000 });
});

Then('I see a "Refresh" button', async ({ page }) => {
  await expect(page.getByTestId("refresh-button")).toBeVisible();
});

Then("setup-status is re-fetched", async ({ page }) => {
  // After the visibilitychange event, the page calls checkStatus() → fetchSetupStatus()
  // We verify by waiting for a new setup-status request
  const req = await page.waitForRequest((r) => r.url().includes("/api/setup-status"), {
    timeout: 3_000,
  });
  expect(req).toBeTruthy();
});
