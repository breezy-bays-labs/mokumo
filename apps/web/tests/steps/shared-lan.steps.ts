import { expect } from "@playwright/test";
import { Then, When } from "../support/app.fixture";

const TOAST_SELECTOR = "[data-sonner-toast]";

When("I copy the LAN URL", async ({ page }) => {
  await page.getByRole("button", { name: "Copy LAN URL to clipboard" }).click();
});

Then("I see the LAN URL {string}", async ({ page }, url: string) => {
  await expect(page.getByText(url).first()).toBeVisible();
});

Then("I see a {string} toast message", async ({ page }, message: string) => {
  await expect(page.locator(TOAST_SELECTOR).filter({ hasText: message }).first()).toBeVisible();
});
