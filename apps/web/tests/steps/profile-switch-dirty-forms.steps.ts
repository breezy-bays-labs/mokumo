import { expect, type Page } from "@playwright/test";
import { Given, When, Then } from "../support/app.fixture";

// ────────────────────────────────────────────────────────────────────────────
// Constants
// ────────────────────────────────────────────────────────────────────────────

const SETUP_STATUS_ROUTE = "**/api/setup-status";
const PROFILE_SWITCH_ROUTE = "**/api/profile/switch";

// ────────────────────────────────────────────────────────────────────────────
// Per-test state
// ────────────────────────────────────────────────────────────────────────────

type DirtyTestState = {
  switchRequests: Array<unknown>;
  urlBeforeSwitch: string | null;
};

const testState = new WeakMap<Page, DirtyTestState>();

function getState(page: Page): DirtyTestState {
  if (!testState.has(page)) {
    testState.set(page, {
      switchRequests: [],
      urlBeforeSwitch: null,
    });
  }
  return testState.get(page)!;
}

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

async function mockSetupStatus(page: Page, mode: "demo" | "production" = "demo"): Promise<void> {
  await page.route(SETUP_STATUS_ROUTE, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        setup_complete: true,
        setup_mode: mode,
        is_first_launch: false,
        production_setup_complete: true,
        shop_name: "Gary's Printing Co",
      }),
    }),
  );
}

async function interceptSwitchRoute(page: Page): Promise<void> {
  const state = getState(page);
  await page.route(PROFILE_SWITCH_ROUTE, async (route) => {
    state.switchRequests.push(route.request().postDataJSON() as unknown);
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ profile: "production" }),
    });
  });
}

/**
 * Navigate to the customers page (mock-based, no real backend).
 * Opens the "Add Customer" sheet so the form with use:formDirty is mounted.
 */
async function navigateToCustomerForm(page: Page): Promise<void> {
  await mockSetupStatus(page, "demo");
  await page.goto("/customers");
  await page.waitForLoadState("networkidle");
  // Open the Add Customer sheet — contains the form with use:formDirty
  await page.getByRole("button", { name: "Add Customer" }).first().click();
  await expect(page.getByRole("dialog")).toBeVisible();
}

/**
 * Type into the Display Name field in the customer form sheet.
 * This fires an `input` event which marks the form dirty via use:formDirty.
 */
async function typeInCustomerForm(page: Page): Promise<void> {
  await page.getByLabel("Display Name").fill("Test Customer");
}

/**
 * Open the profile switcher dropdown and click the production entry
 * to trigger a profile switch.
 */
async function triggerProfileSwitch(page: Page): Promise<void> {
  await page.getByTestId("profile-switcher-trigger").click();
  await expect(page.getByTestId("profile-dropdown")).toBeVisible();
  await page.getByTestId("profile-entry-production").click();
}

/**
 * Full setup for "dirty dialog is open" state:
 * navigate to form, type something, intercept switch route, then trigger switch.
 */
async function setupDirtyDialogOpen(page: Page): Promise<void> {
  await interceptSwitchRoute(page);
  await navigateToCustomerForm(page);
  await typeInCustomerForm(page);
  await triggerProfileSwitch(page);
  await expect(page.getByTestId("unsaved-changes-dialog")).toBeVisible();
}

// ────────────────────────────────────────────────────────────────────────────
// Givens
// ────────────────────────────────────────────────────────────────────────────

Given("I am on a page with no dirty forms", async ({ page }) => {
  await mockSetupStatus(page, "demo");
  await interceptSwitchRoute(page);
  await page.goto("/");
  await page.waitForLoadState("networkidle");
});

Given("I have unsaved changes in a form", async ({ page }) => {
  await interceptSwitchRoute(page);
  await navigateToCustomerForm(page);
  await typeInCustomerForm(page);
});

Given("the unsaved changes dialog is open", async ({ page }) => {
  await setupDirtyDialogOpen(page);
});

Given("I navigate to a page with a form", async ({ page }) => {
  await navigateToCustomerForm(page);
});

