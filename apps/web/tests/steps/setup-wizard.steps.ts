import { expect, type Page } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";
import { buildServerInfo, mockHealth, mockServerInfo } from "../support/server-info.helpers";

const SETUP_ROUTE = "**/api/setup";
const SETUP_STATUS_ROUTE = "**/api/setup-status";
const LAN_ACCESS_ROUTE = "**/api/settings/lan-access";

type LanPrefState = { enabled: boolean; lastWritten: boolean | null };

const lanPrefStates = new WeakMap<Page, LanPrefState>();

function getLanPrefState(page: Page): LanPrefState {
  let state = lanPrefStates.get(page);
  if (!state) {
    state = { enabled: false, lastWritten: null };
    lanPrefStates.set(page, state);
  }
  return state;
}

async function mockLanAccess(page: Page): Promise<void> {
  const state = getLanPrefState(page);
  await page.route(LAN_ACCESS_ROUTE, async (route) => {
    const req = route.request();
    if (req.method() === "PUT") {
      const body = (await req.postDataJSON()) as { enabled: boolean };
      state.enabled = body.enabled;
      state.lastWritten = body.enabled;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ enabled: body.enabled }),
      });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ enabled: state.enabled }),
    });
  });
}

const TEST_TOKEN = "test-token-123";
const TEST_SHOP = "Test Shop";
const TEST_NAME = "Gary";
const TEST_EMAIL = "gary@example.com";
const TEST_PASSWORD = "SecurePassword123!";

async function mockSetupStatus(page: Page, complete: boolean): Promise<void> {
  await page.route(SETUP_STATUS_ROUTE, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ setup_complete: complete }),
    });
  });
}

async function mockSetupSuccess(page: Page): Promise<void> {
  await page.route(SETUP_ROUTE, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ recovery_codes: ["CODE1", "CODE2", "CODE3", "CODE4"] }),
    });
  });
}

async function mockSetupFailure(page: Page): Promise<void> {
  await page.route(SETUP_ROUTE, async (route) => {
    await route.fulfill({
      status: 400,
      contentType: "application/json",
      body: JSON.stringify({
        code: "validation_error",
        message: "Invalid setup token",
        details: null,
      }),
    });
  });
}

/** Fill step 2 (shop name) and advance to step 3 */
async function fillStep2(page: Page): Promise<void> {
  await page.getByLabel("Shop name").fill(TEST_SHOP);
  await page.getByRole("button", { name: "Continue" }).click();
}

/** Fill step 3 fields (name, email, password) without submitting */
async function fillStep3Fields(page: Page): Promise<void> {
  await page.getByLabel("Name").fill(TEST_NAME);
  await page.getByLabel("Email").fill(TEST_EMAIL);
  await page.getByLabel("Password").fill(TEST_PASSWORD);
}

/** Navigate from step 1 through step 2 to reach step 3 */
async function navigateToStep3(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Get Started" }).click();
  await fillStep2(page);
}

/** Advance to the LAN access consent step (step 5) */
async function advanceToLanStep(page: Page): Promise<void> {
  await mockLanAccess(page);
  await page.getByRole("button", { name: "Get Started" }).click();
  await fillStep2(page);
  await fillStep3Fields(page);
  const tokenField = page.locator("#setup-token");
  if (await tokenField.isVisible()) {
    await tokenField.fill(TEST_TOKEN);
  }
  await page.getByRole("button", { name: "Create Account" }).click();
  await expect(page.getByText("Recovery Codes", { exact: true })).toBeVisible();
  await page.getByLabel("I have saved my recovery codes").click();
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByText("Enable LAN Access?")).toBeVisible();
}

/** Complete the full wizard through the LAN step to reach the completion screen */
async function completeWizardToStep5(page: Page): Promise<void> {
  await advanceToLanStep(page);
  await page.getByTestId("lan-skip").click();
  await expect(page.getByText("You're all set!")).toBeVisible();
}

// --- Given steps ---

Given("the setup wizard is opened with a setup token in the URL", async ({ page, appUrl }) => {
  await mockSetupStatus(page, false);
  await mockSetupSuccess(page);
  await mockHealth(page);
  await page.goto(new URL(`/setup?setup_token=${TEST_TOKEN}`, appUrl).toString());
  await expect(page.getByText("Welcome to Mokumo Print")).toBeVisible();
});

Given("the setup wizard is opened without a setup token in the URL", async ({ page, appUrl }) => {
  await mockSetupStatus(page, false);
  await mockSetupSuccess(page);
  await mockHealth(page);
  await page.goto(new URL("/setup", appUrl).toString());
  await expect(page.getByText("Welcome to Mokumo Print")).toBeVisible();
});

Given("I have completed the setup wizard", async ({ page, appUrl }) => {
  await mockSetupStatus(page, false);
  await mockSetupSuccess(page);
  await mockHealth(page);
  await page.goto(new URL(`/setup?setup_token=${TEST_TOKEN}`, appUrl).toString());
  await expect(page.getByText("Welcome to Mokumo Print")).toBeVisible();
});

