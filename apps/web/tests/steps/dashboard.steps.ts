import { expect } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";
import { buildHttpUrl } from "../support/local-server";
import { buildServerInfo, mockHealth, mockServerInfo } from "../support/server-info.helpers";

Given(
  "the server has mDNS active with hostname {string} on port {int}",
  async ({ page }, hostname: string, port: number) => {
    const info = buildServerInfo({
      host: hostname,
      lan_url: buildHttpUrl(hostname, port),
      ip_url: buildHttpUrl("192.168.1.42", port),
      mdns_active: true,
      port,
    });
    await mockHealth(page);
    await mockServerInfo(page, info);
  },
);

Given(
  "the server has mDNS inactive with IP {string} on port {int}",
  async ({ page }, ip: string, port: number) => {
    const info = buildServerInfo({
      host: ip,
      lan_url: null,
      ip_url: buildHttpUrl(ip, port),
      mdns_active: false,
      port,
    });
    await mockHealth(page);
    await mockServerInfo(page, info);
  },
);

Given("the server has no LAN access", async ({ page }) => {
  const info = buildServerInfo({
    host: "localhost",
    lan_url: null,
    ip_url: null,
    mdns_active: false,
  });
  await mockHealth(page);
  await mockServerInfo(page, info);
});

Given("the server is healthy", async ({ page }) => {
  await mockHealth(page);
  await mockServerInfo(page, buildServerInfo());
});

Given("I am on the dashboard", async ({ page, appUrl }) => {
  await page.goto(new URL("/", appUrl).toString());
  await expect(page.getByRole("heading", { name: "Your Shop" })).toBeVisible();
});

When("I navigate to the dashboard", async ({ page, appUrl }) => {
  await page.goto(new URL("/", appUrl).toString());
  await expect(page.getByRole("heading", { name: "Your Shop" })).toBeVisible();
});

Then("I see the {string} card", async ({ page }, cardTitle: string) => {
  await expect(page.getByText(cardTitle)).toBeVisible();
});

Then("I see {string}", async ({ page }, text: string) => {
  await expect(page.getByText(text)).toBeVisible();
});

Then("I do not see the {string} card", async ({ page }, cardTitle: string) => {
  await expect(page.getByText(cardTitle)).not.toBeVisible();
});

Then("the clipboard contains {string}", async ({ page }, expected: string) => {
  await expect.poll(async () => page.evaluate(() => navigator.clipboard.readText())).toBe(expected);
});

Then("I see the server status as {string}", async ({ page }, status: string) => {
  await expect(page.getByText(status)).toBeVisible();
});
