import { getCustomerActivity } from "$lib/api/customers";
import type { ActivityEntryResponse } from "$lib/types/ActivityEntryResponse";
import type { PaginatedList } from "$lib/types/PaginatedList";

export async function load({ params, url, depends }) {
  depends(`activity:customer:${params.id}`);
  const page = Number(url.searchParams.get("page") ?? "1") || 1;
  const perPage = Number(url.searchParams.get("per_page") ?? "20") || 20;

  const result = await getCustomerActivity(params.id, { page, per_page: perPage });

  if (!result.ok) {
    return {
      activity: null as PaginatedList<ActivityEntryResponse> | null,
      error: result.error.message,
    };
  }

  if ("data" in result) {
    return { activity: result.data, error: null as string | null };
  }

  return {
    activity: null as PaginatedList<ActivityEntryResponse> | null,
    error: "Could not load activity",
  };
}
