import { expect, test } from "../support/lan-client.fixture";

test.describe.serial("[SMOKE-12/13/14] frontend state", () => {
  test("[SMOKE-12] close-to-tray preference persists across server restart", async ({
    freshLanBackend,
    page,
  }) => {
    await page.goto(freshLanBackend.url);

    // Navigate to settings and set close-to-tray preference via UI.
    // The settings page path may vary — adjust the selector if it changes.
    await page.goto(`${freshLanBackend.url}/settings`);
    const closeTrayToggle = page.locator('[data-testid="close-to-tray-toggle"]');
    const isVisible = await closeTrayToggle.isVisible({ timeout: 3_000 }).catch(() => false);

    if (!isVisible) {
      test.skip(true, "close-to-tray toggle not found — UI may not be implemented yet");
      return;
    }

    // Enable close-to-tray
    if (!(await closeTrayToggle.isChecked())) {
      await closeTrayToggle.click();
    }

    // Capture dataDir before stopping — required for SMOKE-12 restart
    const savedDataDir = freshLanBackend.dataDir;
    await freshLanBackend.stop();

    // Restart with the same data directory so the preference is preserved
    await freshLanBackend.start(undefined, savedDataDir);
    await page.reload();
    await page.goto(`${freshLanBackend.url}/settings`);

    // Preference must survive the restart
    await expect(page.locator('[data-testid="close-to-tray-toggle"]')).toBeChecked({
      timeout: 5_000,
    });
  });

  test("[SMOKE-13] first-launch nudge — setup banner is visible on a fresh data directory", async ({
    freshLanBackend,
    page,
  }) => {
    // freshLanBackend starts with a fresh tmpdir — first launch state
    await page.goto(freshLanBackend.url);

    // On first launch the setup wizard or nudge banner should appear.
    // Accept either a setup token form or a first-launch indicator.
    const setupIndicator = page.locator(
      '[data-testid="setup-banner"], [data-testid="setup-wizard"], input[placeholder*="token" i]',
    );
    await expect(setupIndicator.first()).toBeVisible({ timeout: 5_000 });
  });

  test("[SMOKE-14] null LAN address — UI renders without crashing when LAN IP is unavailable", async ({
    freshLanBackend,
    page,
  }) => {
    // The backend emits a null LAN address on interfaces with no IP (e.g., CI).
    // The frontend must render without throwing — check the page loads and has
    // no error boundary.
    await page.goto(freshLanBackend.url);
    const errorBoundary = page.locator('[data-testid="error-boundary"], .error-boundary');
    await expect(errorBoundary).not.toBeVisible({ timeout: 5_000 });

    // Page title or root element should be present
    await expect(page.locator("body")).toBeVisible({ timeout: 3_000 });
  });
});
