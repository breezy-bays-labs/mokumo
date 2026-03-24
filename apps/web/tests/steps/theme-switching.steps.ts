import { expect } from "@playwright/test";
import { When, Then } from "../support/storybook.fixture";
import {
  extractOklchLightness,
  gotoStory,
  rootHasClass,
  toThemeSlug,
} from "../support/storybook.helpers";

/**
 * Expected --primary lightness per theme (0-1 range).
 * Some themes use different primary in dark mode.
 */
const THEME_PRIMARY_LIGHTNESS: Record<string, { light: number; dark: number }> = {
  niji: { light: 0.56, dark: 0.745 },
  tangerine: { light: 0.6397, dark: 0.6397 },
  "midnight-bloom": { light: 0.5676, dark: 0.5676 },
  "solar-dusk": { light: 0.5553, dark: 0.7049 },
  "soft-pop": { light: 0.5106, dark: 0.6801 },
  "sunset-horizon": { light: 0.7357, dark: 0.7357 },
};

When("I select the {string} theme", async ({ page, storybookUrl }, theme: string) => {
  const isDark = await rootHasClass(page, "dark");
  const globals: Record<string, string> = { theme: toThemeSlug(theme) };
  if (isDark) globals.mode = "dark";

  await gotoStory(page, storybookUrl, globals);
});

Then(
  /the "(.*)" CSS variable changes to the (.*) value/,
  async ({ page }, varName: string, theme: string) => {
    const slug = toThemeSlug(theme);
    const themeLightness = THEME_PRIMARY_LIGHTNESS[slug];
    expect(themeLightness, `Unknown theme: ${slug}`).toBeDefined();

    // Single evaluate to get both mode and CSS value
    const { isDark, value } = await page.evaluate(
      (name) => ({
        isDark: document.documentElement.classList.contains("dark"),
        value: getComputedStyle(document.documentElement).getPropertyValue(name).trim(),
      }),
      varName,
    );

    const expectedLightness = isDark ? themeLightness.dark : themeLightness.light;
    const actualLightness = extractOklchLightness(value);
    expect(actualLightness, `Expected oklch for ${varName}, got: "${value}"`).not.toBeNull();
    expect(actualLightness!).toBeCloseTo(expectedLightness, 1);
  },
);

// Reuses the same assertion as "has the {string} class" in dark-mode.steps.ts
// but with "still has" wording for Gherkin readability
Then("the root element still has the {string} class", async ({ page }, className: string) => {
  expect(await rootHasClass(page, className)).toBe(true);
});

When("I open the theme switcher", async ({ page, storybookUrl }) => {
  await page.goto(storybookUrl);
  await expect(page).toHaveTitle(/storybook/i);
});

Then("{string} is listed as an option", async ({ page, storybookUrl }, theme: string) => {
  const slug = toThemeSlug(theme);
  await gotoStory(page, storybookUrl, { theme: slug });

  if (slug === "niji") {
    const hasAnyTheme = await page.evaluate(() =>
      Array.from(document.documentElement.classList).some((c) => c.startsWith("theme-")),
    );
    expect(hasAnyTheme).toBe(false);
  } else {
    expect(await rootHasClass(page, `theme-${slug}`)).toBe(true);
  }
});
