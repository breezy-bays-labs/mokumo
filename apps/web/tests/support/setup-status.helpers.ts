import type { Page } from "@playwright/test";
import type { SetupStatusResponse } from "../../src/lib/types/SetupStatusResponse";

const SETUP_STATUS_ROUTE = "**/api/setup-status";

export function buildSetupStatus(
  overrides: Partial<SetupStatusResponse> = {},
): SetupStatusResponse {
  return {
    setup_complete: true,
    setup_mode: "production",
    is_first_launch: false,
    production_setup_complete: false,
    shop_name: null,
    logo_url: null,
    ...overrides,
  };
}

export async function mockSetupStatus(
  page: Page,
  overrides: Partial<SetupStatusResponse> = {},
): Promise<void> {
  const status = buildSetupStatus(overrides);
  await page.unroute(SETUP_STATUS_ROUTE).catch(() => {});
  await page.route(SETUP_STATUS_ROUTE, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(status),
    });
  });
}
