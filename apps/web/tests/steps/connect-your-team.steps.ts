import { expect } from "@playwright/test";
import { Given, Then } from "../support/app.fixture";
import { buildHttpUrl } from "../support/local-server";
import { buildServerInfo, mockHealth, mockServerInfo } from "../support/server-info.helpers";
import { mockSetupStatus } from "../support/setup-status.helpers";

// -- Background --

Given("the server-info API returns LAN status", async ({ page }) => {
  await mockHealth(page);
  await mockServerInfo(page, buildServerInfo());
});

// "I see the {string} card" is in shared-steps.ts

// -- Scenario: QR code encodes the IP-based URL --

Given("the server is running on {word} port {int}", async ({ page }, ip: string, port: number) => {
  const info = buildServerInfo({
    host: ip,
    ip_url: buildHttpUrl(ip, port),
    lan_url: null,
    mdns_active: false,
    port,
  });
  await mockHealth(page);
  await mockServerInfo(page, info);
});

Then("I see a QR code", async ({ page }) => {
  await expect(page.getByTestId("qr-code")).toBeVisible();
});

Then("the QR code encodes {string}", async ({ page }, expectedUrl: string) => {
  const qrValue = await page.getByTestId("qr-code").getAttribute("data-qr-value");
  expect(qrValue).toBe(expectedUrl);
});

// -- Scenario: QR code uses IP even when mDNS is active --

Given("mDNS is active as {string}", async ({ page }, hostname: string) => {
  const info = buildServerInfo({
    mdns_active: true,
    lan_url: buildHttpUrl(hostname, 6565),
    ip_url: buildHttpUrl("192.168.1.50", 6565),
    port: 6565,
  });
  await mockHealth(page);
  await mockServerInfo(page, info);
});

Given("the server IP is {word} on port {int}", async ({ page }, ip: string, port: number) => {
  const info = buildServerInfo({
    mdns_active: true,
    lan_url: buildHttpUrl("mokumo.local", port),
    ip_url: buildHttpUrl(ip, port),
    port,
  });
  await mockHealth(page);
  await mockServerInfo(page, info);
});

// -- Scenario: Copy connection link --

// "I click {string}" is in setup-wizard.steps.ts

Then("the clipboard contains the IP-based URL", async ({ page }) => {
  const clipboardText = await page.evaluate(() => navigator.clipboard.readText());
  expect(clipboardText).toMatch(/^https?:\/\/\d+\.\d+\.\d+\.\d+:\d+$/);
});

// "I see a {string} toast message" is in shared-lan.steps.ts

// -- Scenario: mDNS status shows active/unavailable --

Given("mDNS is active", async ({ page }) => {
  const info = buildServerInfo({ mdns_active: true });
  await mockHealth(page);
  await mockServerInfo(page, info);
});

Given("mDNS is inactive", async ({ page }) => {
  const info = buildServerInfo({
    mdns_active: false,
    lan_url: null,
  });
  await mockHealth(page);
  await mockServerInfo(page, info);
});

Then("I see a green status dot", async ({ page }) => {
  const dot = page.getByTestId("mdns-status-dot");
  await expect(dot).toBeVisible();
  await expect(dot).toHaveClass(/bg-status-success/);
});

Then("the status text reads {string}", async ({ page }, text: string) => {
  await expect(page.getByTestId("mdns-status-text")).toHaveText(text);
});

Then("I see a yellow status dot", async ({ page }) => {
  const dot = page.getByTestId("mdns-status-dot");
  await expect(dot).toBeVisible();
  await expect(dot).toHaveClass(/bg-status-warning/);
});

// -- Scenario: mDNS URL displayed when active --

Given(
  "mDNS is active as {string} on port {int}",
  async ({ page }, hostname: string, port: number) => {
    const info = buildServerInfo({
      mdns_active: true,
      lan_url: buildHttpUrl(hostname, port),
      ip_url: buildHttpUrl("192.168.1.50", port),
      port,
    });
    await mockHealth(page);
    await mockServerInfo(page, info);
  },
);

Then("I see the mDNS URL {string}", async ({ page }, url: string) => {
  await expect(page.getByText(url)).toBeVisible();
});

// -- Scenario: IP URL always displayed --

Then("I see the IP address URL", async ({ page }) => {
  await expect(page.getByTestId("copy-team-url")).toBeVisible();
});

// -- Scenario: Troubleshooting guidance --

Then(
  "I see troubleshooting text mentioning {string} and {string}",
  async ({ page }, term1: string, term2: string) => {
    const text = page.getByTestId("troubleshooting-text");
    await expect(text).toBeVisible();
    await expect(text).toContainText(term1);
    await expect(text).toContainText(term2);
  },
);

// -- Scenario: First-run nudge --

Given("no employee sessions have ever been created", async ({ page }) => {
  await mockSetupStatus(page, { is_first_launch: true });
});

Then("the {string} card has a visual highlight", async ({ page }, _title: string) => {
  const card = page.getByTestId("connect-your-team");
  await expect(card).toHaveClass(/ring-2/);
  await expect(card).toHaveClass(/ring-primary/);
});

Then("I see a {string} badge on the card", async ({ page }, badgeText: string) => {
  const card = page.getByTestId("connect-your-team");
  await expect(card.getByText(badgeText, { exact: true })).toBeVisible();
});

// -- Scenario: First-run nudge disappears --

Given("an employee has connected at least once", async ({ page }) => {
  await mockSetupStatus(page, { is_first_launch: false });
});

Then("the {string} card has no visual highlight", async ({ page }, _title: string) => {
  const card = page.getByTestId("connect-your-team");
  await expect(card).not.toHaveClass(/ring-2/);
});

Then("I do not see a {string} badge", async ({ page }, badgeText: string) => {
  const card = page.getByTestId("connect-your-team");
  await expect(card.getByText(badgeText, { exact: true })).not.toBeVisible();
});
