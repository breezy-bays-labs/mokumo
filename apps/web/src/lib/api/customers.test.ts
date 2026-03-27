import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  listCustomers,
  getCustomer,
  createCustomer,
  updateCustomer,
  deleteCustomer,
  getEntityActivity,
  getCustomerActivity,
} from "./customers";

// Mock the shared apiFetch and buildQuery from the parent api module
vi.mock("$lib/api", () => ({
  apiFetch: vi.fn(),
  buildQuery: vi.fn((params: Record<string, unknown>) => {
    const entries = Object.entries(params).filter(
      ([, v]) => v !== undefined && v !== "" && v !== false,
    );
    if (entries.length === 0) return "";
    return "?" + new URLSearchParams(entries.map(([k, v]) => [k, String(v)])).toString();
  }),
}));

import { apiFetch } from "$lib/api";

const mockApiFetch = vi.mocked(apiFetch);

describe("customers API client", () => {
  beforeEach(() => {
    mockApiFetch.mockReset();
  });

  describe("listCustomers", () => {
    it("calls /api/customers with no query when no params", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: { items: [], total: 0 } });

      await listCustomers();

      expect(mockApiFetch).toHaveBeenCalledWith("/api/customers");
    });

    it("includes search param in query string", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: { items: [], total: 0 } });

      await listCustomers({ search: "acme" });

      expect(mockApiFetch).toHaveBeenCalledWith(expect.stringContaining("search=acme"));
    });

    it("includes pagination params in query string", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: { items: [], total: 0 } });

      await listCustomers({ page: 2, per_page: 10 });

      const url = mockApiFetch.mock.calls[0][0] as string;
      expect(url).toContain("page=2");
      expect(url).toContain("per_page=10");
    });

    it("includes include_deleted when true", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: { items: [], total: 0 } });

      await listCustomers({ include_deleted: true });

      expect(mockApiFetch).toHaveBeenCalledWith(expect.stringContaining("include_deleted=true"));
    });

    it("returns the api result as-is", async () => {
      const expected = { ok: true as const, status: 200, data: { items: [], total: 0 } };
      mockApiFetch.mockResolvedValue(expected);

      const result = await listCustomers();

      expect(result).toBe(expected);
    });
  });

  describe("getCustomer", () => {
    it("calls /api/customers/:id without query by default", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: {} });

      await getCustomer("abc-123");

      expect(mockApiFetch).toHaveBeenCalledWith("/api/customers/abc-123");
    });

    it("appends include_deleted query when true", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: {} });

      await getCustomer("abc-123", true);

      expect(mockApiFetch).toHaveBeenCalledWith("/api/customers/abc-123?include_deleted=true");
    });

    it("does not append query when includeDeleted is false", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: {} });

      await getCustomer("abc-123", false);

      expect(mockApiFetch).toHaveBeenCalledWith("/api/customers/abc-123");
    });
  });

  describe("createCustomer", () => {
    it("sends POST with JSON body", async () => {
      const data = { display_name: "Test Corp" };
      mockApiFetch.mockResolvedValue({ ok: true, status: 201, data: { id: "new-id" } });

      await createCustomer(data);

      expect(mockApiFetch).toHaveBeenCalledWith("/api/customers", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data),
      });
    });
  });

  describe("updateCustomer", () => {
    it("sends PUT with JSON body to correct URL", async () => {
      const data = { display_name: "Updated Corp" };
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: { id: "abc-123" } });

      await updateCustomer("abc-123", data);

      expect(mockApiFetch).toHaveBeenCalledWith("/api/customers/abc-123", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data),
      });
    });
  });

  describe("deleteCustomer", () => {
    it("sends DELETE to correct URL", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: { id: "abc-123" } });

      await deleteCustomer("abc-123");

      expect(mockApiFetch).toHaveBeenCalledWith("/api/customers/abc-123", {
        method: "DELETE",
      });
    });
  });

  describe("getEntityActivity", () => {
    it("builds query with entity_type and entity_id", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: { items: [], total: 0 } });

      await getEntityActivity("customer", "abc-123");

      const url = mockApiFetch.mock.calls[0][0] as string;
      expect(url).toContain("/api/activity");
      expect(url).toContain("entity_type=customer");
      expect(url).toContain("entity_id=abc-123");
    });

    it("includes pagination params", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: { items: [], total: 0 } });

      await getEntityActivity("customer", "abc-123", { page: 3, per_page: 5 });

      const url = mockApiFetch.mock.calls[0][0] as string;
      expect(url).toContain("page=3");
      expect(url).toContain("per_page=5");
    });
  });

  describe("getCustomerActivity", () => {
    it("delegates to getEntityActivity with entity_type customer", async () => {
      mockApiFetch.mockResolvedValue({ ok: true, status: 200, data: { items: [], total: 0 } });

      await getCustomerActivity("abc-123", { page: 1 });

      const url = mockApiFetch.mock.calls[0][0] as string;
      expect(url).toContain("entity_type=customer");
      expect(url).toContain("entity_id=abc-123");
      expect(url).toContain("page=1");
    });
  });
});
