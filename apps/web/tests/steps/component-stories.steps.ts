import { expect } from "@playwright/test";
import { Then } from "../support/storybook.fixture";
import { storybookIframeUrl, toStoryId } from "../support/storybook.helpers";
import type { DataTable } from "playwright-bdd";

/**
 * Wait for meaningful content to appear in the story.
 *
 * Checks two locations because Bits UI portal-based components (Dialog,
 * Select, Sheet, DropdownMenu) render content into `body > [data-bits-portal]`
 * rather than inside `#storybook-root`.
 *
 * Uses `waitForFunction` with a timeout so async rendering (Svelte effects,
 * context providers) has time to settle.
 */
async function storyHasContent(page: import("@playwright/test").Page): Promise<boolean> {
  try {
    await page.waitForFunction(
      () => {
        const root = document.getElementById("storybook-root");
        if (root && root.innerHTML.trim().length > 0) return true;
        const portals = document.querySelectorAll("body > [data-bits-portal]");
        for (const portal of portals) {
          if (portal.innerHTML.trim().length > 0) return true;
        }
        return false;
      },
      { timeout: 5_000 },
    );
    return true;
  } catch {
    return false;
  }
}

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

      const hasContent = await storyHasContent(page);
      expect(hasContent, `Story "${storyId}" for ${component} rendered no content`).toBe(true);
    }
  },
);