Given("I am on the setup completion screen", async ({ page, appUrl }) => {
  await mockSetupStatus(page, false);
  await mockSetupSuccess(page);
  await mockHealth(page);
  await mockServerInfo(page, buildServerInfo());
  await page.goto(new URL(`/setup?setup_token=${TEST_TOKEN}`, appUrl).toString());
  await expect(page.getByText("Welcome to Mokumo Print")).toBeVisible();
  await completeWizardToStep5(page);
});

// --- When steps ---

When("I reach the admin account step", async ({ page }) => {
  await navigateToStep3(page);
  await expect(page.getByText("Admin Account")).toBeVisible();
});

When("account creation fails with an error", async ({ page }) => {
  // Override the setup mock to fail
  await page.unroute(SETUP_ROUTE);
  await mockSetupFailure(page);
  // Fill form fields
  await fillStep3Fields(page);
  // If token field is visible, fill it
  const tokenField = page.locator("#setup-token");
  if (await tokenField.isVisible()) {
    await tokenField.fill(TEST_TOKEN);
  }
  await page.getByRole("button", { name: "Create Account" }).click();
});

When("I reach the completion screen", async ({ page }) => {
  await completeWizardToStep5(page);
});

When("I click {string}", async ({ page }, buttonName: string) => {
  // After setup completes, navigating to dashboard needs setup_complete: true
  if (buttonName === "Open Dashboard") {
    await page.unroute(SETUP_STATUS_ROUTE);
    await mockSetupStatus(page, true);
  }
  await page.getByRole("button", { name: buttonName }).click();
});

// --- Then steps ---

Then("I do not see the setup token field", async ({ page }) => {
  await expect(page.locator("#setup-token")).not.toBeVisible();
});

Then("I see the setup token field", async ({ page }) => {
  await expect(page.locator("#setup-token")).toBeVisible();
});

Then("the admin account form shows name, email, and password fields", async ({ page }) => {
  await expect(page.getByLabel("Name")).toBeVisible();
  await expect(page.getByLabel("Email")).toBeVisible();
  await expect(page.getByLabel("Password")).toBeVisible();
});

Then("the field helper text says {string}", async ({ page }, text: string) => {
  await expect(page.getByText(text)).toBeVisible();
});

Then("I see the error message", async ({ page }) => {
  await expect(page.getByText("Invalid setup token")).toBeVisible();
});

Then("I see instructions for connecting other devices", async ({ page }) => {
  await expect(
    page.getByText("Other devices on your network can reach your shop at"),
  ).toBeVisible();
});

Then("I am redirected to the dashboard", async ({ page }) => {
  await expect(page).toHaveURL(/\/$/);
});

// --- LAN access consent step ---

Given("I have completed the recovery codes step", async ({ page, appUrl }) => {
  await mockSetupStatus(page, false);
  await mockSetupSuccess(page);
  await mockHealth(page);
  await mockServerInfo(page, buildServerInfo());
  await mockLanAccess(page);
  await page.goto(new URL(`/setup?setup_token=${TEST_TOKEN}`, appUrl).toString());
  await page.getByRole("button", { name: "Get Started" }).click();
  await fillStep2(page);
  await fillStep3Fields(page);
  await page.getByRole("button", { name: "Create Account" }).click();
  await expect(page.getByText("Recovery Codes", { exact: true })).toBeVisible();
  await page.getByLabel("I have saved my recovery codes").click();
});

Given("I am on the LAN access consent step", async ({ page, appUrl }) => {
  await mockSetupStatus(page, false);
  await mockSetupSuccess(page);
  await mockHealth(page);
  await mockServerInfo(page, buildServerInfo());
  await page.goto(new URL(`/setup?setup_token=${TEST_TOKEN}`, appUrl).toString());
  await advanceToLanStep(page);
});

Given("the LAN access API accepts updates", async ({ page }) => {
  await mockLanAccess(page);
});

When("I continue past the recovery codes", async ({ page }) => {
  await page.getByRole("button", { name: "Continue" }).click();
});

Then('I see the "Enable LAN Access?" step', async ({ page }) => {
  await expect(page.getByText("Enable LAN Access?")).toBeVisible();
});

Then('I see an "Enable LAN Access" button', async ({ page }) => {
  await expect(page.getByTestId("lan-enable")).toBeVisible();
});

Then('I see a "Not now" button', async ({ page }) => {
  await expect(page.getByTestId("lan-skip")).toBeVisible();
});

Then("the LAN access preference is set to enabled", async ({ page }) => {
  await expect.poll(() => getLanPrefState(page).lastWritten).toBe(true);
});

Then("the LAN access preference is set to disabled", async ({ page }) => {
  await expect.poll(() => getLanPrefState(page).lastWritten).toBe(false);
});

Then("I reach the completion screen", async ({ page }) => {
  await expect(page.getByText("You're all set!")).toBeVisible();
});
