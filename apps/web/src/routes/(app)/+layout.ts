import { redirect } from "@sveltejs/kit";
import type { MeResponse } from "$lib/types/kikan/MeResponse";
import type { SetupStatusResponse } from "$lib/types/kikan/SetupStatusResponse";
import type { LayoutLoad } from "./$types";

export const load: LayoutLoad = async ({ fetch }) => {
  const statusRes = await fetch("/api/setup-status");
  if (!statusRes.ok) {
    // Server not ready yet (Tauri startup race) or unreachable. Route to /welcome so its
    // retry loop can recover — this avoids a hard error screen before the server is up.
    throw redirect(307, "/welcome");
  }

  const status = (await statusRes.json()) as SetupStatusResponse;

  if (status.is_first_launch) {
    throw redirect(307, "/welcome");
  }
  if (!status.setup_complete) {
    throw redirect(307, "/setup");
  }

  const res = await fetch("/api/auth/me");
  if (!res.ok) {
    throw redirect(307, "/login");
  }

  const data = (await res.json()) as MeResponse;
  return {
    user: data.user,
    recovery_codes_remaining: data.recovery_codes_remaining,
    setup_mode: status.setup_mode,
    production_setup_complete: status.production_setup_complete,
    shop_name: status.shop_name,
    is_first_launch: status.is_first_launch,
    logo_url: status.logo_url,
  };
};
