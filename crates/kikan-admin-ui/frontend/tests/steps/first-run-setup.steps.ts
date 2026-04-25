import { expect, type Page } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";
import { mockAppMeta, mockBranding, mockSetupStatus } from "../support/mocks";

const SETUP_PATH = "/admin/setup";
const SETUP_TOKEN = "setup-token-abc123";
const MDNS_HOSTNAME = "kiln-room.local";
const MDNS_PORT = 4242;

async function gotoWizard(page: Page, withToken: boolean): Promise<void> {
  await mockBranding(page);
  await mockSetupStatus(page, {
    setup_complete: false,
    setup_mode: "production",
  });
  const url = withToken ? `${SETUP_PATH}?setup_token=${SETUP_TOKEN}` : SETUP_PATH;
  await page.goto(url);
}

Given("the setup wizard is opened with a valid setup token", async ({ page }) => {
  await gotoWizard(page, true);
});

Given("the setup wizard is opened with a setup token in the URL", async ({ page }) => {
  await gotoWizard(page, true);
});

Given("the setup wizard is opened without a setup token in the URL", async ({ page }) => {
  await gotoWizard(page, false);
});

Given("I am on the create-profile step", async ({ page }) => {
  await gotoWizard(page, true);
  await page.getByTestId("wizard-step-create-profile").click();
});

Given("I have entered a profile name", async ({ page }) => {
  await page.getByLabel(/profile name/i).fill("Default");
});

Given("I am on the finish step", async ({ page }) => {
  await gotoWizard(page, true);
  await page.getByTestId("wizard-step-finish").click();
});

Given("the platform reports an mDNS hostname and port", async ({ page }) => {
  await mockAppMeta(page, {
    mdns_hostname: MDNS_HOSTNAME,
    port: MDNS_PORT,
    running_shops: 1,
  });
});

When("I am on the welcome step", async ({ page }) => {
  await expect(page.getByTestId("wizard-step-welcome")).toHaveAttribute("data-active", "true");
});

When("I reach the create-admin step", async ({ page }) => {
  await page.getByTestId("wizard-step-create-admin").click();
});

When("I copy the shop URL", async ({ page }) => {
  await page.getByRole("button", { name: /copy shop url/i }).click();
});

When("I try to navigate away from the wizard", async ({ page }) => {
  // Click an internal link the wizard renders for cancellation. Hard browser
  // navigations (page.goto / typing URL) cannot trigger a custom Dialog —
  // they only fire native beforeunload. The wizard surfaces "Back to sign-in"
  // as the in-app exit, and that's the affordance the user clicks.
  await page.getByTestId("wizard-cancel-link").click();
});

Then("I see a four-step progress indicator", async ({ page }) => {
  await expect(page.getByTestId("wizard-progress")).toBeVisible();
  await expect(page.getByTestId("wizard-progress").locator("[data-step]")).toHaveCount(4);
});

Then(
  'the steps are "Welcome", "Create admin", "Create profile", and "Finish"',
  async ({ page }) => {
    const steps = page.getByTestId("wizard-progress").locator("[data-step]");
    await expect(steps.nth(0)).toContainText(/welcome/i);
    await expect(steps.nth(1)).toContainText(/create admin/i);
    await expect(steps.nth(2)).toContainText(/create profile/i);
    await expect(steps.nth(3)).toContainText(/finish/i);
  },
);

Then("I see a welcome message", async ({ page }) => {
  await expect(page.getByTestId("wizard-welcome-message")).toBeVisible();
});

Then("I see that the setup token has already been accepted", async ({ page }) => {
  await expect(page.getByTestId("wizard-token-accepted")).toBeVisible();
});

Then('I do not see a separate "Verify token" step', async ({ page }) => {
  const steps = page.getByTestId("wizard-progress").locator("[data-step]");
  await expect(steps).toHaveCount(4);
  await expect(steps.filter({ hasText: /verify token/i })).toHaveCount(0);
});

Then("I do not see the setup token field", async ({ page }) => {
  await expect(page.getByLabel(/setup token/i)).toHaveCount(0);
});

Then("the create-admin form shows name, email, and password fields", async ({ page }) => {
  await expect(page.getByLabel("Name")).toBeVisible();
  await expect(page.getByLabel("Email")).toBeVisible();
  await expect(page.getByLabel("Password")).toBeVisible();
});

Then("I see the setup token field", async ({ page }) => {
  await expect(page.getByLabel(/setup token/i)).toBeVisible();
});

Then("the field helper text tells me where to find the token in my terminal", async ({ page }) => {
  await expect(page.getByTestId("setup-token-helper")).toContainText(/terminal|cli/i);
});

Then("the clipboard contains the shop URL", async ({ page }) => {
  // The copy handler is async (fetches /app-meta then writes the resolved
  // URL). page.click resolves on the click, not on handler completion, so
  // poll the clipboard until the write lands.
  await expect
    .poll(async () => page.evaluate(() => navigator.clipboard.readText()), { timeout: 5_000 })
    .not.toBe("");
  const text = await page.evaluate(() => navigator.clipboard.readText());
  expect(text).toContain(MDNS_HOSTNAME);
  expect(text).toContain(String(MDNS_PORT));
});

Then('I see a "URL copied to clipboard" toast', async ({ page }) => {
  await expect(page.getByText(/url copied to clipboard/i)).toBeVisible();
});

Then('I am asked "Leave setup?"', async ({ page }) => {
  await expect(page.getByRole("dialog", { name: /leave setup\?/i })).toBeVisible();
});

Then("I can choose to stay on the wizard", async ({ page }) => {
  await expect(page.getByRole("button", { name: /stay on wizard|cancel/i })).toBeVisible();
});
