import type { CustomerFormData } from "$lib/schemas/customer";
import type { CustomerResponse } from "$lib/types/shop/CustomerResponse";

/**
 * Build an update payload from form data and the original customer.
 *
 * Implements the 3-state serialization required by the Rust UpdateCustomer struct
 * (crates/core/src/customer/mod.rs:94-192):
 * - Unchanged field → omit from JSON (backend: outer None → keep current value)
 * - Cleared field (had value, now empty) → send null (backend: Some(None) → set NULL)
 * - Changed field → send new value (backend: Some(Some(v)) → set value)
 *
 * Non-nullable fields (display_name) use simple Option<T> (omit or value, never null).
 * Boolean fields (tax_exempt) also use simple Option<bool>.
 *
 * COUPLING NOTE: This function is coupled to the Rust UpdateCustomer struct.
 * If new clearable fields are added to the backend, they must be handled here.
 * See PATTERNS.md for the sync protocol.
 */
export function buildUpdatePayload(
  formData: CustomerFormData,
  original: CustomerResponse,
): Record<string, unknown> {
  const payload: Record<string, unknown> = {};

  // Non-nullable string: display_name (Option<String>)
  if (formData.display_name !== original.display_name) {
    payload.display_name = formData.display_name;
  }

  // Clearable string fields (Option<Option<String>>)
  const clearableStringFields = [
    "company_name",
    "email",
    "phone",
    "address_line1",
    "address_line2",
    "city",
    "state",
    "postal_code",
    "country",
    "notes",
    "payment_terms",
  ] as const;

  for (const field of clearableStringFields) {
    const formVal = formData[field] ?? "";
    const origVal = original[field] ?? "";

    if (formVal === origVal) continue;

    if (formVal === "" && origVal !== "") {
      // Cleared: send null
      payload[field] = null;
    } else {
      // Changed: send new value
      payload[field] = formVal;
    }
  }

  // Boolean fields (Option<bool>)
  const taxExempt = formData.tax_exempt ?? false;
  if (taxExempt !== original.tax_exempt) {
    payload.tax_exempt = taxExempt;
  }

  // Clearable number: credit_limit_cents (Option<Option<i64>>)
  const formCredit = formData.credit_limit_cents ?? null;
  const origCredit = original.credit_limit_cents ?? null;
  if (formCredit !== origCredit) {
    payload.credit_limit_cents = formCredit === null ? null : formCredit;
  }

  return payload;
}
