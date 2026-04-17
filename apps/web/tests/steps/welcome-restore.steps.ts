import { expect, type Page, type FileChooser } from "@playwright/test";
import { Given, When, Then } from "../support/app.fixture";

const VALIDATE_ROUTE = "**/api/shop/restore/validate";
const RESTORE_ROUTE = "**/api/shop/restore";
const SETUP_STATUS_ROUTE = "**/api/setup-status";

// ────────────────────────────────────────────────────────────────────────────
// Cross-step state
// ────────────────────────────────────────────────────────────────────────────

type RestoreWorld = {
  fileChooserPromise?: Promise<FileChooser>;
  resolveRestoreWith?: (status: number, body: object) => void;
  capturedDisabledState?: { setup: boolean; demo: boolean; open: boolean };
};

const world = new WeakMap<Page, RestoreWorld>();

function getWorld(page: Page): RestoreWorld {
  if (!world.has(page)) world.set(page, {});
  return world.get(page)!;
}

// ────────────────────────────────────────────────────────────────────────────
// Mock helpers
// ────────────────────────────────────────────────────────────────────────────

async function mockSetupStatus(page: Page): Promise<void> {
  await page.route(SETUP_STATUS_ROUTE, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        setup_complete: true,
        setup_mode: "demo",
        is_first_launch: true,
        production_setup_complete: false,
        shop_name: null,
      }),
    }),
  );
}

async function mockValidateSuccess(page: Page): Promise<void> {
  await page.route(VALIDATE_ROUTE, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        file_name: "shop-backup.db",
        file_size: 102400,
        schema_version: "42",
      }),
    }),
  );
}

async function mockValidateFailure(page: Page, code: string): Promise<void> {
  await page.route(VALIDATE_ROUTE, (route) =>
    route.fulfill({
      status: 422,
      contentType: "application/json",
      body: JSON.stringify({ code, message: code, details: null }),
    }),
  );
}

async function mockRestoreSuccess(page: Page): Promise<void> {
  await page.route(RESTORE_ROUTE, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({}),
    }),
  );
}

async function mockRestoreFailure(page: Page): Promise<void> {
  await page.route(RESTORE_ROUTE, (route) =>
    route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ code: "restore_failed", message: "restore_failed", details: null }),
    }),
  );
}

const FAKE_DB = {
  name: "shop-backup.db",
  mimeType: "application/octet-stream",
  buffer: Buffer.from("SQLite format 3\0".padEnd(100, "\0")),
};

// Navigate from welcome to restore page, returning the file chooser.
// Must set up the file chooser listener BEFORE the click that triggers it.
async function navigateToRestore(page: Page, appUrl: string): Promise<FileChooser> {
  await mockSetupStatus(page);
  const fcPromise = page.waitForEvent("filechooser");
  await page.goto(`${appUrl}/welcome`);
  await page.waitForSelector("[data-testid='setup-shop-button']");
  await page.getByTestId("open-existing-shop-button").click();
  await page.waitForURL("**/welcome/restore");
  return fcPromise;
}

// Navigate to restore page and set files — waits for valid-state.
async function reachValidState(page: Page, appUrl: string): Promise<void> {
  await mockValidateSuccess(page);
  const fc = await navigateToRestore(page, appUrl);
  await fc.setFiles([FAKE_DB]);
  await expect(page.getByTestId("valid-state")).toBeVisible({ timeout: 8_000 });
}

// ────────────────────────────────────────────────────────────────────────────
// Givens
// ────────────────────────────────────────────────────────────────────────────

Given("the file picker is open from {string}", async ({ page, appUrl }, label: string) => {
  if (label !== "Open Existing Shop") throw new Error(`Unknown label: "${label}"`);

  await mockSetupStatus(page);
  const w = getWorld(page);
  w.fileChooserPromise = page.waitForEvent("filechooser");
  await page.goto(`${appUrl}/welcome`);
  await page.waitForSelector("[data-testid='setup-shop-button']");
  await page.getByTestId("open-existing-shop-button").click();
  await page.waitForURL("**/welcome/restore");
});

Given("I selected a .db file via the file picker", async ({ page, appUrl }) => {
  // Stall validate so we can observe the validating state
  await page.route(VALIDATE_ROUTE, () => {
    // Intentionally never fulfils — keeps the component in validating state for this test.
  });
  const fc = await navigateToRestore(page, appUrl);
  await fc.setFiles([FAKE_DB]);
  await expect(page.getByTestId("validating-state")).toBeVisible({ timeout: 8_000 });
});

Given("I selected a valid Mokumo .db file", async ({ page, appUrl }) => {
  await mockValidateSuccess(page);
  const fc = await navigateToRestore(page, appUrl);
  await fc.setFiles([FAKE_DB]);
});

