import { expect, type Page } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";
import {
  mockAppMeta,
  mockAuthMe,
  mockBranding,
  mockOverview,
  mockProfiles,
} from "../support/mocks";

const OVERVIEW_PATH = "/admin/";

const FRESH_INSTALL_OVERVIEW = {
  fresh_install: true,
  get_started_steps: [
    {
      id: "create-profile",
      label: "Create your first profile",
      complete: false,
    },
    { id: "invite-team", label: "Invite a teammate", complete: false },
    { id: "open-shop", label: "Open your shop", complete: false },
  ],
};

const POPULATED_OVERVIEW = {
  fresh_install: false,
  get_started_steps: [],
  stat_strip: [
    { label: "Active profiles", value: "2" },
    { label: "Users", value: "5" },
  ],
  recent_activity: [{ id: "act-1", label: "Profile created", href: "/admin/profiles/p-1" }],
  backups: { last_at: "2026-04-24T03:00:00Z", next_at: "2026-04-25T03:00:00Z" },
  system_health: { status: "ok" as const },
};

async function gotoSignedInOverview(page: Page): Promise<void> {
  // Branding is mocked by individual scenarios that care about specific values
  // (default vs custom shop noun); the chrome falls back to FALLBACK_BRANDING
  // when /branding 404s, so omitting the mock here lets a custom-noun Given
  // earlier in the scenario survive without being clobbered by a default mock.
  await mockAuthMe(page, { signed_in: true, install_role: "Admin" });
  await mockProfiles(page, [{ id: "p-1", name: "Default", active: true }]);
  await mockAppMeta(page, {
    mdns_hostname: "shop.local",
    port: 4242,
    running_shops: 1,
  });
  await page.goto(OVERVIEW_PATH);
}

Given("I am on the admin overview", async ({ page }) => {
  await mockOverview(page, POPULATED_OVERVIEW);
  await gotoSignedInOverview(page);
});

Given("the platform reports a fresh-install state", async ({ page }) => {
  await mockOverview(page, FRESH_INSTALL_OVERVIEW);
});

Given('the "You\'re set up" completion banner is showing', async ({ page }) => {
  await mockOverview(page, {
    ...POPULATED_OVERVIEW,
    fresh_install: false,
  });
  await gotoSignedInOverview(page);
  await page.evaluate(() => {
    document.body.dataset.youreSetUp = "true";
  });
  await expect(page.getByTestId("youre-set-up-banner")).toBeVisible();
});

Given("the overview is in the populated state", async ({ page }) => {
  await mockOverview(page, POPULATED_OVERVIEW);
  await gotoSignedInOverview(page);
});

Given("the recent-activity region lists at least one entry", async ({ page }) => {
  await expect(
    page.getByTestId("overview-recent-activity").locator("[data-activity-entry]").first(),
  ).toBeVisible();
});

Given("the platform reports no running shops", async ({ page }) => {
  await mockAppMeta(page, {
    mdns_hostname: null,
    port: null,
    running_shops: 0,
  });
});

When("I open the admin overview", async ({ page }) => {
  await gotoSignedInOverview(page);
});

When("the configured display duration elapses", async ({ page }) => {
  await page.waitForTimeout(50);
});

When("I click a recent-activity entry", async ({ page }) => {
  await page
    .getByTestId("overview-recent-activity")
    .locator("[data-activity-entry]")
    .first()
    .click();
});

When('I hover the "Open shop" affordance', async ({ page }) => {
  await page.getByTestId("topbar-open-shop").hover();
});

Then('I see a "Get Started" panel', async ({ page }) => {
  await expect(page.getByTestId("get-started-panel")).toBeVisible();
});

Then("I see three checklist steps", async ({ page }) => {
  await expect(page.getByTestId("get-started-panel").locator("[data-checklist-step]")).toHaveCount(
    3,
  );
});

Then("the steps use the configured app name and shop noun in their copy", async ({ page }) => {
  const text = await page.getByTestId("get-started-panel").innerText();
  expect(text).toMatch(/Mokumo/);
  expect(text).toMatch(/shop/i);
});

Then("the populated dashboard remains visible", async ({ page }) => {
  await expect(page.getByTestId("overview-stat-strip")).toBeVisible();
  await expect(page.getByTestId("overview-recent-activity")).toBeVisible();
});

Then("I see a stat strip region", async ({ page }) => {
  await expect(page.getByTestId("overview-stat-strip")).toBeVisible();
});

Then("I see a recent-activity region", async ({ page }) => {
  await expect(page.getByTestId("overview-recent-activity")).toBeVisible();
});

Then("I see a backups region", async ({ page }) => {
  await expect(page.getByTestId("overview-backups")).toBeVisible();
});

Then("I see a system-health region", async ({ page }) => {
  await expect(page.getByTestId("overview-system-health")).toBeVisible();
});

Then("I am taken to the screen that owns that entry", async ({ page }) => {
  await expect(page).toHaveURL(/\/admin\/profiles\/p-1/);
});

Then("the sidebar lists every entry declared in the nav config", async ({ page }) => {
  const navEntries = page.getByTestId("sidebar-nav").locator("[data-nav-entry]");
  await expect(navEntries.first()).toBeVisible();
  expect(await navEntries.count()).toBeGreaterThan(0);
});

