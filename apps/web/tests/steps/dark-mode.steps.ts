import { expect } from "@playwright/test";
import { When, Then } from "../support/storybook.fixture";
import {
  extractOklchLightness,
  getCssVariableValue,
  gotoStory,
  rootHasClass,
} from "../support/storybook.helpers";

When("I toggle dark mode on", async ({ page, storybookUrl }) => {
  await gotoStory(page, storybookUrl, { mode: "dark" });
});

When("I toggle dark mode off", async ({ page, storybookUrl }) => {
  await gotoStory(page, storybookUrl, { mode: "light" });
});

Then("the {string} CSS variable resolves to a dark value", async ({ page }, varName: string) => {
  const value = await getCssVariableValue(page, varName);
  const lightness = extractOklchLightness(value);
  expect(lightness, `Expected oklch value for ${varName}, got: "${value}"`).not.toBeNull();
  expect(lightness!).toBeLessThan(0.4);
});

Then("the {string} CSS variable resolves to a light value", async ({ page }, varName: string) => {
  const value = await getCssVariableValue(page, varName);
  const lightness = extractOklchLightness(value);
  expect(lightness, `Expected oklch value for ${varName}, got: "${value}"`).not.toBeNull();
  expect(lightness!).toBeGreaterThan(0.8);
});

Then("the root element has the {string} class", async ({ page }, className: string) => {
  expect(await rootHasClass(page, className)).toBe(true);
});

Then("the root element does not have the {string} class", async ({ page }, className: string) => {
  expect(await rootHasClass(page, className)).toBe(false);
});
