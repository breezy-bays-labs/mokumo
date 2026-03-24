import { expect } from "@playwright/test";
import { When, Then } from "../support/storybook.fixture";

When("I select the {string} viewport", async ({ page }, viewport: string) => {
  const width = parseInt(viewport, 10);
  await page.setViewportSize({ width, height: 900 });
});

Then("the canvas width is {int} pixels", async ({ page }, width: number) => {
  const size = page.viewportSize();
  expect(size?.width).toBe(width);
});
