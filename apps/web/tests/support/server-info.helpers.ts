import type { Page } from "@playwright/test";
import type { ServerInfoResponse } from "../../src/lib/types/ServerInfoResponse";
import type { HealthResponse } from "../../src/lib/types/HealthResponse";
import { buildHttpUrl } from "./local-server";

const HEALTH_ROUTE = "**/api/health";
const SERVER_INFO_ROUTE = "**/api/server-info";

export const HEALTHY_RESPONSE: HealthResponse = {
  status: "ok",
  version: "0.1.0",
  uptime_seconds: 120,
  database: "ok",
  install_ok: true,
  storage_ok: true,
};

export function buildServerInfo(overrides: Partial<ServerInfoResponse> = {}): ServerInfoResponse {
  return {
    host: "mokumo.local",
    ip_url: buildHttpUrl("192.168.1.42", 3000),
    lan_url: buildHttpUrl("mokumo.local", 3000),
    mdns_active: true,
    port: 3000,
    ...overrides,
  };
}

export async function mockHealth(page: Page): Promise<void> {
  await page.route(HEALTH_ROUTE, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(HEALTHY_RESPONSE),
    });
  });
}

export async function mockServerInfo(page: Page, info: ServerInfoResponse): Promise<void> {
  await page.route(SERVER_INFO_ROUTE, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(info),
    });
  });
}
