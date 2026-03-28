import { listCustomers } from "$lib/api/customers";
import type { CustomerResponse } from "$lib/types/CustomerResponse";
import type { PaginatedList } from "$lib/types/PaginatedList";

export async function load({ url, depends }) {
  depends("app:customers");
  const search = url.searchParams.get("search") ?? "";
  const page = Number(url.searchParams.get("page") ?? "1") || 1;
  const perPage = Number(url.searchParams.get("per_page") ?? "25") || 25;
  const includeDeleted = url.searchParams.get("include_deleted") === "true";

  const result = await listCustomers({
    search: search || undefined,
    page,
    per_page: perPage,
    include_deleted: includeDeleted || undefined,
  });

  if (!result.ok) {
    return {
      customers: null as PaginatedList<CustomerResponse> | null,
      error: result.error.message,
    };
  }

  if ("data" in result) {
    return { customers: result.data, error: null as string | null };
  }

  return {
    customers: null as PaginatedList<CustomerResponse> | null,
    error: "Unexpected empty response",
  };
}
