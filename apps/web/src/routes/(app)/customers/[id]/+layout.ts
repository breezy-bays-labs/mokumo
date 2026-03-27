import { getCustomer } from "$lib/api/customers";
import type { CustomerResponse } from "$lib/types/CustomerResponse";

export async function load({ params }) {
  const result = await getCustomer(params.id, true);

  if (!result.ok) {
    return { customer: null as CustomerResponse | null, error: result.error.message };
  }

  if ("data" in result) {
    return { customer: result.data, error: null as string | null };
  }

  return { customer: null as CustomerResponse | null, error: "Customer not found" };
}
