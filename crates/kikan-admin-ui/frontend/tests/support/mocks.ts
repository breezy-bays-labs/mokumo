import type { Page } from "@playwright/test";

/**
 * Centralized fetch mocks for the chrome BDD harness.
 *
 * The chrome talks to the platform through `fetchPlatform()` (single call
 * site, CQO-A6); these helpers intercept those routes via Playwright's
 * `page.route()`. Step defs call them in Given/When; the chrome itself
 * does not exist yet (S2 RED), so the mocks are forward-compatible — once
 * the chrome lands they keep step defs deterministic.
 */

export const PLATFORM_BRANDING = "**/api/platform/v1/branding";
export const PLATFORM_SETUP_STATUS = "**/api/platform/v1/setup-status";
export const PLATFORM_AUTH_ME = "**/api/platform/v1/auth/me";
export const PLATFORM_PROFILES = "**/api/platform/v1/profiles";
export const PLATFORM_APP_META = "**/api/platform/v1/app-meta";
export const PLATFORM_OVERVIEW = "**/api/platform/v1/overview";

export type BrandingFixture = {
  app_name: string;
  shop_noun_singular: string;
  shop_noun_plural: string;
  logo_url: string | null;
  accent_color: string;
};

export const DEFAULT_BRANDING: BrandingFixture = {
  app_name: "Mokumo",
  shop_noun_singular: "shop",
  shop_noun_plural: "shops",
  logo_url: null,
  accent_color: "#6366f1",
};

export async function mockBranding(
  page: Page,
  branding: Partial<BrandingFixture> = {},
): Promise<void> {
  const body = { ...DEFAULT_BRANDING, ...branding };
  await page.route(PLATFORM_BRANDING, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(body),
    });
  });
}

export async function mockSetupStatus(
  page: Page,
  status: {
    setup_complete: boolean;
    setup_mode?: "demo" | "production" | null;
    is_first_launch?: boolean;
    production_setup_complete?: boolean;
    shop_name?: string | null;
    logo_url?: string | null;
  },
): Promise<void> {
  const body = {
    setup_mode: "production" as const,
    is_first_launch: !status.setup_complete,
    production_setup_complete: status.setup_complete,
    shop_name: null,
    logo_url: null,
    ...status,
  };
  await page.route(PLATFORM_SETUP_STATUS, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(body),
    });
  });
}

export async function mockAuthMe(
  page: Page,
  me: {
    signed_in: boolean;
    install_role?: "Owner" | "Admin" | "None";
    user_id?: string;
  } = {
    signed_in: false,
  },
): Promise<void> {
  await page.route(PLATFORM_AUTH_ME, async (route) => {
    if (!me.signed_in) {
      await route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({
          code: "unauthenticated",
          message: "Not signed in",
          details: null,
        }),
      });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        user_id: me.user_id ?? "user-1",
        install_role: me.install_role ?? "Admin",
      }),
    });
  });
}

export async function mockOffline(page: Page): Promise<void> {
  await page.route("**/api/**", async (route) => {
    await route.abort("internetdisconnected");
  });
}

export async function mockOnline(page: Page): Promise<void> {
  await page.unroute("**/api/**");
}

export async function mockPlatformError(page: Page, status: number): Promise<void> {
  await page.route("**/api/platform/v1/**", async (route) => {
    await route.fulfill({
      status,
      contentType: "application/json",
      body: JSON.stringify({
        code: status === 401 ? "unauthenticated" : status === 403 ? "forbidden" : "server_error",
        message: `HTTP ${status}`,
        details: null,
      }),
      headers: status === 429 ? { "Retry-After": "30" } : {},
    });
  });
}

export async function mockProfiles(
  page: Page,
  profiles: Array<{ id: string; name: string; active: boolean }>,
): Promise<void> {
  await page.route(PLATFORM_PROFILES, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ profiles }),
    });
  });
}

export async function mockAppMeta(
  page: Page,
  meta: {
    mdns_hostname: string | null;
    port: number | null;
    running_shops: number;
  },
): Promise<void> {
  await page.route(PLATFORM_APP_META, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(meta),
    });
  });
}

export async function mockOverview(
  page: Page,
  overview: {
    fresh_install: boolean;
    get_started_steps: Array<{ id: string; label: string; complete: boolean }>;
    stat_strip?: Array<{ label: string; value: string }>;
    recent_activity?: Array<{ id: string; label: string; href: string }>;
    backups?: { last_at: string | null; next_at: string | null };
    system_health?: { status: "ok" | "degraded" | "down" };
  },
): Promise<void> {
  await page.route(PLATFORM_OVERVIEW, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(overview),
    });
  });
}
