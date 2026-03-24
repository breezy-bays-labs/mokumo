import { When, Then } from "../support/storybook.fixture";

// S2: Theme fonts — step definitions wired as stubs (RED)
// Implementation comes in Session S2

When("I inspect the computed styles", async ({ page: _page }) => {
  throw new Error("Not implemented — S2: inspect computed styles");
});

Then("the computed font-family for body text includes a system font", async ({ page: _page }) => {
  throw new Error("Not implemented — S2: system font assertion");
});

Then("no custom woff2 font files are loaded", async ({ page: _page }) => {
  throw new Error("Not implemented — S2: no woff2 assertion");
});

Then(
  "the computed font-family for body text includes {string}",
  async ({ page: _page }, _fontName: string) => {
    throw new Error("Not implemented — S2: body font assertion");
  },
);

Then(
  "the computed font-family for monospace text includes {string}",
  async ({ page: _page }, _fontName: string) => {
    throw new Error("Not implemented — S2: monospace font assertion");
  },
);

Then(
  "the computed font-family for serif text includes {string}",
  async ({ page: _page }, _fontName: string) => {
    throw new Error("Not implemented — S2: serif font assertion");
  },
);
