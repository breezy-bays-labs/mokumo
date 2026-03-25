import { expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { Given, When, Then } from "../support/storybook.fixture";
import { storybookIframeUrl, toStoryId } from "../support/storybook.helpers";

// Bits UI AlertDialog renders nested portal elements, producing 2 content nodes.
const DIALOG_TITLE = '[data-slot="alert-dialog-title"]';
const DIALOG_DESCRIPTION = '[data-slot="alert-dialog-description"]';
const CANCEL_BUTTON = '[data-slot="alert-dialog-cancel"]';
const ACTION_BUTTON = '[data-slot="alert-dialog-action"]';
const STORY_ROOT = "#storybook-root";
const TRIGGER_BUTTON = `${STORY_ROOT} [data-slot="alert-dialog-trigger"]`;

/**
 * Click a button inside Bits UI AlertDialog.
 * Bits UI renders a nested overlay that intercepts Playwright's pointer events.
 * Hide the intercepting overlay, perform the click, then restore it.
 */
async function clickDialogButton(page: import("@playwright/test").Page, selector: string) {
  // Hide the nested overlay that blocks clicks
  await page.evaluate(() => {
    const overlays = document.querySelectorAll('[data-slot="alert-dialog-overlay"]');
    overlays.forEach((el) => {
      (el as HTMLElement).style.pointerEvents = "none";
    });
  });
  await page.locator(selector).first().click({ timeout: 5000 });
  // Restore overlay pointer events
  await page.evaluate(() => {
    const overlays = document.querySelectorAll('[data-slot="alert-dialog-overlay"]');
    overlays.forEach((el) => {
      (el as HTMLElement).style.pointerEvents = "";
    });
  });
  await page.waitForTimeout(300);
}

Given(
  "Storybook is showing the ConfirmDialog {word} story",
  async ({ page, storybookUrl }, variant: string) => {
    const storyId = toStoryId("confirmdialog", variant.toLowerCase());
    const url = storybookIframeUrl(storybookUrl, undefined, storyId);
    await page.goto(url);
    // Wait for the trigger button to render
    await page.locator(TRIGGER_BUTTON).first().waitFor({ state: "visible", timeout: 10000 });
    // Open the dialog by clicking the trigger
    await page.locator(TRIGGER_BUTTON).first().click();
    // Wait for dialog to animate open
    await page.getByRole("alertdialog").first().waitFor({ state: "visible", timeout: 5000 });
  },
);

When("I click the cancel button", async ({ page }) => {
  await clickDialogButton(page, CANCEL_BUTTON);
});

When("confirmation is triggered with a slow operation", async ({ page }) => {
  await clickDialogButton(page, ACTION_BUTTON);
  // Wait for loading spinner
  await page.locator(`${ACTION_BUTTON} svg`).first().waitFor({ state: "visible", timeout: 5000 });
});

When("confirmation is triggered with a successful operation", async ({ page }) => {
  await clickDialogButton(page, ACTION_BUTTON);
});

When("confirmation is triggered with a failing operation", async ({ page }) => {
  await clickDialogButton(page, ACTION_BUTTON);
  await page.waitForTimeout(300);
});

When("I press the Escape key", async ({ page }) => {
  await page.keyboard.press("Escape");
});

When("I run an accessibility scan", async ({ page }) => {
  await expect(page.getByRole("alertdialog").first()).toBeVisible();
});

Then("the dialog title is visible", async ({ page }) => {
  await expect(page.locator(DIALOG_TITLE).first()).toBeVisible();
});

Then("the dialog description is visible", async ({ page }) => {
  await expect(page.locator(DIALOG_DESCRIPTION).first()).toBeVisible();
});

Then("a cancel button is visible", async ({ page }) => {
  await expect(page.locator(CANCEL_BUTTON).first()).toBeVisible();
});

Then("an action button is visible", async ({ page }) => {
  await expect(page.locator(ACTION_BUTTON).first()).toBeVisible();
});

Then("the action button has destructive variant styling", async ({ page }) => {
  const actionButton = page.locator(ACTION_BUTTON).first();
  await expect(actionButton).toBeVisible();
  const classes = await actionButton.getAttribute("class");
  expect(classes).toContain("destructive");
});

Then("the dialog is no longer visible", async ({ page }) => {
  // Bits UI keeps portal elements in DOM; check that visible dialog content disappears
  await expect(page.locator('[data-slot="alert-dialog-content"][data-state="open"]')).toHaveCount(
    0,
    { timeout: 5000 },
  );
});

Then("the action button shows a loading spinner", async ({ page }) => {
  const spinner = page.locator(`${ACTION_BUTTON} svg`).first();
  await expect(spinner).toBeVisible({ timeout: 5000 });
});

Then("the cancel button is disabled", async ({ page }) => {
  await expect(page.locator(CANCEL_BUTTON).first()).toBeDisabled();
});

Then("the action button is disabled", async ({ page }) => {
  await expect(page.locator(ACTION_BUTTON).first()).toBeDisabled();
});

Then("an error message is visible in the dialog", async ({ page }) => {
  // The error message contains the text from the rejected promise
  const errorMsg = page.getByText("Network error");
  await expect(errorMsg.first()).toBeVisible({ timeout: 5000 });
});

Then("the dialog is still open", async ({ page }) => {
  await expect(page.getByRole("alertdialog").first()).toBeVisible();
});

Then("no critical accessibility violations are found", async ({ page }) => {
  const dialog = page.getByRole("alertdialog").first();
  const dialogId = await dialog.getAttribute("id");
  const results = await new AxeBuilder({ page }).include(`#${dialogId}`).analyze();
  const critical = results.violations.filter(
    (v) => v.impact === "critical" || v.impact === "serious",
  );
  expect(
    critical,
    `Found ${critical.length} critical/serious a11y violation(s): ${critical.map((v) => v.id).join(", ")}`,
  ).toHaveLength(0);
});
