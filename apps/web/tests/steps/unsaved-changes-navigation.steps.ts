import { expect, type Page } from "@playwright/test";
import { Given, When, Then } from "../support/app.fixture";

// ────────────────────────────────────────────────────────────────────────────
// Constants
// ────────────────────────────────────────────────────────────────────────────

const SETUP_STATUS_ROUTE = "**/api/setup-status";

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

async function mockSetupStatus(page: Page): Promise<void> {
  await page.route(SETUP_STATUS_ROUTE, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        setup_complete: true,
        setup_mode: "demo",
        is_first_launch: false,
        production_setup_complete: true,
        shop_name: "Gary's Printing Co",
      }),
    }),
  );
}

async function navigateToCustomerForm(page: Page): Promise<void> {
  await mockSetupStatus(page);
  await page.goto("/customers");
  await page.waitForLoadState("networkidle");
  await page.getByRole("button", { name: "Add Customer" }).first().click();
  await expect(page.getByRole("dialog")).toBeVisible();
}

async function typeInCustomerForm(page: Page): Promise<void> {
  await page.getByLabel("Display Name").fill("Test Customer");
}

async function clickSidebarHome(page: Page): Promise<void> {
  await page.getByRole("link", { name: "Home" }).click();
}

async function setupNavigationDialogOpen(page: Page): Promise<void> {
  await navigateToCustomerForm(page);
  await typeInCustomerForm(page);
  await clickSidebarHome(page);
  await expect(page.getByTestId("unsaved-changes-dialog")).toBeVisible();
}

// ────────────────────────────────────────────────────────────────────────────
// Givens
// ────────────────────────────────────────────────────────────────────────────

Given("I am on the customers page with no dirty forms", async ({ page }) => {
  await mockSetupStatus(page);
  await page.goto("/customers");
  await page.waitForLoadState("networkidle");
});

Given("I have unsaved changes in the customer form", async ({ page }) => {
  await navigateToCustomerForm(page);
  await typeInCustomerForm(page);
});

Given("the navigation unsaved changes dialog is open", async ({ page }) => {
  await setupNavigationDialogOpen(page);
});

Given("I navigated to the customers page from the home page", async ({ page }) => {
  await mockSetupStatus(page);
  await page.goto("/");
  await page.waitForLoadState("networkidle");
  await page.getByRole("link", { name: "Customers" }).click();
  await page.waitForURL((url) => url.pathname === "/customers");
});

// ────────────────────────────────────────────────────────────────────────────
// Whens
// ────────────────────────────────────────────────────────────────────────────

When("I click a sidebar link to navigate away", async ({ page }) => {
  await clickSidebarHome(page);
});

When('I click "Leave anyway" in the navigation dialog', async ({ page }) => {
  await page.getByTestId("unsaved-changes-confirm-btn").click();
});

When('I click "Cancel" in the navigation dialog', async ({ page }) => {
  await page.getByTestId("unsaved-changes-cancel-btn").click();
});

When("I press the browser back button", async ({ page }) => {
  await page.goBack();
});

When("I save the customer form successfully", async ({ page }) => {
  await page.route("**/api/customers", async (route) => {
    if (route.request().method() === "POST") {
      await route.fulfill({
        status: 201,
        contentType: "application/json",
        body: JSON.stringify({ id: 999, display_name: "Test Customer" }),
      });
    } else {
      await route.continue();
    }
  });
  const nameInput = page.getByLabel("Display Name");
  if (!(await nameInput.inputValue())) {
    await nameInput.fill("Test Customer");
  }
  await page.getByRole("button", { name: /Create|Save Changes/i }).click();
  await expect(page.getByRole("dialog", { name: "Add Customer" })).not.toBeVisible({
    timeout: 3000,
  });
});

// ────────────────────────────────────────────────────────────────────────────
// Thens
// ────────────────────────────────────────────────────────────────────────────

Then("the navigation completes without a dialog", async ({ page }) => {
  await page.waitForURL((url) => url.pathname === "/", { timeout: 5000 });
  await expect(page.getByTestId("unsaved-changes-dialog")).not.toBeVisible();
});

Then('the "Unsaved changes" navigation dialog appears', async ({ page }) => {
  await expect(page.getByTestId("unsaved-changes-dialog")).toBeVisible();
});

Then("the navigation has not completed", async ({ page }) => {
  expect(page.url()).toContain("/customers");
});

Then("the navigation completes to the destination", async ({ page }) => {
  await page.waitForURL((url) => url.pathname === "/", { timeout: 5000 });
});

Then("I remain on the customers page with form data intact", async ({ page }) => {
  expect(page.url()).toContain("/customers");
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.getByLabel("Display Name")).toHaveValue("Test Customer");
});
