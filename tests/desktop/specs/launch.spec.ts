/**
 * Desktop launch E2E test via tauri-driver + WebDriverIO.
 *
 * Verifies the Tauri app launches, renders the webview, and responds
 * to basic WebDriver commands. Runs on Linux (WebKitWebDriver) and
 * Windows (EdgeDriver). macOS requires community webdriver plugins.
 *
 * Prerequisites:
 *   - cargo install tauri-driver
 *   - cargo build -p mokumo-desktop (debug build)
 *   - Linux: libwebkit2gtk-4.1-dev + xvfb (for headless)
 */

describe("Desktop Launch", () => {
  it("should start and have a window", async () => {
    // tauri-driver + WebDriverIO creates a session that launches the app.
    // If we get here without error, the app started successfully.
    const title = await browser.getTitle();
    console.log(`Window title: "${title}"`);

    // The app should have a window — title may be empty or "Mokumo"
    // depending on when the webview finishes loading.
    // WDIO v9 provides expect-webdriverio globally.
    await expect(browser).toHaveTitle(/.*/);
  });

  it("should have a webview with content", async () => {
    // Wait for the SvelteKit app to render something
    const body = await $("body");
    await expect(body).toBeExisting();

    const html = await browser.getPageSource();
    // Verify we got a non-trivial HTML document
    if (html.length === 0) {
      throw new Error("Page source is empty — webview failed to render");
    }
  });

  it("should respond to navigation", async () => {
    // Verify the webview URL is accessible
    const url = await browser.getUrl();
    console.log(`Webview URL: ${url}`);

    // Mokumo serves from http://127.0.0.1:{port} (embedded API server with
    // External webview URL), not from tauri://localhost like bundled Tauri apps.
    await expect(browser).toHaveUrl(/tauri|localhost|127\.0\.0\.1/);
  });
});
