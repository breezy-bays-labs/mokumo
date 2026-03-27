import { describe, it, expect } from "vitest";
import { customerFormSchema, customerListParamsSchema, PAYMENT_TERMS_OPTIONS } from "./customer";

describe("PAYMENT_TERMS_OPTIONS", () => {
  it("contains the expected payment term values", () => {
    const values = PAYMENT_TERMS_OPTIONS.map((o) => o.value);
    expect(values).toEqual(["due_on_receipt", "net_15", "net_30", "net_60"]);
  });

  it("has labels for each option", () => {
    for (const option of PAYMENT_TERMS_OPTIONS) {
      expect(option.label).toBeTruthy();
    }
  });
});

describe("customerFormSchema", () => {
  it("accepts valid minimal data (display_name only)", () => {
    const result = customerFormSchema.safeParse({ display_name: "Acme Corp" });
    expect(result.success).toBe(true);
  });

  it("rejects empty display_name", () => {
    const result = customerFormSchema.safeParse({ display_name: "" });
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0].message).toBe("Display name is required");
    }
  });

  it("rejects missing display_name", () => {
    const result = customerFormSchema.safeParse({});
    expect(result.success).toBe(false);
  });

  it("trims whitespace from display_name", () => {
    const result = customerFormSchema.safeParse({ display_name: "  Acme Corp  " });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.display_name).toBe("Acme Corp");
    }
  });

  it("rejects whitespace-only display_name", () => {
    const result = customerFormSchema.safeParse({ display_name: "   " });
    expect(result.success).toBe(false);
  });

  it("validates email format when provided", () => {
    const result = customerFormSchema.safeParse({
      display_name: "Test",
      email: "not-an-email",
    });
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0].message).toBe("Invalid email address");
    }
  });

  it("accepts valid email", () => {
    const result = customerFormSchema.safeParse({
      display_name: "Test",
      email: "user@example.com",
    });
    expect(result.success).toBe(true);
  });

  it("accepts empty string for email (allows clearing)", () => {
    const result = customerFormSchema.safeParse({
      display_name: "Test",
      email: "",
    });
    expect(result.success).toBe(true);
  });

  it("accepts all optional string fields", () => {
    const result = customerFormSchema.safeParse({
      display_name: "Test",
      company_name: "Test LLC",
      phone: "(555) 123-4567",
      address_line1: "123 Main St",
      address_line2: "Suite 200",
      city: "Springfield",
      state: "IL",
      postal_code: "62701",
      country: "US",
      notes: "Good customer",
      payment_terms: "net_30",
    });
    expect(result.success).toBe(true);
  });

  it("accepts tax_exempt boolean", () => {
    const result = customerFormSchema.safeParse({
      display_name: "Test",
      tax_exempt: true,
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.tax_exempt).toBe(true);
    }
  });

  it("coerces credit_limit_cents from string to number", () => {
    const result = customerFormSchema.safeParse({
      display_name: "Test",
      credit_limit_cents: "5000",
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.credit_limit_cents).toBe(5000);
    }
  });

  it("rejects negative credit_limit_cents", () => {
    const result = customerFormSchema.safeParse({
      display_name: "Test",
      credit_limit_cents: -100,
    });
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0].message).toBe("Must be 0 or greater");
    }
  });

  it("accepts zero credit_limit_cents", () => {
    const result = customerFormSchema.safeParse({
      display_name: "Test",
      credit_limit_cents: 0,
    });
    expect(result.success).toBe(true);
  });

  it("rejects non-integer credit_limit_cents", () => {
    const result = customerFormSchema.safeParse({
      display_name: "Test",
      credit_limit_cents: 50.5,
    });
    expect(result.success).toBe(false);
  });

  it("returns full data shape on valid complete input", () => {
    const input = {
      display_name: "Full Test",
      company_name: "Full LLC",
      email: "full@test.com",
      phone: "555-1234",
      address_line1: "1 St",
      address_line2: "Apt 2",
      city: "Town",
      state: "CA",
      postal_code: "90210",
      country: "US",
      notes: "Notes",
      payment_terms: "net_30",
      tax_exempt: false,
      credit_limit_cents: 10000,
    };
    const result = customerFormSchema.safeParse(input);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data).toEqual(input);
    }
  });
});

describe("customerListParamsSchema", () => {
  it("provides defaults for empty input", () => {
    const result = customerListParamsSchema.safeParse({});
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data).toEqual({
        search: "",
        page: 1,
        per_page: 25,
        include_deleted: false,
      });
    }
  });

  it("parses search string", () => {
    const result = customerListParamsSchema.safeParse({ search: "acme" });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.search).toBe("acme");
    }
  });

  it("coerces page from string to number", () => {
    const result = customerListParamsSchema.safeParse({ page: "3" });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.page).toBe(3);
    }
  });

  it("rejects page less than 1", () => {
    const result = customerListParamsSchema.safeParse({ page: 0 });
    expect(result.success).toBe(false);
  });

  it("coerces per_page from string to number", () => {
    const result = customerListParamsSchema.safeParse({ per_page: "50" });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.per_page).toBe(50);
    }
  });

  it("rejects per_page greater than 100", () => {
    const result = customerListParamsSchema.safeParse({ per_page: 101 });
    expect(result.success).toBe(false);
  });

  it("rejects per_page less than 1", () => {
    const result = customerListParamsSchema.safeParse({ per_page: 0 });
    expect(result.success).toBe(false);
  });

  it("accepts include_deleted as true", () => {
    const result = customerListParamsSchema.safeParse({ include_deleted: true });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.include_deleted).toBe(true);
    }
  });
});
