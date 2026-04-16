import { getCustomer } from "$lib/api/customers";
import type { CustomerResponse } from "$lib/types/kikan/CustomerResponse";

export async function load({ params, depends }) {
  depends(`customer:${params.id}`);
  const result = await getCustomer(params.id, true);

  if (!result.ok) {
    return { customer: null as CustomerResponse | null, error: result.error.message };
  }

  if ("data" in result) {
    return { customer: result.data, error: null as string | null };
  }

  return { customer: null as CustomerResponse | null, error: "Customer not found" };
}