Given("I selected a non-Mokumo .db file", async ({ page, appUrl }) => {
  await mockValidateFailure(page, "not_mokumo_database");
  const fc = await navigateToRestore(page, appUrl);
  await fc.setFiles([FAKE_DB]);
});

Given("I selected a corrupt .db file", async ({ page, appUrl }) => {
  await mockValidateFailure(page, "database_corrupt");
  const fc = await navigateToRestore(page, appUrl);
  await fc.setFiles([FAKE_DB]);
});

Given("I selected a .db file from a newer Mokumo version", async ({ page, appUrl }) => {
  await mockValidateFailure(page, "schema_incompatible");
  const fc = await navigateToRestore(page, appUrl);
  await fc.setFiles([FAKE_DB]);
});

Given("validation has failed for a selected file", async ({ page, appUrl }) => {
  await mockValidateFailure(page, "not_mokumo_database");
  const fc = await navigateToRestore(page, appUrl);
  await fc.setFiles([FAKE_DB]);
  await expect(page.getByTestId("invalid-state")).toBeVisible({ timeout: 8_000 });
});

Given("I see the confirmation screen with a valid file", async ({ page, appUrl }) => {
  await reachValidState(page, appUrl);
});

Given("I clicked {string}", async ({ page, appUrl }, label: string) => {
  if (label !== "Import and Restart") throw new Error(`Unknown label: "${label}"`);
  const w = getWorld(page);
  // Stall restore so When steps can decide the outcome
  await page.route(RESTORE_ROUTE, async (route) => {
    await new Promise<void>((resolve) => {
      w.resolveRestoreWith = (status, body) => {
        void route.fulfill({
          status,
          contentType: "application/json",
          body: JSON.stringify(body),
        });
        resolve();
      };
    });
  });
  await reachValidState(page, appUrl);
  await page.getByTestId("import-button").click();
  await expect(page.getByTestId("importing-state")).toBeVisible({ timeout: 8_000 });
});

Given("the restore succeeded and the server is restarting", async ({ page, appUrl }) => {
  await mockSetupStatus(page);
  await mockValidateSuccess(page);
  await mockRestoreSuccess(page);
  // Abort /login navigation so the browser stays on /welcome/restore while the
  // restart timeout fires. Aborting (vs hanging) avoids a pending-navigation
  // state that blocks Playwright's locator assertions.
  // Respond to any top-level /login navigation with 204 No Content so the
  // browser stays on /welcome/restore (per HTML spec) — lets the 15s timeout
  // timer fire on the still-mounted page without racing navigation.
  await page.route("**/login**", async (route) => {
    await route.fulfill({ status: 204 });
  });
  const fc = await navigateToRestore(page, appUrl);
  await fc.setFiles([FAKE_DB]);
  await expect(page.getByTestId("valid-state")).toBeVisible({ timeout: 8_000 });
  // Install clock before the import click (before setTimeout calls)
  await page.clock.install();
  await page.getByTestId("import-button").click();
  await expect(page.getByTestId("restarting-state")).toBeVisible({ timeout: 8_000 });
});

Given("the import has failed", async ({ page, appUrl }) => {
  await mockRestoreFailure(page);
  await reachValidState(page, appUrl);
  await page.getByTestId("import-button").click();
  await expect(page.getByTestId("import-failed-state")).toBeVisible({ timeout: 8_000 });
});

Given("a shop database was just restored", async () => {
  // No-op — the When step navigates to login?restored=true directly.
});

Given("I see the restore banner on the login page", async ({ page, appUrl }) => {
  await page.goto(`${appUrl}/login?restored=true`);
  await expect(page.getByTestId("dismiss-restore-banner")).toBeVisible({ timeout: 5_000 });
});

Given("no file has been selected", async () => {
  // No-op — direct navigation test uses goto without fromWelcome state.
});

Given("I have exceeded the import attempt limit", async ({ page }) => {
  await mockValidateFailure(page, "rate_limited");
});

// ────────────────────────────────────────────────────────────────────────────
// Whens
// ────────────────────────────────────────────────────────────────────────────

When('I click "Open Existing Shop"', async ({ page }) => {
  const w = getWorld(page);
  // Register before the click so we don't miss the filechooser event from the
  // restore page. Swallow rejection for scenarios that don't await it (e.g.
  // the disabled-state scenario ends before the picker ever opens).
  const fcPromise = page.waitForEvent("filechooser");
  fcPromise.catch(() => {});
  w.fileChooserPromise = fcPromise;
  // Capture the transient disabled state inside a single browser roundtrip.
  // Svelte 5 flushes $state mutations on a microtask; SvelteKit's goto()
  // takes several ticks to dynamic-import the restore route, so the old page
  // remains mounted long enough to observe the flushed `navigating=true`.
  const captured = await page.evaluate(async () => {
    const isDisabled = (sel: string): boolean =>
      document.querySelector<HTMLButtonElement>(sel)?.disabled ?? false;
    const btn = document.querySelector<HTMLButtonElement>(
      '[data-testid="open-existing-shop-button"]',
    );
    btn?.click();
    for (let i = 0; i < 20; i++) {
      if (isDisabled('[data-testid="open-existing-shop-button"]')) {
        return {
          setup: isDisabled('[data-testid="setup-shop-button"]'),
          demo: isDisabled('[data-testid="explore-demo-button"]'),
          open: true,
        };
      }
      await Promise.resolve();
    }
    return { setup: false, demo: false, open: false };
  });
  w.capturedDisabledState = captured;
});

