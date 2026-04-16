import { apiFetch, buildQuery, type ApiResult } from "$lib/api";
import type { ActivityEntryResponse } from "$lib/types/kikan/ActivityEntryResponse";
import type { CustomerResponse } from "$lib/types/kikan/CustomerResponse";
import type { PaginatedList } from "$lib/types/kikan/PaginatedList";

interface ListCustomersParams {
  page?: number;
  per_page?: number;
  search?: string;
  include_deleted?: boolean;
}

interface PaginationParams {
  page?: number;
  per_page?: number;
}

export function listCustomers(
  params: ListCustomersParams = {},
): Promise<ApiResult<PaginatedList<CustomerResponse>>> {
  const query = buildQuery({
    page: params.page,
    per_page: params.per_page,
    search: params.search,
    include_deleted: params.include_deleted,
  });
  return apiFetch<PaginatedList<CustomerResponse>>(`/api/customers${query}`);
}

export function getCustomer(
  id: string,
  includeDeleted = false,
): Promise<ApiResult<CustomerResponse>> {
  const query = includeDeleted ? "?include_deleted=true" : "";
  return apiFetch<CustomerResponse>(`/api/customers/${id}${query}`);
}

export function createCustomer(
  data: Record<string, unknown>,
): Promise<ApiResult<CustomerResponse>> {
  return apiFetch<CustomerResponse>("/api/customers", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  });
}

export function updateCustomer(
  id: string,
  data: Record<string, unknown>,
): Promise<ApiResult<CustomerResponse>> {
  return apiFetch<CustomerResponse>(`/api/customers/${id}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  });
}

export function deleteCustomer(id: string): Promise<ApiResult<CustomerResponse>> {
  return apiFetch<CustomerResponse>(`/api/customers/${id}`, {
    method: "DELETE",
  });
}

export function restoreCustomer(id: string): Promise<ApiResult<CustomerResponse>> {
  return apiFetch<CustomerResponse>(`/api/customers/${id}/restore`, {
    method: "PATCH",
  });
}

export function getEntityActivity(
  entityType: string,
  entityId: string,
  params: PaginationParams = {},
): Promise<ApiResult<PaginatedList<ActivityEntryResponse>>> {
  const query = buildQuery({
    entity_type: entityType,
    entity_id: entityId,
    page: params.page,
    per_page: params.per_page,
  });
  return apiFetch<PaginatedList<ActivityEntryResponse>>(`/api/activity${query}`);
}

export function getCustomerActivity(
  id: string,
  params: PaginationParams = {},
): Promise<ApiResult<PaginatedList<ActivityEntryResponse>>> {
  return getEntityActivity("customer", id, params);
}
