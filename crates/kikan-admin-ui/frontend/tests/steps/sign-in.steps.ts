import { expect } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";
import { DEFAULT_BRANDING, mockBranding, mockSetupStatus } from "../support/mocks";

const SIGN_IN_PATH = "/admin/login";

async function gotoSignIn(page: import("@playwright/test").Page): Promise<void> {
  await page.goto(SIGN_IN_PATH);
}

Given("I am on the admin sign-in screen", async ({ page }) => {
  await mockBranding(page);
  await gotoSignIn(page);
});

When("I open the admin sign-in screen", async ({ page }) => {
  await gotoSignIn(page);
});

Given("the platform reports that no admin account exists", async ({ page }) => {
  await mockBranding(page);
  await mockSetupStatus(page, { setup_complete: false });
});

Given("the platform reports that an admin account exists", async ({ page }) => {
  await mockBranding(page);
  await mockSetupStatus(page, { setup_complete: true });
});

Then("I see an email field", async ({ page }) => {
  await expect(page.getByLabel("Email")).toBeVisible();
});

Then("I see a password field", async ({ page }) => {
  await expect(page.getByLabel("Password")).toBeVisible();
});

Then('I see a "Sign in" button', async ({ page }) => {
  await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
});

Then('I see a "Forgot password?" link', async ({ page }) => {
  await expect(page.getByRole("link", { name: "Forgot password?" })).toBeVisible();
});

Then("the page shows the configured app name", async ({ page }) => {
  await expect(page.getByText(DEFAULT_BRANDING.app_name)).toBeVisible();
});

Then("the page shows the configured shop noun in body copy", async ({ page }) => {
  await expect(page.locator("body")).toContainText(DEFAULT_BRANDING.shop_noun_singular);
});

Then('I see a "First time setup?" link', async ({ page }) => {
  await expect(page.getByRole("link", { name: /first time setup\??/i })).toBeVisible();
});

Then("the link points to the setup wizard", async ({ page }) => {
  const link = page.getByRole("link", { name: /first time setup\??/i });
  await expect(link).toHaveAttribute("href", /\/admin\/setup/);
});

Then('I do not see a "First time setup?" link', async ({ page }) => {
  // Anchor on the form so the negative assertion isn't vacuously true on a
  // bare/loading page.
  await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
  await expect(page.getByRole("link", { name: /first time setup\??/i })).toHaveCount(0);
});