When("I cancel the file picker", async ({ page }) => {
  const w = getWorld(page);
  const fc = await w.fileChooserPromise!;
  await fc.setFiles([]);
});

When("validation succeeds", async ({ page }) => {
  await expect(page.getByTestId("valid-state")).toBeVisible({ timeout: 8_000 });
});

When("validation fails with {string}", async ({ page }, _code: string) => {
  await expect(page.getByTestId("invalid-state")).toBeVisible({ timeout: 8_000 });
});

When('I click "Choose Different File"', async ({ page }) => {
  const w = getWorld(page);
  w.fileChooserPromise = page.waitForEvent("filechooser");
  await page.getByTestId("choose-different-button").click();
});

When('I click "Import and Restart"', async ({ page }) => {
  // Stall restore so we can observe importing state
  await page.route(RESTORE_ROUTE, async () => {
    await new Promise<void>(() => {});
  });
  await page.getByTestId("import-button").click();
});

When("the restore request fails", async ({ page }) => {
  const w = getWorld(page);
  if (!w.resolveRestoreWith) {
    throw new Error('resolveRestoreWith missing — did `Given I clicked "Import and Restart"` run?');
  }
  w.resolveRestoreWith(500, { code: "restore_failed", message: "restore_failed", details: null });
  await expect(page.getByTestId("import-failed-state")).toBeVisible({ timeout: 8_000 });
});

When('I click "Try Again"', async ({ page }) => {
  const w = getWorld(page);
  w.fileChooserPromise = page.waitForEvent("filechooser");
  await page.getByTestId("try-again-button").click();
});

When("the restore request succeeds", async ({ page }) => {
  const w = getWorld(page);
  if (!w.resolveRestoreWith) {
    throw new Error('resolveRestoreWith missing — did `Given I clicked "Import and Restart"` run?');
  }
  w.resolveRestoreWith(200, {});
  await expect(page.getByTestId("restarting-state")).toBeVisible({ timeout: 8_000 });
});

When("the server does not respond within 15 seconds", async ({ page }) => {
  // RESTART_REDIRECT_MS=2000 fires first (navigation stalled), then RESTART_TIMEOUT_MS=15000
  await page.clock.runFor(16_000);
});

When(/^I (?:arrive at|navigate directly to) "([^"]+)"$/, async ({ page, appUrl }, path: string) => {
  await page.goto(`${appUrl}${path}`);
});

When("I dismiss the banner", async ({ page }) => {
  await page.getByTestId("dismiss-restore-banner").click();
});

When("I try to validate or import another file", async ({ page, appUrl }) => {
  const fc = await navigateToRestore(page, appUrl);
  await fc.setFiles([FAKE_DB]);
  await expect(page.getByTestId("invalid-state")).toBeVisible({ timeout: 8_000 });
});

// ────────────────────────────────────────────────────────────────────────────
// Thens
// ────────────────────────────────────────────────────────────────────────────

Then(/the "([^"]+)" button has secondary\/outline styling/, async ({ page }, label: string) => {
  const testId =
    label === "Open Existing Shop"
      ? "open-existing-shop-button"
      : label.toLowerCase().replace(/\s+/g, "-") + "-button";
  const btn = page.getByTestId(testId);
  await expect(btn).toBeVisible();
  const cls = await btn.getAttribute("class");
  expect(cls).toContain("outline");
});

Then(
  'the button order is "Set Up My Shop", "Explore Demo", "Open Existing Shop"',
  async ({ page }) => {
    const boxes = await Promise.all([
      page.getByTestId("setup-shop-button").boundingBox(),
      page.getByTestId("explore-demo-button").boundingBox(),
      page.getByTestId("open-existing-shop-button").boundingBox(),
    ]);
    for (const box of boxes) expect(box).not.toBeNull();
    const [setupBox, demoBox, restoreBox] = boxes as NonNullable<(typeof boxes)[number]>[];
    expect(setupBox.y).toBeLessThan(demoBox.y);
    expect(demoBox.y).toBeLessThan(restoreBox.y);
  },
);