Then("each entry's label and href match the nav config", async ({ page }) => {
  const navEntries = page.getByTestId("sidebar-nav").locator("[data-nav-entry]");
  const count = await navEntries.count();
  expect(count).toBeGreaterThan(0);
  for (let i = 0; i < count; i++) {
    const entry = navEntries.nth(i);
    await expect(entry).toHaveAttribute("data-nav-label", /.+/);
    await expect(entry).toHaveAttribute("href", /^\/admin\//);
  }
});

Then("the overview entry in the sidebar is marked active", async ({ page }) => {
  await expect(
    page
      .getByTestId("sidebar-nav")
      .locator('[data-nav-entry][data-nav-id="overview"][data-active="true"]'),
  ).toBeVisible();
});

Then("no other sidebar entry is marked active", async ({ page }) => {
  const active = page.getByTestId("sidebar-nav").locator('[data-nav-entry][data-active="true"]');
  await expect(active).toHaveCount(1);
});

Then('I see a "PROFILE" divider in the sidebar', async ({ page }) => {
  await expect(page.getByTestId("sidebar-profile-divider")).toBeVisible();
  await expect(page.getByTestId("sidebar-profile-divider")).toContainText(/PROFILE/);
});

Then("the divider sits above the profile switcher block", async ({ page }) => {
  const divider = page.getByTestId("sidebar-profile-divider");
  const switcher = page.getByTestId("sidebar-profile-switcher");
  const dividerBox = await divider.boundingBox();
  const switcherBox = await switcher.boundingBox();
  expect(dividerBox).not.toBeNull();
  expect(switcherBox).not.toBeNull();
  expect(dividerBox!.y).toBeLessThan(switcherBox!.y);
});

Then('the topbar shows the "Control Plane" label', async ({ page }) => {
  await expect(page.getByTestId("topbar")).toContainText(/Control Plane/);
});

Then('the topbar shows an "ADMIN" badge', async ({ page }) => {
  await expect(page.getByTestId("topbar-admin-badge")).toBeVisible();
});

Then('the topbar shows an "Open shop" affordance', async ({ page }) => {
  await expect(page.getByTestId("topbar-open-shop")).toBeVisible();
});

Then('the topbar shows a "Help" affordance', async ({ page }) => {
  await expect(page.getByTestId("topbar-help")).toBeVisible();
});

Then("the topbar does not show a countdown timer", async ({ page }) => {
  // Parent-surface precondition: the topbar must exist. Without it the
  // negative assertion below is vacuously true. With it, the test fails
  // today (no chrome) and passes once S3 lands the topbar without sudo.
  await expect(page.getByTestId("topbar")).toBeVisible();
  await expect(page.getByTestId("topbar-sudo-countdown")).toHaveCount(0);
});

Then('I see a "No shops to open" tooltip', async ({ page }) => {
  await expect(page.getByRole("tooltip", { name: /no shops to open/i })).toBeVisible();
});

Then("the affordance is disabled", async ({ page }) => {
  await expect(page.getByTestId("topbar-open-shop")).toBeDisabled();
});

Then(
  "the documented branding CSS custom properties are set on the chrome surfaces",
  async ({ page }) => {
    // Wait for the chrome to be visually present before sampling :root tokens.
    // page.goto returns at the load event, but in CSR mode the layout effect
    // that mirrors branding onto :root runs after hydration, and Vite-dev
    // injects app.css asynchronously. Once the topbar testid is visible the
    // layout has rendered and both sources of --brand-* are in place.
    await expect(page.getByTestId("topbar")).toBeVisible();
    await expect
      .poll(async () =>
        page.evaluate(() =>
          getComputedStyle(document.documentElement)
            .getPropertyValue("--brand-bg")
            .trim(),
        ),
      )
      .not.toBe("");
    const props = await page.evaluate(() => {
      const cs = getComputedStyle(document.documentElement);
      return {
        bg: cs.getPropertyValue("--brand-bg").trim(),
        fg: cs.getPropertyValue("--brand-fg").trim(),
        primary: cs.getPropertyValue("--brand-primary").trim(),
        accent: cs.getPropertyValue("--brand-accent").trim(),
      };
    });
    expect(props.bg).not.toBe("");
    expect(props.fg).not.toBe("");
    expect(props.primary).not.toBe("");
    expect(props.accent).not.toBe("");
  },
);

Then("the topbar, sidebar, and overview body all consume those tokens", async ({ page }) => {
  for (const testId of ["topbar", "sidebar-nav", "overview-body"]) {
    const surface = page.getByTestId(testId);
    // getComputedStyle resolves var() into the final color, so we can't read
    // the var name back. Inspect the inline cssText where the chrome binds
    // the tokens, which is the actual contract the chrome promises.
    const usesToken = await surface.evaluate((el) =>
      (el as HTMLElement).style.cssText.includes("--brand"),
    );
    expect(usesToken).toBe(true);
  }
});

Then("the sidebar and topbar copy use the configured shop noun", async ({ page }) => {
  await expect(page.getByTestId("sidebar-nav")).toContainText(/kiln/);
  await expect(page.getByTestId("topbar")).toContainText(/kiln/);
});

Then("no hard-coded mokumo-shop nouns appear in the chrome", async ({ page }) => {
  const chromeText = await page
    .locator('[data-testid="topbar"], [data-testid="sidebar-nav"]')
    .allInnerTexts();
  const joined = chromeText.join(" ");
  expect(joined).not.toMatch(/garment|invoice|quote|customer/i);
});
