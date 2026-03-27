import { redirect } from "@sveltejs/kit";
import type { MeResponse } from "$lib/types/MeResponse";
import type { LayoutLoad } from "./$types";

type SetupStatusResponse = {
  setup_complete: boolean;
};

export const load: LayoutLoad = async ({ fetch }) => {
  const statusRes = await fetch("/api/setup-status");
  if (statusRes.ok) {
    const status = (await statusRes.json()) as SetupStatusResponse;
    if (!status.setup_complete) {
      throw redirect(307, "/setup");
    }
  }

  const res = await fetch("/api/auth/me");
  if (!res.ok) {
    throw redirect(307, "/login");
  }

  const data = (await res.json()) as MeResponse;
  return { user: data.user };
};
