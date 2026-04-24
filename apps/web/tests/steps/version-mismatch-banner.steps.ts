import type { Page, Route } from "@playwright/test";
import { expect } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";

// Matches the Rust-side const pinned by the vitest drift guard. Kept as
// a step-file literal because the Playwright BDD harness has no ts-rs
// import path — drift between this literal and `kikan_types::API_VERSION`
// is caught by `apps/web/src/lib/stores/version-check.test.ts` (which
// regex-greps the Rust const and compares to the Vite define), so any
// three-way drift surfaces as a loud unit-test failure, not a flaky
// browser run.
const UI_BUILT_FOR = "1.0.0";

const KIKAN_VERSION_ROUTE = "**/api/kikan-version";

type MockKikanVersion = {
  api_version: string;
  engine_version?: string;
  engine_commit?: string;
  schema_versions?: Record<string, string>;
};

async function mockKikanVersion(page: Page, body: MockKikanVersion) {
  await page.route(KIKAN_VERSION_ROUTE, async (route: Route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        engine_version: "0.1.0",
        engine_commit: "abcdef012345",
        schema_versions: { production: "m20260321_000000_init" },
        ...body,
      }),
    });
  });
}

Given("the server reports kikan api_version {string}", async ({ page }, version: string) => {
  await mockKikanVersion(page, { api_version: version });
});

Given("the server reports the same kikan api_version the UI was built for", async ({ page }) => {
  await mockKikanVersion(page, { api_version: UI_BUILT_FOR });
});

When("I open the admin UI", async ({ page }) => {
  await page.goto("/");
});

Then("a version-mismatch banner is visible", async ({ page }) => {
  await expect(page.getByTestId("version-mismatch-banner")).toBeVisible();
});

Then("the banner names both the UI version and the server version", async ({ page }) => {
  const banner = page.getByTestId("version-mismatch-banner");
  await expect(banner).toContainText(UI_BUILT_FOR);
  await expect(banner).toContainText("99.0.0");
});

Then("no version-mismatch banner is visible", async ({ page }) => {
  await expect(page.getByTestId("version-mismatch-banner")).not.toBeVisible();
});
