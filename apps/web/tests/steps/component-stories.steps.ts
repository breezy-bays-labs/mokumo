import { Then } from "../support/storybook.fixture";
import type { DataTable } from "playwright-bdd";

// S4: Component stories — step definitions wired as stubs (RED)
// Implementation comes in Session S4

Then(
  "each of the following components has at least one story:",
  async ({ page: _page }, _dataTable: DataTable) => {
    throw new Error("Not implemented — S4: component story existence check");
  },
);