Then("all three buttons are disabled", async ({ page }) => {
  // State captured synchronously in the When step (Svelte 5 flush_sync) — no browser roundtrip needed.
  const { setup, demo, open } = getWorld(page).capturedDisabledState ?? {};
  expect(setup, "setup-shop-button disabled").toBe(true);
  expect(demo, "explore-demo-button disabled").toBe(true);
  expect(open, "open-existing-shop-button disabled").toBe(true);
});

Then("a file picker dialog opens filtered to .db files", async ({ page }) => {
  const w = getWorld(page);
  const fc = await w.fileChooserPromise!;
  const accept = await fc.element().getAttribute("accept");
  expect(accept).toBe(".db");
});

Then("I see a {string} message with a spinner", async ({ page }, text: string) => {
  await expect(page.getByText(text)).toBeVisible({ timeout: 8_000 });
});

Then("a validation request is sent to the server", async ({ page }) => {
  await expect(page.getByTestId("validating-state")).toBeVisible();
});

Then("I see the file name and size", async ({ page }) => {
  await expect(page.getByText("shop-backup.db")).toBeVisible();
  // 102400 bytes = 100.0 KB
  await expect(page.getByText("100.0 KB")).toBeVisible();
});

Then("I see {string} with a success indicator", async ({ page }, text: string) => {
  await expect(page.getByText(text)).toBeVisible();
});

Then("I see a credential warning {string}", async ({ page }, text: string) => {
  await expect(page.getByText(text)).toBeVisible();
});

Then(/I see an? "([^"]+)" button/, async ({ page }, label: string) => {
  if (label === "Import and Restart") {
    await expect(page.getByTestId("import-button")).toBeVisible();
  } else if (label === "Open Existing Shop") {
    await expect(page.getByTestId("open-existing-shop-button")).toBeVisible();
  } else if (label === "Choose Different File") {
    await expect(page.getByTestId("choose-different-button")).toBeVisible();
  } else if (label === "Try Again") {
    await expect(page.getByTestId("try-again-button")).toBeVisible();
  } else if (label === "Retry") {
    await expect(page.getByTestId("retry-button")).toBeVisible();
  } else {
    await expect(page.getByRole("button", { name: label })).toBeVisible();
  }
});

Then(
  "I see an error message explaining the file is not a valid Mokumo database",
  async ({ page }) => {
    await expect(page.getByTestId("invalid-state")).toBeVisible({ timeout: 8_000 });
    await expect(page.getByText("not a valid Mokumo database", { exact: false })).toBeVisible();
  },
);

Then('I see a "Back" link', async ({ page }) => {
  const backLink = page.getByTestId("back-button");
  await expect(backLink).toBeVisible();
  await expect(backLink).toHaveText("Back");
});

Then("I see an error message explaining the file appears damaged", async ({ page }) => {
  await expect(page.getByTestId("invalid-state")).toBeVisible({ timeout: 8_000 });
  await expect(page.getByText("appears to be damaged", { exact: false })).toBeVisible();
});

Then("I see an error message advising to update Mokumo", async ({ page }) => {
  await expect(page.getByTestId("invalid-state")).toBeVisible({ timeout: 8_000 });
  await expect(page.getByText("newer version", { exact: false })).toBeVisible();
});

Then("the file picker opens again", async ({ page }) => {
  const w = getWorld(page);
  await w.fileChooserPromise;
  void page;
});

Then("I see a spinner with {string}", async ({ page }, text: string) => {
  await expect(page.getByTestId("importing-state")).toBeVisible({ timeout: 8_000 });
  await expect(page.getByText(text)).toBeVisible();
});

Then("a restore request is sent to the server", async ({ page }) => {
  await expect(page.getByTestId("importing-state")).toBeVisible();
});

Then("I see an error message", async ({ page }) => {
  await expect(page.getByTestId("import-failed-state")).toBeVisible({ timeout: 8_000 });
});

Then('I see a "Back to Welcome" link', async ({ page }) => {
  await expect(page.getByTestId("back-button")).toBeVisible();
  await expect(page.getByTestId("back-button")).toContainText("Back to Welcome");
});

Then("I see {string}", async ({ page }, text: string) => {
  await expect(page.getByText(text, { exact: false })).toBeVisible({ timeout: 8_000 });
});

Then("the page reloads to {string} after a short delay", async ({ page }, path: string) => {
  await page.waitForURL(`**${path}**`, { timeout: 5_000 });
  expect(page.url()).toContain(path);
});

Then("I see a banner {string}", async ({ page }, text: string) => {
  await expect(page.getByText(text, { exact: false })).toBeVisible({ timeout: 5_000 });
});

Then("the banner is no longer visible", async ({ page }) => {
  await expect(
    page.getByText("Your shop data has been imported", { exact: false }),
  ).not.toBeVisible();
});