Given("I had unsaved changes and navigated away", async ({ page }) => {
  await navigateToCustomerForm(page);
  await typeInCustomerForm(page);
  // Navigate via SvelteKit's client-side router (not a full page reload) so the
  // form component unmounts gracefully and formDirty.destroy() runs, clearing
  // the dirty entry from profile.dirtyForms.
  await page.getByRole("link", { name: "Home" }).click();
  await page.waitForURL((url) => url.pathname === "/");
});

Given("I have two open forms with unsaved changes", async ({ page }) => {
  // NOTE: This step simulates multiple pending changes by dispatching a second
  // input event on the same form — so the same form-dirty-* ID is reused.
  // This tests that the guard fires exactly once regardless of how many input
  // events the form has received, not that two independent forms are both open.
  // True multi-form coverage requires a second mounted form element and is tracked
  // as a follow-up test gap.
  await interceptSwitchRoute(page);
  await navigateToCustomerForm(page);
  await typeInCustomerForm(page);
  await page.evaluate(() => {
    const form = document.querySelector("form");
    if (form) {
      form.dispatchEvent(new Event("input", { bubbles: true }));
    }
  });
});

// ────────────────────────────────────────────────────────────────────────────
// Whens
// ────────────────────────────────────────────────────────────────────────────

When("I select a different profile from the sidebar switcher", async ({ page }) => {
  await triggerProfileSwitch(page);
});

When('I click "Leave anyway"', async ({ page }) => {
  await page.getByTestId("unsaved-changes-confirm-btn").click();
});

When('I click "Cancel"', async ({ page }) => {
  await page.getByTestId("unsaved-changes-cancel-btn").click();
});

When("I press the Escape key", async ({ page }) => {
  await page.keyboard.press("Escape");
});

When("I click outside the dialog", async ({ page }) => {
  // Click the overlay at top-left (outside the centered dialog content).
  // The Bits UI dialog overlay covers the full viewport, so (10, 10) reliably
  // lands on the overlay rather than the dialog content.
  await page.mouse.click(10, 10);
});

When("I type in an input field without saving", async ({ page }) => {
  await typeInCustomerForm(page);
});

When("I save the form", async ({ page }) => {
  // Mock the customer create endpoint to return success so the sheet closes.
  // formDirty clears dirty state only on destroy() (form unmount), so the form
  // must actually close — not just receive a submit event — to mark itself clean.
  // This matches production behavior: a failed API call leaves the sheet open and
  // dirty state intact; a successful save closes the sheet and clears dirty state.
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
  // Wait for the sheet to close — this fires formDirty destroy(), clearing dirty state.
  await expect(page.getByRole("dialog", { name: "Add Customer" })).not.toBeVisible({
    timeout: 3000,
  });
});

When("I return to that form", async ({ page }) => {
  // Navigate via SvelteKit's client-side router to preserve page context.
  await page.getByRole("link", { name: "Customers" }).click();
  await page.waitForURL((url) => url.pathname === "/customers");
  await page.getByRole("button", { name: "Add Customer" }).first().click();
  await expect(page.getByRole("dialog")).toBeVisible();
});

When("I initiate a profile switch", async ({ page }) => {
  await triggerProfileSwitch(page);
});

When('I click "Open Profile Switcher" on the Settings page', async ({ page }) => {
  await mockSetupStatus(page, "demo");
  await page.goto("/settings/system");
  await page.waitForLoadState("networkidle");
  // The "Open Profile Switcher" button on the System settings page sets
  // profile.openProfileSwitcher = true, which causes the sidebar dropdown to open.
  await page.getByTestId("open-profile-switcher-btn").click();
});

When("I select a different profile", async ({ page }) => {
  await expect(page.getByTestId("profile-dropdown")).toBeVisible();
  await page.getByTestId("profile-entry-production").click();
});

// ────────────────────────────────────────────────────────────────────────────
// Thens
// ────────────────────────────────────────────────────────────────────────────

Then("no unsaved changes dialog appears", async ({ page }) => {
  await expect(page.getByTestId("unsaved-changes-dialog")).not.toBeVisible();
});

Then("the profile switch proceeds immediately", async ({ page }) => {
  // Verify the switch API was called, then that we navigated to "/".
  // Asserting on switchRequests avoids a tautological URL check when the
  // scenario starts on "/" (which would pass even if the switch never fired).
  const state = getState(page);
  await expect.poll(() => state.switchRequests.length, { timeout: 5000 }).toBeGreaterThan(0);
  await page.waitForURL((url) => url.pathname === "/", { timeout: 5000 });
});

