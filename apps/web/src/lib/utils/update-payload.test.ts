import { describe, expect, it } from "vitest";
import { buildUpdatePayload } from "./update-payload";
import type { CustomerFormData } from "$lib/schemas/customer";
import type { CustomerResponse } from "$lib/types/kikan/CustomerResponse";

function makeCustomer(overrides: Partial<CustomerResponse> = {}): CustomerResponse {
  return {
    id: "test-id",
    display_name: "Acme Corp",
    company_name: "Acme LLC",
    email: "info@acme.com",
    phone: "(555) 123-4567",
    address_line1: "123 Main St",
    address_line2: null,
    city: "Springfield",
    state: "IL",
    postal_code: "62701",
    country: "US",
    notes: "Good customer",
    portal_enabled: false,
    portal_user_id: null,
    tax_exempt: false,
    tax_exemption_certificate_path: null,
    tax_exemption_expires_at: null,
    payment_terms: "net_30",
    credit_limit_cents: 50000,
    stripe_customer_id: null,
    quickbooks_customer_id: null,
    lead_source: null,
    tags: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    deleted_at: null,
    ...overrides,
  };
}

function makeFormData(overrides: Partial<CustomerFormData> = {}): CustomerFormData {
  return {
    display_name: "Acme Corp",
    company_name: "Acme LLC",
    email: "info@acme.com",
    phone: "(555) 123-4567",
    address_line1: "123 Main St",
    address_line2: "",
    city: "Springfield",
    state: "IL",
    postal_code: "62701",
    country: "US",
    notes: "Good customer",
    payment_terms: "net_30",
    tax_exempt: false,
    credit_limit_cents: 50000,
    ...overrides,
  };
}

describe("buildUpdatePayload", () => {
  it("returns empty object when nothing changed", () => {
    const payload = buildUpdatePayload(makeFormData(), makeCustomer());
    expect(payload).toEqual({});
  });

  it("includes display_name when changed (non-nullable)", () => {
    const payload = buildUpdatePayload(makeFormData({ display_name: "New Name" }), makeCustomer());
    expect(payload).toEqual({ display_name: "New Name" });
  });

  it("sends new value when clearable string field is changed", () => {
    const payload = buildUpdatePayload(makeFormData({ email: "new@acme.com" }), makeCustomer());
    expect(payload).toEqual({ email: "new@acme.com" });
  });

  it("sends null when clearable string field is cleared", () => {
    const payload = buildUpdatePayload(
      makeFormData({ email: "" }),
      makeCustomer({ email: "info@acme.com" }),
    );
    expect(payload).toEqual({ email: null });
  });

  it("omits clearable string field when unchanged", () => {
    const payload = buildUpdatePayload(makeFormData(), makeCustomer());
    expect(payload).not.toHaveProperty("email");
    expect(payload).not.toHaveProperty("company_name");
  });

  it("handles field going from null to value", () => {
    const payload = buildUpdatePayload(
      makeFormData({ address_line2: "Suite 200" }),
      makeCustomer({ address_line2: null }),
    );
    expect(payload).toEqual({ address_line2: "Suite 200" });
  });

  it("treats both null original and empty form as no change", () => {
    const payload = buildUpdatePayload(
      makeFormData({ address_line2: "" }),
      makeCustomer({ address_line2: null }),
    );
    expect(payload).not.toHaveProperty("address_line2");
  });

  it("includes tax_exempt when changed", () => {
    const payload = buildUpdatePayload(
      makeFormData({ tax_exempt: true }),
      makeCustomer({ tax_exempt: false }),
    );
    expect(payload).toEqual({ tax_exempt: true });
  });

  it("omits tax_exempt when unchanged", () => {
    const payload = buildUpdatePayload(
      makeFormData({ tax_exempt: false }),
      makeCustomer({ tax_exempt: false }),
    );
    expect(payload).not.toHaveProperty("tax_exempt");
  });

  it("sends new credit_limit_cents when changed", () => {
    const payload = buildUpdatePayload(
      makeFormData({ credit_limit_cents: 100000 }),
      makeCustomer({ credit_limit_cents: 50000 }),
    );
    expect(payload).toEqual({ credit_limit_cents: 100000 });
  });

  it("sends null for credit_limit_cents when cleared", () => {
    const payload = buildUpdatePayload(
      makeFormData({ credit_limit_cents: undefined }),
      makeCustomer({ credit_limit_cents: 50000 }),
    );
    expect(payload).toEqual({ credit_limit_cents: null });
  });

  it("omits credit_limit_cents when unchanged", () => {
    const payload = buildUpdatePayload(
      makeFormData({ credit_limit_cents: 50000 }),
      makeCustomer({ credit_limit_cents: 50000 }),
    );
    expect(payload).not.toHaveProperty("credit_limit_cents");
  });

  it("handles multiple changes at once", () => {
    const payload = buildUpdatePayload(
      makeFormData({
        display_name: "New Corp",
        email: "",
        phone: "(555) 999-0000",
        tax_exempt: true,
      }),
      makeCustomer(),
    );
    expect(payload).toEqual({
      display_name: "New Corp",
      email: null,
      phone: "(555) 999-0000",
      tax_exempt: true,
    });
  });

  it("handles all clearable fields being cleared", () => {
    const payload = buildUpdatePayload(
      makeFormData({
        company_name: "",
        email: "",
        phone: "",
        address_line1: "",
        city: "",
        state: "",
        postal_code: "",
        country: "",
        notes: "",
        payment_terms: "",
        credit_limit_cents: undefined,
      }),
      makeCustomer(),
    );
    expect(payload.company_name).toBeNull();
    expect(payload.email).toBeNull();
    expect(payload.phone).toBeNull();
    expect(payload.address_line1).toBeNull();
    expect(payload.city).toBeNull();
    expect(payload.state).toBeNull();
    expect(payload.postal_code).toBeNull();
    expect(payload.country).toBeNull();
    expect(payload.notes).toBeNull();
    expect(payload.payment_terms).toBeNull();
    expect(payload.credit_limit_cents).toBeNull();
  });
});
