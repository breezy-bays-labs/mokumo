import { expect } from "@playwright/test";
import { Given, When, Then } from "../support/app.fixture";
import { mockSetupStatus } from "../support/setup-status.helpers";

// ────────────────────────────────────────────────────────────────────────────
// Givens
// ────────────────────────────────────────────────────────────────────────────

Given("the system is in production mode", async ({ page }) => {
  await mockSetupStatus(page, { setup_mode: "production", production_setup_complete: true });
});

Given("the system is in demo mode", async ({ page }) => {
  await mockSetupStatus(page, { setup_mode: "demo" });
});

// ────────────────────────────────────────────────────────────────────────────
// Whens
// ────────────────────────────────────────────────────────────────────────────

When("the profile is switched to production mode", async ({ page }) => {
  await mockSetupStatus(page, { setup_mode: "production", production_setup_complete: true });
});

// ────────────────────────────────────────────────────────────────────────────
// Thens
// ────────────────────────────────────────────────────────────────────────────

Then('I see the "Production Mode" section', async ({ page }) => {
  await expect(page.getByTestId("production-mode-section")).toBeVisible();
});

Then('I do not see the "Production Mode" section', async ({ page }) => {
  await expect(page.getByTestId("production-mode-section")).not.toBeVisible();
});

Then('I see an "Active" badge next to "Production Mode"', async ({ page }) => {
  const section = page.getByTestId("production-mode-section");
  await expect(section.getByText("Active")).toBeVisible();
});

Then('I see the "Demo Mode" section', async ({ page }) => {
  await expect(page.getByTestId("demo-mode-section")).toBeVisible();
});

Then('I do not see the "Demo Mode" section', async ({ page }) => {
  await expect(page.getByTestId("demo-mode-section")).not.toBeVisible();
});
