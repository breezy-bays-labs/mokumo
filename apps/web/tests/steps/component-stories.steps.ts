import { expect } from "@playwright/test";
import { Then } from "../support/storybook.fixture";
import { storybookIframeUrl, toStoryId } from "../support/storybook.helpers";
import type { DataTable } from "playwright-bdd";

Then(
  "each of the following components has at least one story:",
  async ({ page, storybookUrl }, dataTable: DataTable) => {
    const rows = dataTable.rows();
    for (const [component] of rows) {
      const storyId = toStoryId(component);
      const url = storybookIframeUrl(storybookUrl, undefined, storyId);
      const response = await page.goto(url, { waitUntil: "load" });
      expect(
        response?.ok(),
        `Story "${storyId}" for ${component} did not load (HTTP ${response?.status()})`,
      ).toBe(true);

      await page.locator("#storybook-root").waitFor({ state: "attached", timeout: 5000 });
    }
  },
);
