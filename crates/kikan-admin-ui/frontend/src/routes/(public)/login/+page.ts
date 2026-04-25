import { loadSetupStatus } from "$lib/setup-status";

export const load = async () => ({
  setupStatus: await loadSetupStatus(),
});
