import { apiFetch, type ApiResult } from "$lib/api";
import type { KikanVersionResponse } from "$lib/types/kikan/KikanVersionResponse";

/**
 * The api_version this SPA was built against. Injected by Vite's `define`
 * config from `KIKAN_ADMIN_UI_BUILT_FOR` in vite.config.ts; the vitest
 * drift guard in `version-check.test.ts` pins that value to
 * `kikan_types::API_VERSION`.
 */
export const ADMIN_UI_BUILT_FOR: string = __KIKAN_ADMIN_UI_BUILT_FOR__;

export type VersionCheckState =
  | { status: "pending" }
  | { status: "match"; serverVersion: string }
  | { status: "mismatch"; uiVersion: string; serverVersion: string }
  | { status: "unreachable" };

type VersionFetcher = () => Promise<ApiResult<KikanVersionResponse>>;

class VersionCheck {
  state: VersionCheckState = $state({ status: "pending" });

  async run(
    fetcher: VersionFetcher = () => apiFetch<KikanVersionResponse>("/api/kikan-version"),
  ): Promise<void> {
    const result = await fetcher();
    if (!result.ok || !("data" in result)) {
      this.state = { status: "unreachable" };
      return;
    }
    const serverVersion = result.data.api_version;
    if (serverVersion === ADMIN_UI_BUILT_FOR) {
      this.state = { status: "match", serverVersion };
    } else {
      this.state = {
        status: "mismatch",
        uiVersion: ADMIN_UI_BUILT_FOR,
        serverVersion,
      };
    }
  }
}

export const versionCheck = new VersionCheck();
