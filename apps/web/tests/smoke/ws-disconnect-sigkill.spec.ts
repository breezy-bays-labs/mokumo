import { expect, test } from "../support/lan-client.fixture";

const SIGKILL_BANNER_TIMEOUT = process.env.CI ? 6_000 : 4_000;

test.describe.serial("[SMOKE-02] ws-disconnect-sigkill", () => {
  test("[SMOKE-02] server killed hard (SIGKILL) — disconnect banner appears in browser within timeout", async ({
    freshLanBackend,
    page,
  }) => {
    test.skip(process.platform === "win32", "SIGKILL not available on Windows");

    await page.goto(freshLanBackend.url);
    // Confirm the banner is not visible before we kill the server
    await expect(page.locator('[data-testid="disconnect-banner"]')).not.toBeVisible({
      timeout: 5_000,
    });

    freshLanBackend.killHard();

    // TCP close → ws.onClose → reconnect → banner
    await expect(page.locator('[data-testid="disconnect-banner"]')).toBeVisible({
      timeout: SIGKILL_BANNER_TIMEOUT,
    });
  });
});
