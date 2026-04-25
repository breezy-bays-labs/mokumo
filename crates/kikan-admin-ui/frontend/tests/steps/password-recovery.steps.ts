import { expect, type Page } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";
import { mockBranding } from "../support/mocks";

const RECOVER_PATH = "/admin/recover";

async function gotoRecover(page: Page): Promise<void> {
  await mockBranding(page);
  await page.goto(RECOVER_PATH);
}

Given("I open the password-recovery wizard", async ({ page }) => {
  await gotoRecover(page);
});

Given("I am on the request-PIN step", async ({ page }) => {
  await gotoRecover(page);
  await page.getByTestId("recover-step-request-pin").click();
});

Given("I am on the new-password step with a verified PIN", async ({ page }) => {
  await gotoRecover(page);
  await page.getByTestId("recover-step-new-password").click();
});

Given("I have entered a recovery email", async ({ page }) => {
  await page.getByLabel("Email").fill("admin@example.com");
});

When("I enter a password that violates a strength rule", async ({ page }) => {
  await page.getByLabel(/new password/i).fill("a");
});

Then("I see a three-step progress indicator", async ({ page }) => {
  await expect(page.getByTestId("recover-progress")).toBeVisible();
  await expect(page.getByTestId("recover-progress").locator("[data-step]")).toHaveCount(3);
});

Then('the steps are "Request PIN", "Enter PIN", and "New password"', async ({ page }) => {
  const steps = page.getByTestId("recover-progress").locator("[data-step]");
  await expect(steps.nth(0)).toContainText(/request pin/i);
  await expect(steps.nth(1)).toContainText(/enter pin/i);
  await expect(steps.nth(2)).toContainText(/new password/i);
});

Then("I see which rule failed", async ({ page }) => {
  await expect(page.getByTestId("password-strength-error")).toBeVisible();
});

Then("the submit button stays disabled until the rule passes", async ({ page }) => {
  await expect(page.getByRole("button", { name: /set password|update password/i })).toBeDisabled();
});
