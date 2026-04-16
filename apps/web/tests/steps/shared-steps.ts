import { expect } from "@playwright/test";
import { Then } from "../support/app.fixture";

Then("I see the {string} card", async ({ page }, cardTitle: string) => {
  await expect(page.getByText(cardTitle)).toBeVisible();
});

Then("I see {string}", async ({ page }, text: string) => {
  await expect(page.getByText(text)).toBeVisible();
});

Then("I see a {string} button", async ({ page }, text: string) => {
  await expect(page.getByRole("button", { name: text })).toBeVisible();
});

Then("I see an {string} button", async ({ page }, text: string) => {
  await expect(page.getByRole("button", { name: text })).toBeVisible();
});
