import type { Page } from "@playwright/test";
import { test, expect, SCREENSHOT_BASE } from "../support/demo.fixture";
import { TEST_ADMIN } from "../support/app-helpers";

const DEMO_CUSTOMER = {
  displayName: "Acme Screen Printing",
  companyName: "Acme Apparel Co",
  email: "orders@acmeprinting.com",
  phone: "(555) 867-5309",
};

/**
 * Wait for animations to settle, then capture screenshot.
 * networkidle is safe here because we run against a local Axum server with no
 * external requests, WebSockets, or SSE. If the server adds persistent connections,
 * replace with targeted waitForResponse/waitForSelector per screenshot.
 */
async function stableScreenshot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState("networkidle");
  await page.waitForTimeout(300);
  await page.screenshot({
    path: `${SCREENSHOT_BASE}/${name}.png`,
    fullPage: false,
  });
}

test.describe("M0 Demo Screenshots", () => {
  test.describe.configure({ mode: "serial" });
  test.setTimeout(120_000);

  // ── V2: Setup Wizard (screenshots #10-#12) ──────────────────────────

  test("#10 setup-wizard-shop-name", async ({ demoPage, setupToken }) => {
    await demoPage.goto(`/setup?setup_token=${setupToken}`);
    await expect(demoPage.getByText("Welcome to Mokumo Print")).toBeVisible();
    // Step 1 → Step 2 (shop name)
    await demoPage.getByRole("button", { name: "Get Started" }).click();
    await expect(demoPage.getByLabel("Shop name")).toBeVisible();
    await demoPage.getByLabel("Shop name").fill(TEST_ADMIN.shopName);
    await stableScreenshot(demoPage, "10-setup-wizard-shop-name");
  });

  test("#11 setup-wizard-password", async ({ demoPage }) => {
    // Step 2 → Step 3 (admin account)
    await demoPage.getByRole("button", { name: "Continue" }).click();
    await expect(demoPage.getByLabel("Password")).toBeVisible();
    await demoPage.getByLabel("Name").fill(TEST_ADMIN.name);
    await demoPage.getByLabel("Email").fill(TEST_ADMIN.email);
    await demoPage.getByLabel("Password").fill(TEST_ADMIN.password);
    await stableScreenshot(demoPage, "11-setup-wizard-password");
  });

  test("#12 setup-wizard-recovery-code", async ({ demoPage, setupToken }) => {
    // Fill setup token if visible, then submit
    const tokenField = demoPage.locator("#setup-token");
    if (await tokenField.isVisible()) {
      await tokenField.fill(setupToken);
    }
    await demoPage.getByRole("button", { name: "Create Account" }).click();
    // Step 4: Recovery codes
    await expect(demoPage.getByText("Recovery Codes", { exact: true })).toBeVisible();
    // Assert codes container has child elements (not just an empty container)
    const codeElements = demoPage.locator("[data-testid='recovery-codes'] code, .font-mono");
    await expect(codeElements.first()).toBeVisible();
    await stableScreenshot(demoPage, "12-setup-wizard-recovery-code");
  });

  // ── V3: Login + Dashboard + Shell (screenshots #13-#18) ─────────────

  test("#13 login-screen", async ({ demoPage }) => {
    // Save codes and continue to completion
    await demoPage.getByLabel("I have saved my recovery codes").click();
    await demoPage.getByRole("button", { name: "Continue" }).click();
    await expect(demoPage.getByText("You're all set!")).toBeVisible();
    // Complete wizard → goes to dashboard (auto-logged-in after setup)
    await demoPage.getByRole("button", { name: "Open Dashboard" }).click();
    await expect(demoPage.getByRole("heading", { name: "Your Shop" })).toBeVisible({
      timeout: 15_000,
    });
    // Log out to capture the login screen
    await demoPage.goto("/login");
    await expect(
      demoPage.locator("[data-slot='card-title']").filter({ hasText: "Sign in" }),
    ).toBeVisible({ timeout: 10_000 });
    await stableScreenshot(demoPage, "13-login-screen");
  });

  test("#14 dashboard-after-login", async ({ demoPage }) => {
    // UI-driven login (the visual flow IS the point)
    await demoPage.locator("#email").fill(TEST_ADMIN.email);
    await demoPage.locator("#password").fill(TEST_ADMIN.password);
    await demoPage.getByRole("button", { name: "Sign in" }).click();
    await expect(demoPage.getByRole("heading", { name: "Your Shop" })).toBeVisible({
      timeout: 15_000,
    });
    await stableScreenshot(demoPage, "14-dashboard-after-login");
  });

  test("#15 sidebar-navigation", async ({ demoPage }) => {
    // Sidebar is visible after login with navigation items
    await expect(demoPage.locator("[data-sidebar='content']")).toBeVisible();
    await stableScreenshot(demoPage, "15-sidebar-navigation");
  });

  test("#16 sidebar-collapsed", async ({ demoPage }) => {
    // Click trigger to collapse (NOT viewport resize — 768px triggers mobile sheet overlay)
    await demoPage.locator("[data-sidebar='trigger']").click();
    // Wait for CSS animation to complete — collapsed sidebar is narrow (3rem ≈ 47-48px)
    await expect(demoPage.locator("[data-sidebar='sidebar']")).toHaveCSS("width", /^4[7-8]px$/, {
      timeout: 5_000,
    });
    await stableScreenshot(demoPage, "16-sidebar-collapsed");
    // Re-expand for subsequent tests
    await demoPage.locator("[data-sidebar='trigger']").click();
    await expect(demoPage.locator("[data-sidebar='sidebar']")).not.toHaveCSS(
      "width",
      /^4[7-8]px$/,
      {
        timeout: 5_000,
      },
    );
  });

  test("#17 empty-state-customers", async ({ demoPage }) => {
    await demoPage.goto("/customers");
    await expect(
      demoPage
        .getByText("No customers yet")
        .or(demoPage.getByRole("heading", { name: "Customers" })),
    ).toBeVisible({ timeout: 10_000 });
    await stableScreenshot(demoPage, "17-empty-state-customers");
  });

  test("#18 theme-switcher", async ({ demoPage }) => {
    // First close any open popover from navigation
    await demoPage.keyboard.press("Escape");
    await expect(demoPage.getByTestId("help-popover"))
      .not.toBeVisible({ timeout: 3_000 })
      .catch(() => {});
    // The theme switcher is inside the sidebar footer Owner popover
    await demoPage.getByText("Owner", { exact: true }).click();
    await expect(demoPage.getByLabel("Toggle light/dark mode")).toBeVisible({ timeout: 5_000 });
    await stableScreenshot(demoPage, "18-theme-switcher");
    // Close popover
    await demoPage.keyboard.press("Escape");
  });

  // ── V4: Customer CRUD (screenshots #19-#24) ─────────────────────────

  test("#19 new-customer-button", async ({ demoPage }) => {
    await demoPage.goto("/customers");
    await expect(demoPage.getByRole("button", { name: "Add Customer" })).toBeVisible({
      timeout: 10_000,
    });
    await stableScreenshot(demoPage, "19-new-customer-button");
  });

  test("#20 customer-form-filled", async ({ demoPage }) => {
    await demoPage.getByRole("button", { name: "Add Customer" }).click();
    const dialog = demoPage.getByRole("dialog");
    await expect(dialog).toBeVisible();
    // Fill with demo data
    await dialog.locator("#display_name").fill(DEMO_CUSTOMER.displayName);
    await dialog.locator("#company_name").fill(DEMO_CUSTOMER.companyName);
    await dialog.locator("#email").fill(DEMO_CUSTOMER.email);
    await dialog.locator("#phone").fill(DEMO_CUSTOMER.phone);
    await stableScreenshot(demoPage, "20-customer-form-filled");
  });

  test("#21 customer-detail", async ({ demoPage }) => {
    // Submit the form and wait for API response
    const dialog = demoPage.getByRole("dialog");
    await Promise.all([
      demoPage.waitForResponse((r) => r.url().includes("/api/customers") && r.status() === 201),
      dialog.getByRole("button", { name: "Create" }).click(),
    ]);
    // Wait for sheet to close
    await expect(dialog).toHaveCount(0, { timeout: 10_000 });
    // Reload customer list to see the new customer
    await demoPage.goto("/customers");
    await expect(demoPage.locator("table tbody tr").first()).toBeVisible({ timeout: 10_000 });
    // Click into detail
    await demoPage.locator("table").getByText(DEMO_CUSTOMER.displayName).click();
    await expect(demoPage.getByRole("heading", { name: DEMO_CUSTOMER.displayName })).toBeVisible({
      timeout: 10_000,
    });
    await stableScreenshot(demoPage, "21-customer-detail");
  });

  test("#22 customer-edit", async ({ demoPage }) => {
    await demoPage.getByRole("button", { name: "Edit" }).click();
    const dialog = demoPage.getByRole("dialog");
    await expect(dialog).toBeVisible();
    // Assert form has existing data
    await expect(dialog.locator("#display_name")).toHaveValue(DEMO_CUSTOMER.displayName);
    await stableScreenshot(demoPage, "22-customer-edit");
    // Close without saving
    await demoPage.keyboard.press("Escape");
    await expect(demoPage.getByRole("dialog")).toHaveCount(0, { timeout: 5_000 });
  });

  test("#23 customer-search", async ({ demoPage }) => {
    // Navigate to list — should have at least 1 customer
    await demoPage.goto("/customers");
    await expect(demoPage.getByRole("heading", { name: "Customers" })).toBeVisible({
      timeout: 10_000,
    });
    const rows = demoPage.locator("table tbody tr");
    await expect(rows.first()).toBeVisible({ timeout: 10_000 });
    const initialCount = await rows.count();
    // Type in search
    const searchInput = demoPage.getByPlaceholder("Search customers");
    await Promise.all([
      demoPage.waitForResponse((r) => r.url().includes("/api/customers") && r.ok()),
      searchInput.fill("Acme"),
    ]);
    // Assert row count decreased (or stayed same if only 1) and at least 1 row visible
    const filteredCount = await rows.count();
    expect(filteredCount).toBeLessThanOrEqual(initialCount);
    expect(filteredCount).toBeGreaterThan(0);
    await stableScreenshot(demoPage, "23-customer-search");
  });

  test("#24 customer-kpi-cards", async ({ demoPage }) => {
    // Clear search to show all customers with KPI strip
    const searchInput = demoPage.getByPlaceholder("Search customers");
    await Promise.all([
      demoPage.waitForResponse((r) => r.url().includes("/api/customers") && r.ok()),
      searchInput.clear(),
    ]);
    await expect(demoPage.getByText(/total customer/)).toBeVisible();
    await stableScreenshot(demoPage, "24-customer-kpi-cards");
  });

  // ── V5: Soft Delete + Restore + Activity + Settings (#25-#28, #33) ──

  test("#25 soft-delete-confirm", async ({ demoPage }) => {
    // Navigate to the customer detail
    await demoPage.locator("table").getByText(DEMO_CUSTOMER.displayName).click();
    await expect(demoPage.getByRole("heading", { name: DEMO_CUSTOMER.displayName })).toBeVisible({
      timeout: 10_000,
    });
    // Click archive
    await demoPage.getByRole("button", { name: "Archive" }).click();
    const dialog = demoPage.getByRole("alertdialog").or(demoPage.getByRole("dialog"));
    await expect(dialog).toBeVisible();
    await stableScreenshot(demoPage, "25-soft-delete-confirm");
  });

  test("#26 show-deleted-toggle", async ({ demoPage }) => {
    // Confirm archive
    const dialog = demoPage.getByRole("alertdialog").or(demoPage.getByRole("dialog"));
    await dialog.getByRole("button", { name: "Archive" }).click();
    // Wait for redirect — navigate to list with include_deleted to show the toggle + archived row
    await expect(demoPage).toHaveURL(/\/customers/, { timeout: 10_000 });
    await demoPage.goto("/customers?include_deleted=true");
    await expect(demoPage.locator("table tbody tr").first()).toBeVisible({ timeout: 10_000 });
    // Assert archived row has visual indicator
    const archivedRow = demoPage
      .locator("table tbody tr")
      .filter({ hasText: DEMO_CUSTOMER.displayName });
    await expect(archivedRow).toBeVisible();
    await stableScreenshot(demoPage, "26-show-deleted-toggle");
  });

  test("#27 restore-customer", async ({ demoPage }) => {
    // Click into the archived customer detail
    await demoPage.locator("table").getByText(DEMO_CUSTOMER.displayName).click();
    await expect(demoPage.getByRole("heading", { name: DEMO_CUSTOMER.displayName })).toBeVisible({
      timeout: 10_000,
    });
    // Assert Restore button visible (from #89)
    await expect(demoPage.getByRole("button", { name: "Restore" })).toBeVisible();
    await stableScreenshot(demoPage, "27-restore-customer");
  });

  test("#28 activity-log-list", async ({ demoPage }) => {
    // Restore the customer first so we have a rich activity log
    await demoPage.getByRole("button", { name: "Restore" }).click();
    const dialog = demoPage.getByRole("alertdialog").or(demoPage.getByRole("dialog"));
    await dialog.getByRole("button", { name: "Restore" }).click();
    // Wait for restore to complete
    await expect(demoPage.getByText("Archived")).toHaveCount(0, { timeout: 5_000 });
    // Navigate to Activity tab
    const tabNav = demoPage.getByLabel("Tab navigation");
    await tabNav.getByRole("link", { name: "Activity" }).click();
    await expect(demoPage.getByTestId("activity-entry").first()).toBeVisible({ timeout: 10_000 });
    await stableScreenshot(demoPage, "28-activity-log-list");
  });

  test("#33 settings-page", async ({ demoPage }) => {
    await demoPage.goto("/settings/shop");
    await expect(demoPage.getByRole("heading", { name: "Shop Settings" })).toBeVisible({
      timeout: 10_000,
    });
    await expect(demoPage.getByTestId("lan-status-badge")).toBeVisible();
    await stableScreenshot(demoPage, "33-settings-page");
  });
});
