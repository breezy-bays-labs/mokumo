import { expect, type Page } from "@playwright/test";
import { Given, When, Then } from "../support/app.fixture";

const errorMocked = new WeakMap<Page, boolean>();
const logoutResponseUrl = new WeakMap<Page, string>();

Given("the logout endpoint will return a server error", async ({ page }) => {
  errorMocked.set(page, true);
  await page.route("**/api/auth/logout", async (route) => {
    await route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({
        code: "internal_error",
        message: "Session store unavailable",
        details: null,
      }),
    });
  });
});

When("the user opens the avatar popover", async ({ page }) => {
  await page.locator("[data-testid='user-menu-trigger']").click();
  await expect(page.locator("[data-testid='logout-button']")).toBeVisible();
});

When('the user clicks "Log out"', async ({ page }) => {
  // Only mock success if the error scenario hasn't already mocked an error response
  if (!errorMocked.get(page)) {
    await page.route("**/api/auth/logout", async (route) => {
      await route.fulfill({ status: 204 });
    });
  }

  // Mock /login route so SvelteKit doesn't 404
  await page.route("**/login", async (route) => {
    if (route.request().resourceType() === "document") {
      await route.fulfill({
        status: 200,
        contentType: "text/html",
        body: "<html><body>Login Page</body></html>",
      });
    } else {
      await route.continue();
    }
  });

  // Wait for the logout response deterministically (not a fixed sleep)
  const responsePromise = page.waitForResponse(
    (r) => r.url().includes("/api/auth/logout") && r.request().method() === "POST",
  );

  const logoutButton = page.locator("[data-testid='logout-button']");
  await logoutButton.click();
  const response = await responsePromise;
  logoutResponseUrl.set(page, new URL(response.url()).pathname);
});

Then("a POST request was sent to {string}", async ({ page }, endpoint: string) => {
  const url = logoutResponseUrl.get(page);
  expect(url, `Expected a POST request to ${endpoint}`).toBe(endpoint);
});

Then("the page navigates to {string}", async ({ page }, path: string) => {
  await page.waitForURL(`**${path}`, { timeout: 5_000 });
  expect(new URL(page.url()).pathname).toBe(path);
});

Then("an error toast is shown with text {string}", async ({ page }, message: string) => {
  const toastEl = page.locator("[data-sonner-toast][data-type='error']");
  await expect(toastEl).toBeVisible({ timeout: 5_000 });
  await expect(toastEl).toContainText(message);
});

Then("the page does not navigate to {string}", async ({ page }, path: string) => {
  expect(new URL(page.url()).pathname).not.toBe(path);
});
