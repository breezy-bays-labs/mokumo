import { describe, it, expect } from "vitest";
import { CustomerContext } from "./customer-context.svelte";

describe("CustomerContext", () => {
  it("initializes with null customer", () => {
    const ctx = new CustomerContext();
    expect(ctx.customer).toBeNull();
  });

  it("initializes with loading true", () => {
    const ctx = new CustomerContext();
    expect(ctx.loading).toBe(true);
  });

  it("initializes with null error", () => {
    const ctx = new CustomerContext();
    expect(ctx.error).toBeNull();
  });

  describe("isArchived", () => {
    it("returns false when customer is null", () => {
      const ctx = new CustomerContext();
      ctx.customer = null;
      expect(ctx.isArchived).toBe(false);
    });

    it("returns false when customer has no deleted_at", () => {
      const ctx = new CustomerContext();
      ctx.customer = makeCustomer({ deleted_at: null });
      expect(ctx.isArchived).toBe(false);
    });

    it("returns true when customer has deleted_at set", () => {
      const ctx = new CustomerContext();
      ctx.customer = makeCustomer({ deleted_at: "2026-03-01T00:00:00Z" });
      expect(ctx.isArchived).toBe(true);
    });

    it("returns false when customer has undefined deleted_at", () => {
      const ctx = new CustomerContext();
      ctx.customer = makeCustomer({ deleted_at: undefined as unknown as null });
      expect(ctx.isArchived).toBe(false);
    });
  });

  it("allows setting customer", () => {
    const ctx = new CustomerContext();
    const customer = makeCustomer();
    ctx.customer = customer;
    expect(ctx.customer).toBe(customer);
  });

  it("allows setting loading", () => {
    const ctx = new CustomerContext();
    ctx.loading = false;
    expect(ctx.loading).toBe(false);
  });

  it("allows setting error", () => {
    const ctx = new CustomerContext();
    ctx.error = "Something went wrong";
    expect(ctx.error).toBe("Something went wrong");
  });
});

function makeCustomer(
  overrides: Partial<{
    id: string;
    display_name: string;
    deleted_at: string | null;
  }> = {},
) {
  return {
    id: "test-id",
    display_name: "Acme Corp",
    company_name: null,
    email: null,
    phone: null,
    address_line1: null,
    address_line2: null,
    city: null,
    state: null,
    postal_code: null,
    country: null,
    notes: null,
    portal_enabled: false,
    portal_user_id: null,
    tax_exempt: false,
    tax_exemption_certificate_path: null,
    tax_exemption_expires_at: null,
    payment_terms: null,
    credit_limit_cents: null,
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
