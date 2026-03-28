import { redirect } from "@sveltejs/kit";
import type { MeResponse } from "$lib/types/MeResponse";
import type { SetupStatusResponse } from "$lib/types/SetupStatusResponse";
import type { LayoutLoad } from "./$types";

export const load: LayoutLoad = async ({ fetch }) => {
  const statusRes = await fetch("/api/setup-status");
  let setupMode: SetupStatusResponse["setup_mode"] = null;

  if (statusRes.ok) {
    const status = (await statusRes.json()) as SetupStatusResponse;
    setupMode = status.setup_mode;
    if (!status.setup_complete) {
      throw redirect(307, "/setup");
    }
  }

  const res = await fetch("/api/auth/me");
  if (!res.ok) {
    throw redirect(307, "/login");
  }

  const data = (await res.json()) as MeResponse;
  return {
    user: data.user,
    recovery_codes_remaining: data.recovery_codes_remaining,
    setup_mode: setupMode,
  };
};
