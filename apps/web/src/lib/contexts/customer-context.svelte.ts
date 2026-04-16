import { getContext, setContext } from "svelte";
import type { CustomerResponse } from "$lib/types/kikan/CustomerResponse";

const CUSTOMER_KEY = Symbol("customer");

export class CustomerContext {
  customer = $state<CustomerResponse | null>(null);
  loading = $state(true);
  error = $state<string | null>(null);

  get isArchived(): boolean {
    return this.customer?.deleted_at !== null && this.customer?.deleted_at !== undefined;
  }
}

export function setCustomerContext(ctx: CustomerContext): void {
  setContext(CUSTOMER_KEY, ctx);
}

export function getCustomerContext(): CustomerContext {
  return getContext<CustomerContext>(CUSTOMER_KEY);
}
