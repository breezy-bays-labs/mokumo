import type { Page } from "@playwright/test";
import { expect } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";

// -- Helpers --

/** Simulate a WS shutdown message by updating the store directly. */
async function simulateShutdownMessage(page: Page) {
  await page.evaluate(() => {
    const { markShutdown } = (window as any).__wsStatusTestHelpers;
    markShutdown();
  });
}

async function simulateDisconnect(page: Page) {
  await page.evaluate(() => {
    const { markDisconnected } = (window as any).__wsStatusTestHelpers;
    markDisconnected();
  });
}

async function simulateReconnect(page: Page) {
  await page.evaluate(() => {
    const { markConnected } = (window as any).__wsStatusTestHelpers;
    markConnected();
  });
}

// -- Steps --

Given("the WebSocket connection is established", async ({ page, appUrl }) => {
  await page.goto(new URL("/", appUrl).toString());
  // Wait for the page to load and WS status helpers to be available
  await page.waitForFunction(() => (window as any).__wsStatusTestHelpers !== undefined, null, {
    timeout: 5000,
  });
});

When("the server sends a {string} message", async ({ page }, _type: string) => {
  await simulateShutdownMessage(page);
});

Then("I see a banner {string}", async ({ page }, text: string) => {
  const banner = page.getByTestId("disconnect-banner");
  await expect(banner).toBeVisible();
  await expect(banner).toContainText(text);
});

// -- Reconnection indicator --

Given("the WebSocket connection was lost", async ({ page, appUrl }) => {
  await page.goto(new URL("/", appUrl).toString());
  await page.waitForFunction(() => (window as any).__wsStatusTestHelpers !== undefined, null, {
    timeout: 5000,
  });
  await simulateDisconnect(page);
});

When("the client is attempting to reconnect", async () => {
  // Already in reconnecting state from markDisconnected
});

Then("I see a reconnection indicator", async ({ page }) => {
  await expect(page.getByTestId("reconnecting-indicator")).toBeVisible();
});

// -- Banner clears on reconnect --

Given("I see the disconnect banner", async ({ page }) => {
  await expect(page.getByTestId("disconnect-banner")).toBeVisible();
});

When("the client successfully reconnects", async ({ page }) => {
  await simulateReconnect(page);
});

Then("the disconnect banner disappears", async ({ page }) => {
  // After "Reconnected" text clears (3s), banner should disappear
  await expect(page.getByTestId("disconnect-banner")).not.toBeVisible({ timeout: 5000 });
});

Then("I see a {string} confirmation briefly", async ({ page }, text: string) => {
  await expect(page.getByTestId("reconnected-text")).toHaveText(text);
});

// -- Unexpected connection loss --

When("the connection drops unexpectedly", async ({ page }) => {
  await simulateDisconnect(page);
});

// "I see the disconnect banner" is defined as a Given above

Then("the client begins reconnecting automatically", async ({ page }) => {
  await expect(page.getByTestId("reconnecting-indicator")).toBeVisible();
});
