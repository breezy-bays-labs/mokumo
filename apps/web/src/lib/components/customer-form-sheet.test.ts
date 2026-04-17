// @vitest-environment jsdom

import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect, beforeEach } from "vitest";

const { mockToastApiError, mockToastError } = vi.hoisted(() => ({
  mockToastApiError: vi.fn(),
  mockToastError: vi.fn(),
}));

vi.mock("$app/environment", () => ({ browser: true, dev: false, building: false }));
vi.mock("$lib/utils/error-toast", () => ({ toastApiError: mockToastApiError }));
vi.mock("$lib/components/toast", () => ({
  toast: { success: vi.fn(), info: vi.fn(), error: mockToastError },
}));
vi.mock("$lib/api/customers", () => ({
  createCustomer: vi.fn(),
  updateCustomer: vi.fn(),
}));
vi.mock("$lib/actions/form-dirty", () => ({
  formDirty: () => ({ destroy: () => {} }),
}));

import { createCustomer, updateCustomer } from "$lib/api/customers";
import CustomerFormSheet from "./customer-form-sheet.svelte";
import type { CustomerResponse } from "$lib/types/shop/CustomerResponse";

const mockCreateCustomer = vi.mocked(createCustomer);
const mockUpdateCustomer = vi.mocked(updateCustomer);

function makeCustomer(overrides: Partial<CustomerResponse> = {}): CustomerResponse {
  return {
    id: "cust-1",
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

describe("CustomerFormSheet — applyApiErrors", () => {
  beforeEach(() => {
    mockToastApiError.mockClear();
    mockToastError.mockClear();
    mockCreateCustomer.mockClear();
    mockUpdateCustomer.mockClear();
  });

  // ── Create mode ───────────────────────────────────────────────────────────

  it("routes non-validation error through toastApiError on create (not raw toast.error)", async () => {
    const apiError = {
      code: "internal_error" as const,
      message: "panic at src/handlers/customers.rs:42 — index out of bounds",
      details: null,
    };
    mockCreateCustomer.mockResolvedValue({ ok: false, status: 500, error: apiError });

    render(CustomerFormSheet, { open: true, onClose: vi.fn() });
    const user = userEvent.setup();

    await user.type(screen.getByLabelText(/display name/i), "Acme Corp");
    await user.click(screen.getByRole("button", { name: /create/i }));

    await waitFor(() => {
      expect(mockToastApiError).toHaveBeenCalledWith(apiError, expect.any(String));
      expect(mockToastError).not.toHaveBeenCalled();
    });
  });

  it("passes a non-empty user-readable fallback to toastApiError on create", async () => {
    const apiError = {
      code: "internal_error" as const,
      message: "raw internal error",
      details: null,
    };
    mockCreateCustomer.mockResolvedValue({ ok: false, status: 500, error: apiError });

    render(CustomerFormSheet, { open: true, onClose: vi.fn() });
    const user = userEvent.setup();

    await user.type(screen.getByLabelText(/display name/i), "Acme Corp");
    await user.click(screen.getByRole("button", { name: /create/i }));

    await waitFor(() => {
      const [, fallback] = mockToastApiError.mock.calls[0] as [unknown, string];
      expect(fallback.length).toBeGreaterThan(0);
      expect(mockToastError).not.toHaveBeenCalled();
    });
  });

  // ── Edit mode ─────────────────────────────────────────────────────────────

  it("routes non-validation error through toastApiError on update (not raw toast.error)", async () => {
    const apiError = {
      code: "internal_error" as const,
      message: "raw server error",
      details: null,
    };
    mockUpdateCustomer.mockResolvedValue({ ok: false, status: 500, error: apiError });

    const customer = makeCustomer({ display_name: "Acme Corp" });
    render(CustomerFormSheet, { open: true, customer, onClose: vi.fn() });
    const user = userEvent.setup();

    const nameInput = screen.getByLabelText(/display name/i);
    await user.clear(nameInput);
    await user.type(nameInput, "Acme Corp Updated");
    await user.click(screen.getByRole("button", { name: /save changes/i }));

    await waitFor(() => {
      expect(mockToastApiError).toHaveBeenCalledWith(apiError, expect.any(String));
      expect(mockToastError).not.toHaveBeenCalled();
    });
  });

  // ── Validation error (details present) ───────────────────────────────────
  //
  // When the API returns field-level details, applyApiErrors should populate
  // inline fieldErrors — NOT call toastApiError. Verifies the if/else guard
  // isn't accidentally removed or inverted.

  it("populates field errors from details without calling toastApiError on create", async () => {
    const apiError = {
      code: "validation_error" as const,
      message: "Validation failed",
      details: { display_name: ["Display name is required"] },
    };
    mockCreateCustomer.mockResolvedValue({ ok: false, status: 422, error: apiError });

    render(CustomerFormSheet, { open: true, onClose: vi.fn() });
    const user = userEvent.setup();

    await user.type(screen.getByLabelText(/display name/i), "Acme Corp");
    await user.click(screen.getByRole("button", { name: /create/i }));

    await waitFor(() => {
      expect(mockToastApiError).not.toHaveBeenCalled();
      expect(mockToastError).not.toHaveBeenCalled();
      expect(screen.getByText("Display name is required")).toBeInTheDocument();
    });
  });
});
