import { loadBranding } from "$lib/branding";

export const load = async () => ({
  branding: await loadBranding(),
});
