import { When, Then } from "../support/storybook.fixture";
import type { DataTable } from "playwright-bdd";

// S4: Component stories — step definitions wired as stubs (RED)
// Implementation comes in Session S4

Then(
  "each of the following components has at least one story:",
  async ({ page: _page }, _dataTable: DataTable) => {
    throw new Error("Not implemented — S4: component story existence check");
  },
);

When("I open the accessibility panel", async ({ page: _page }) => {
  throw new Error("Not implemented — S3: a11y panel (programmatic axe-core)");
});

Then("axe-core violations are displayed at warning level", async ({ page: _page }) => {
  throw new Error("Not implemented — S3: a11y warning level assertion");
});
