import { expect } from "@playwright/test";
import { Then } from "../support/app.fixture";

Then("I see the branded error page", async ({ page }) => {
  await expect(page.locator("img[alt='Mokumo']")).toBeVisible();
});

Then("the error page shows status {string}", async ({ page }, status: string) => {
  await expect(page.getByText(status, { exact: true })).toBeVisible();
});

Then("the error page shows title {string}", async ({ page }, title: string) => {
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
});

Then("the error page has a {string} link", async ({ page }, text: string) => {
  await expect(page.getByRole("link", { name: text })).toBeVisible();
});

Then("the error page has a {string} button", async ({ page }, text: string) => {
  await expect(page.getByRole("button", { name: text })).toBeVisible();
});
