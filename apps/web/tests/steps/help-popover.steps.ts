import { expect, type Page } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";
import { DEMO_GUIDE_URL } from "../../src/lib/config/constants";

const HELP_TRIGGER = "[data-testid='help-trigger']";
const HELP_POPOVER = "[data-testid='help-popover']";

// --- Shared setup ---

async function loadAppShell(page: Page, appUrl: string) {
  await page.goto(appUrl);
  await expect(page.locator("[data-sidebar='sidebar']")).toBeVisible({ timeout: 10_000 });
}

async function openHelpPopover(page: Page, appUrl: string) {
  await loadAppShell(page, appUrl);
  await page.locator(HELP_TRIGGER).click();
  await expect(page.locator(HELP_POPOVER)).toBeVisible();
}

// --- Visibility ---

Given("the app shell is loaded", async ({ appUrl, page }) => {
  await loadAppShell(page, appUrl);
});

Then("the sidebar footer displays a help icon before the user avatar", async ({ page }) => {
  const footer = page.locator("[data-sidebar='footer']");
  const menuItems = footer.locator("[data-sidebar='menu-item']");
  const helpItem = menuItems.nth(0);
  await expect(helpItem.locator(HELP_TRIGGER)).toBeVisible();
});

When("I hover over the help icon", async ({ page }) => {
  await page.locator(HELP_TRIGGER).hover();
});

Then("a tooltip shows {string}", async ({ page }, text: string) => {
  await expect(page.locator("[data-slot='tooltip-content']").getByText(text)).toBeVisible({
    timeout: 5_000,
  });
});

Given("the sidebar is collapsed to icon rail", async ({ appUrl, page }) => {
  await loadAppShell(page, appUrl);
  const rail = page.locator("[data-sidebar='rail']");
  await rail.click();
  await expect(page.locator("[data-slot='sidebar']")).toHaveAttribute("data-state", "collapsed");
});

Then("the help icon is still visible in the footer", async ({ page }) => {
  await expect(page.locator(HELP_TRIGGER)).toBeVisible();
});

// --- Popover ---

When("I click the help icon", async ({ page }) => {
  await page.locator(HELP_TRIGGER).click();
});

Then("a help popover appears", async ({ page }) => {
  await expect(page.locator(HELP_POPOVER)).toBeVisible();
});

Then("the popover heading is {string}", async ({ page }, heading: string) => {
  await expect(page.locator(HELP_POPOVER).getByText(heading).first()).toBeVisible();
});

Then("the popover contains an {string} button", async ({ page }, buttonText: string) => {
  await expect(page.locator(HELP_POPOVER).getByRole("link", { name: buttonText })).toBeVisible();
});

Then("the popover shows a {string} note", async ({ page }, noteText: string) => {
  await expect(page.locator("[data-testid='internet-note']")).toContainText(noteText);
});

Given("the help popover is open", async ({ appUrl, page }) => {
  await openHelpPopover(page, appUrl);
});

When("I click outside the popover", async ({ page }) => {
  await page.locator("body").click({ position: { x: 0, y: 0 } });
});

Then("the help popover closes", async ({ page }) => {
  await expect(page.locator(HELP_POPOVER)).not.toBeVisible();
});

When("I press Escape on the help popover", async ({ page }) => {
  await page.keyboard.press("Escape");
});

// --- External Navigation ---

When("I click the Open Demo Guide link", async ({ page }) => {
  await page.locator(HELP_POPOVER).getByRole("link", { name: "Open Demo Guide" }).click();
});

Then("a new browser tab opens with the demo guide URL", async ({ page }) => {
  const link = page.locator("[data-testid='open-demo-guide']");
  await expect(link).toHaveAttribute("href", DEMO_GUIDE_URL);
  await expect(link).toHaveAttribute("target", "_blank");
});
