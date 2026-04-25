import { fetchPlatform } from "./platform";

export interface SetupStatus {
  admin_exists: boolean;
  setup_complete: boolean;
  setup_mode?: "production" | "cli";
}

export async function loadSetupStatus(signal?: AbortSignal): Promise<SetupStatus | undefined> {
  try {
    return await fetchPlatform<SetupStatus>("/setup-status", { signal });
  } catch {
    return undefined;
  }
}