Then('the "Unsaved changes" dialog appears', async ({ page }) => {
  await expect(page.getByTestId("unsaved-changes-dialog")).toBeVisible();
});

Then("the profile switch has not been sent yet", async ({ page }) => {
  const state = getState(page);
  expect(state.switchRequests).toHaveLength(0);
});

Then('I see text "You have unsaved changes that will be lost"', async ({ page }) => {
  await expect(
    page
      .getByTestId("unsaved-changes-dialog")
      .getByText(/You have unsaved changes that will be lost/),
  ).toBeVisible();
});

Then("the profile switch request is sent", async ({ page }) => {
  const state = getState(page);
  await expect.poll(() => state.switchRequests.length, { timeout: 5000 }).toBeGreaterThan(0);
});

Then("the dialog closes", async ({ page }) => {
  await expect(page.getByTestId("unsaved-changes-dialog")).not.toBeVisible();
});

Then("the app navigates to the new profile", async ({ page }) => {
  await page.waitForURL((url) => url.pathname === "/", { timeout: 5000 });
});

Then("no profile switch request has been sent", async ({ page }) => {
  const state = getState(page);
  expect(state.switchRequests).toHaveLength(0);
});

Then("I am still on the same page with my form data intact", async ({ page }) => {
  // Should still be on /customers (where the form was)
  expect(page.url()).toContain("/customers");
  // The dialog sheet should still be open (cancel didn't close the sheet)
  await expect(page.getByRole("dialog")).toBeVisible();
  // The typed value should still be in the field
  const nameInput = page.getByLabel("Display Name");
  await expect(nameInput).toHaveValue("Test Customer");
});

Then("the form is tracked as dirty", async ({ page }) => {
  // Evaluate whether profile.dirtyForms has any entries.
  // Since we cannot access Svelte module stores directly from Playwright,
  // we verify indirectly: attempting a profile switch opens the dialog.
  await interceptSwitchRoute(page);
  await triggerProfileSwitch(page);
  await expect(page.getByTestId("unsaved-changes-dialog")).toBeVisible();
  // Cancel so we don't leave the dialog open
  await page.getByTestId("unsaved-changes-cancel-btn").click();
});

Then("the form is no longer tracked as dirty", async ({ page }) => {
  // Intercept and attempt a switch — the dialog should NOT appear
  await interceptSwitchRoute(page);
  await triggerProfileSwitch(page);
  // If dirty, the dialog would appear. If clean, we navigate to "/"
  await page.waitForURL((url) => url.pathname === "/", { timeout: 5000 });
});

Then("profile switching proceeds without the warning dialog", async ({ page }) => {
  await expect(page.getByTestId("unsaved-changes-dialog")).not.toBeVisible();
});

Then(/^the form is not considered dirty \(changes were abandoned\)$/, async ({ page }) => {
  // After navigating away the form was unmounted and formDirty.destroy() ran.
  // Attempt a switch — should proceed without dialog.
  await interceptSwitchRoute(page);
  await triggerProfileSwitch(page);
  await page.waitForURL((url) => url.pathname === "/", { timeout: 5000 });
  // Verify dialog never appeared
  await expect(page.getByTestId("unsaved-changes-dialog")).not.toBeVisible();
});

Then("the unsaved changes dialog appears once", async ({ page }) => {
  await expect(page.getByTestId("unsaved-changes-dialog")).toBeVisible();
  // Ensure there's exactly one dialog (not multiple)
  await expect(page.getByTestId("unsaved-changes-dialog")).toHaveCount(1);
});

Then('clicking "Leave anyway" switches the profile', async ({ page }) => {
  const state = getState(page);
  await page.getByTestId("unsaved-changes-confirm-btn").click();
  await expect.poll(() => state.switchRequests.length, { timeout: 5000 }).toBeGreaterThan(0);
  await page.waitForURL((url) => url.pathname === "/", { timeout: 5000 });
});

Then("the unsaved changes dialog appears", async ({ page }) => {
  await expect(page.getByTestId("unsaved-changes-dialog")).toBeVisible();
});
