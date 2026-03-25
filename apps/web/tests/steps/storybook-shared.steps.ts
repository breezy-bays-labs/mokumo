import { expect } from "@playwright/test";
import { Given, When } from "../support/storybook.fixture";
import { gotoStory, toThemeSlug } from "../support/storybook.helpers";

Given("Storybook is running", async ({ page, storybookUrl }) => {
  await page.goto(storybookUrl);
  await expect(page).toHaveTitle(/storybook/i);
});

Given("Storybook is showing a component story", async ({ page, storybookUrl }) => {
  await gotoStory(page, storybookUrl);
});

Given("Storybook is showing a component in light mode", async ({ page, storybookUrl }) => {
  await gotoStory(page, storybookUrl, { mode: "light" });
});

Given("Storybook is showing a component in dark mode", async ({ page, storybookUrl }) => {
  await gotoStory(page, storybookUrl, { mode: "dark" });
});

Given(
  /Storybook is showing a component with (.+) theme/,
  async ({ page, storybookUrl }, theme: string) => {
    await gotoStory(page, storybookUrl, { theme: toThemeSlug(theme) });
  },
);

Given(
  /Storybook is showing a component in dark mode with (.+) theme/,
  async ({ page, storybookUrl }, theme: string) => {
    await gotoStory(page, storybookUrl, { mode: "dark", theme: toThemeSlug(theme) });
  },
);

When("the story renders", async ({ page }) => {
  // Generic "story renders" step — waits for the storybook root to have content.
  // Component-specific Given steps handle navigation; this step is a no-op sync point.
  const root = page.locator("#storybook-root");
  await root.waitFor({ state: "attached", timeout: 5000 });
});
