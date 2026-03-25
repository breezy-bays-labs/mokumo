import { expect } from "@playwright/test";
import { Given, When, Then } from "../support/storybook.fixture";
import { storybookIframeUrl, toStoryId } from "../support/storybook.helpers";

const TOAST_SELECTOR = "[data-sonner-toast]";
const STORY_ROOT = "#storybook-root";

Given(
  "Storybook is showing the Toast {word} story",
  async ({ page, storybookUrl }, variant: string) => {
    // Map feature-file variant names to Storybook story IDs
    const variantMap: Record<string, string> = { success: "default" };
    const storyVariant = variantMap[variant.toLowerCase()] ?? variant.toLowerCase();
    const storyId = toStoryId("toast", storyVariant);
    const url = storybookIframeUrl(storybookUrl, undefined, storyId);
    await page.goto(url);
    // Wait for the story root to have content and for the trigger button to be visible
    await page
      .locator(`${STORY_ROOT} [data-slot="button"]`)
      .first()
      .waitFor({ state: "visible", timeout: 10000 });
  },
);

When("a success toast is triggered", async ({ page }) => {
  await page.locator(`${STORY_ROOT} [data-slot="button"]`).first().click();
  await page.locator(TOAST_SELECTOR).first().waitFor({ timeout: 5000 });
});

When("an error toast is triggered", async ({ page }) => {
  await page.locator(`${STORY_ROOT} [data-slot="button"]`).first().click();
  await page.locator(TOAST_SELECTOR).first().waitFor({ timeout: 5000 });
});

When("a warning toast is triggered", async ({ page }) => {
  await page.locator(`${STORY_ROOT} [data-slot="button"]`).first().click();
  await page.locator(TOAST_SELECTOR).first().waitFor({ timeout: 5000 });
});

When("an info toast is triggered", async ({ page }) => {
  await page.locator(`${STORY_ROOT} [data-slot="button"]`).first().click();
  await page.locator(TOAST_SELECTOR).first().waitFor({ timeout: 5000 });
});

When("multiple toasts are triggered", async ({ page }) => {
  await page.locator(`${STORY_ROOT} [data-slot="button"]`).first().click();
  // Wait for at least two toasts to be present
  await page.locator(TOAST_SELECTOR).first().waitFor({ timeout: 5000 });
  await page.waitForTimeout(500);
});

When("I click the close button on the toast", async ({ page }) => {
  // svelte-sonner renders close button when closeButton prop is set on Toaster
  const closeButton = page.locator(`${TOAST_SELECTOR} [data-close-button]`).first();
  await closeButton.waitFor({ state: "visible", timeout: 5000 });
  await closeButton.click();
});

Then("a toast notification is visible", async ({ page }) => {
  await expect(page.locator(TOAST_SELECTOR).first()).toBeVisible();
});

Then("the toast has success variant styling", async ({ page }) => {
  const toast = page.locator(TOAST_SELECTOR).first();
  await expect(toast).toBeVisible();
  const classes = await toast.getAttribute("class");
  expect(classes).toContain("bg-success");
});

Then("the toast has error variant styling", async ({ page }) => {
  const toast = page.locator(TOAST_SELECTOR).first();
  await expect(toast).toBeVisible();
  const classes = await toast.getAttribute("class");
  expect(classes).toContain("bg-error");
});

Then("the toast has warning variant styling", async ({ page }) => {
  const toast = page.locator(TOAST_SELECTOR).first();
  await expect(toast).toBeVisible();
  const classes = await toast.getAttribute("class");
  expect(classes).toContain("bg-warning");
});

Then("the toast has info variant styling", async ({ page }) => {
  const toast = page.locator(TOAST_SELECTOR).first();
  await expect(toast).toBeVisible();
  const classes = await toast.getAttribute("class");
  expect(classes).toContain("bg-muted");
});

Then("the toast notification is no longer visible", async ({ page }) => {
  await expect(page.locator(TOAST_SELECTOR)).toHaveCount(0, { timeout: 10000 });
});

Then("more than one toast notification is visible", async ({ page }) => {
  const count = await page.locator(TOAST_SELECTOR).count();
  expect(count).toBeGreaterThan(1);
});
