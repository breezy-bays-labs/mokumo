import { expect } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";
// `When` is used by "I submit the form" below; `Given` covers the
// platform-reachability state changes (playwright-bdd matches Given/When/Then
// by step text, so a single keyword wires both `When` and `And` lines).
import { mockBranding, mockOffline, mockOnline } from "../support/mocks";

/**
 * Steps that recur across multiple features (branding seed, network-loss
 * triggers, the self-healing banner contract). Defining them here once
 * prevents playwright-bdd "multiple matching step definitions" errors.
 */

Given("the platform reports a branding configuration", async ({ page }) => {
  await mockBranding(page);
});

Given("the platform reports a branding configuration with a custom shop noun", async ({ page }) => {
  await mockBranding(page, {
    shop_noun_singular: "kiln",
    shop_noun_plural: "kilns",
  });
});

Given("the platform is unreachable", async ({ page }) => {
  await mockOffline(page);
});

Given("the platform becomes unreachable", async ({ page }) => {
  await mockOffline(page);
});

Given("the platform becomes reachable again", async ({ page }) => {
  await mockOnline(page);
});

Then("I see a self-healing banner that the connection is being retried", async ({ page }) => {
  await expect(page.getByTestId("self-healing-banner")).toBeVisible();
  await expect(page.getByTestId("self-healing-banner")).toContainText(/retry|reconnect/i);
});

Then("my form values are preserved", async ({ page }) => {
  const inputs = page.locator("input:not([type=hidden]):not([type=submit])");
  await expect(inputs.first()).toBeVisible();
  await expect(inputs.first()).toHaveValue(/.+/);
});

Then("the chrome surfaces use the branded color tokens", async ({ page }) => {
  const root = page.locator("html, body").first();
  const accent = await root.evaluate((el) =>
    getComputedStyle(el).getPropertyValue("--brand-accent").trim(),
  );
  expect(accent).not.toBe("");
});

Given("I have entered an email and password", async ({ page }) => {
  await page.getByLabel("Email").fill("admin@example.com");
  await page.getByLabel("Password").fill("hunter2hunter2");
});

When("I submit the form", async ({ page }) => {
  const form = page.locator("form").first();
  await form
    .locator(
      'button[type="submit"], button:has-text("Sign in"), button:has-text("Continue"), button:has-text("Send"), button:has-text("Submit")',
    )
    .first()
    .click();
});

Then("the banner is dismissed automatically", async ({ page }) => {
  await expect(
    page.locator('[data-testid="youre-set-up-banner"], [data-testid="self-healing-banner"]'),
  ).toBeHidden();
});
