import { expect, test } from "../support/lan-client.fixture";

const BANNER_TIMEOUT = process.env.CI ? 6_000 : 4_000;

test.describe.serial("[SMOKE-01/03/04] server lifecycle", () => {
  test("[SMOKE-01] SIGTERM drain — server shuts down gracefully and disconnect banner appears", async ({
    freshLanBackend,
    page,
  }) => {
    test.skip(process.platform === "win32", "SIGTERM not reliable on Windows");

    await page.goto(freshLanBackend.url);
    await expect(page.locator('[data-testid="disconnect-banner"]')).not.toBeVisible({
      timeout: 5_000,
    });

    // stop() sends SIGTERM and waits up to 12 s for the process to exit
    await freshLanBackend.stop();
    expect(freshLanBackend.exitCode).not.toBeNull();

    // After SIGTERM+drain the close frame is delivered; onClose fires → banner
    await expect(page.locator('[data-testid="disconnect-banner"]')).toBeVisible({
      timeout: BANNER_TIMEOUT,
    });
  });

  test("[SMOKE-03] restart on same port — client reconnects after page.reload()", async ({
    freshLanBackend,
    page,
  }) => {
    test.skip(process.platform === "win32", "SIGKILL not available on Windows");

    await page.goto(freshLanBackend.url);
    await expect(page.locator('[data-testid="disconnect-banner"]')).not.toBeVisible({
      timeout: 5_000,
    });

    const port = freshLanBackend.port;
    freshLanBackend.killHard();

    // Banner appears after SIGKILL (TCP close path)
    await expect(page.locator('[data-testid="disconnect-banner"]')).toBeVisible({
      timeout: BANNER_TIMEOUT,
    });

    // Restart the server on the same port so the browser's reconnect loop
    // can reach it at the original URL. page.reload() re-initialises the WS.
    await freshLanBackend.start(port);
    await page.reload();

    // After reload the WS reconnects — banner should not be visible
    await expect(page.locator('[data-testid="disconnect-banner"]')).not.toBeVisible({
      timeout: 10_000,
    });
  });

  test("[SMOKE-04] stop() drains within ceiling — no zombie process", async ({
    freshLanBackend,
  }) => {
    await freshLanBackend.stop();
    expect(freshLanBackend.exitCode).not.toBeNull();
  });
});
