import { listCustomers } from "$lib/api/customers";
import type { CustomerResponse } from "$lib/types/shop/CustomerResponse";
import type { PaginatedList } from "$lib/types/kikan/PaginatedList";

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
      hasArchivedCustomers: false,
    };
  }

  if ("data" in result) {
    const customers = result.data;
    let hasArchivedCustomers = false;

    // When no active customers and no search filter, check if archived customers exist
    if (customers.total === 0 && !search && !includeDeleted) {
      const archivedCheck = await listCustomers({
        include_deleted: true,
        per_page: 1,
      });
      if (archivedCheck.ok && "data" in archivedCheck) {
        hasArchivedCustomers = archivedCheck.data.total > 0;
      } else if (!archivedCheck.ok) {
        console.error("Failed to check for archived customers:", archivedCheck.error.message);
      }
    }

    return { customers, error: null as string | null, hasArchivedCustomers };
  }

  return {
    customers: null as PaginatedList<CustomerResponse> | null,
    error: "Unexpected empty response",
    hasArchivedCustomers: false,
  };
}
