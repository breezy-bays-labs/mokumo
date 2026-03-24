import { expect } from "@playwright/test";
import { When, Then } from "../support/storybook.fixture";
import { getBodyFontFamily, getCssVariableValue } from "../support/storybook.helpers";

When("I inspect the computed styles", async ({ page }) => {
  // No-op — the Given step already navigated to a story.
  // This step exists for Gherkin readability.
  await page.locator("body").waitFor();
});

Then("the computed font-family for body text includes a system font", async ({ page }) => {
  const fontFamily = await getBodyFontFamily(page);
  const systemFonts = ["ui-sans-serif", "system-ui", "-apple-system", "sans-serif", "Segoe UI"];
  const hasSystemFont = systemFonts.some((sf) => fontFamily.includes(sf));
  expect(hasSystemFont, `Expected system font in: "${fontFamily}"`).toBe(true);
});

Then("no custom woff2 font files are loaded", async ({ page }) => {
  const woff2Resources = await page.evaluate(() =>
    performance
      .getEntriesByType("resource")
      .filter((r) => r.name.endsWith(".woff2"))
      .map((r) => r.name),
  );
  expect(
    woff2Resources,
    `Expected no woff2 files, found: ${woff2Resources.join(", ")}`,
  ).toHaveLength(0);
});

Then(
  "the computed font-family for body text includes {string}",
  async ({ page }, fontName: string) => {
    const fontFamily = await getBodyFontFamily(page);
    expect(fontFamily.toLowerCase()).toContain(fontName.toLowerCase());
  },
);

Then(
  "the computed font-family for monospace text includes {string}",
  async ({ page }, fontName: string) => {
    const fontMono = await getCssVariableValue(page, "--font-mono");
    expect(fontMono.toLowerCase()).toContain(fontName.toLowerCase());
  },
);

Then(
  "the computed font-family for serif text includes {string}",
  async ({ page }, fontName: string) => {
    const fontSerif = await getCssVariableValue(page, "--font-serif");
    expect(fontSerif.toLowerCase()).toContain(fontName.toLowerCase());
  },
);
