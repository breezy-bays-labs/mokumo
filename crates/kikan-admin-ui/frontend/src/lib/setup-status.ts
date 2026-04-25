import { fetchPlatform } from "./platform";
import type { SetupStatusResponse } from "./types/kikan/SetupStatusResponse";

export type SetupStatus = SetupStatusResponse;

export async function loadSetupStatus(signal?: AbortSignal): Promise<SetupStatus | undefined> {
  try {
    return await fetchPlatform<SetupStatus>("/setup-status", { signal });
  } catch {
    return undefined;
  }
}
