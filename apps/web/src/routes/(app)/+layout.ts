import type { LayoutLoad } from "./$types";

export const load: LayoutLoad = async ({ fetch }) => {
  try {
    // Check setup status first (no auth required)
    const statusRes = await fetch("/api/setup-status");
    if (statusRes.ok) {
      const status = await statusRes.json();
      if (!status.setup_complete) {
        return { redirect: "/setup" as const, user: null };
      }
    }

    // Then check auth
    const res = await fetch("/api/auth/me");
    if (!res.ok) {
      return { redirect: "/login" as const, user: null };
    }
    const data = await res.json();
    return { user: data.user, redirect: null };
  } catch {
    return { redirect: "/login" as const, user: null };
  }
};
