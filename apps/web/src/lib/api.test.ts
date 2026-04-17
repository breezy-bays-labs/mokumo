import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { apiFetch } from "./api";
import type { ErrorBody } from "./types/kikan/ErrorBody";

describe("apiFetch", () => {
  const originalFetch = globalThis.fetch;

  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  describe("success responses", () => {
    it("returns ok:true with parsed data on 200", async () => {
      const data = { id: 1, name: "Test" };
      vi.mocked(fetch).mockResolvedValue(
        new Response(JSON.stringify(data), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
      );

      const result = await apiFetch<{ id: number; name: string }>("/api/test");

      expect(result).toEqual({ ok: true, status: 200, data });
    });

    it("returns ok:true for 201 created", async () => {
      const data = { id: 42 };
      vi.mocked(fetch).mockResolvedValue(
        new Response(JSON.stringify(data), {
          status: 201,
          headers: { "Content-Type": "application/json" },
        }),
      );

      const result = await apiFetch<{ id: number }>("/api/items");

      expect(result).toEqual({ ok: true, status: 201, data: { id: 42 } });
    });

    it("passes through request options", async () => {
      vi.mocked(fetch).mockResolvedValue(
        new Response(JSON.stringify({}), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
      );

      await apiFetch("/api/test", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: "test" }),
      });

      expect(fetch).toHaveBeenCalledWith("/api/test", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: "test" }),
      });
    });
  });

  describe("error responses", () => {
    it("returns ok:false with ErrorBody on 404", async () => {
      const error: ErrorBody = {
        code: "not_found",
        message: "Customer not found",
        details: null,
      };
      vi.mocked(fetch).mockResolvedValue(
        new Response(JSON.stringify(error), {
          status: 404,
          headers: { "Content-Type": "application/json" },
        }),
      );

      const result = await apiFetch("/api/customers/999");

      expect(result).toEqual({ ok: false, status: 404, error });
    });

    it("returns ok:false with validation details on 422", async () => {
      const error: ErrorBody = {
        code: "validation_error",
        message: "Validation failed",
        details: { email: ["must be a valid email"] },
      };
      vi.mocked(fetch).mockResolvedValue(
        new Response(JSON.stringify(error), {
          status: 422,
          headers: { "Content-Type": "application/json" },
        }),
      );

      const result = await apiFetch("/api/customers");

      expect(result).toEqual({ ok: false, status: 422, error });
    });

    it("returns ok:false with ErrorBody on 500", async () => {
      const error: ErrorBody = {
        code: "internal_error",
        message: "An internal error occurred",
        details: null,
      };
      vi.mocked(fetch).mockResolvedValue(
        new Response(JSON.stringify(error), {
          status: 500,
          headers: { "Content-Type": "application/json" },
        }),
      );

      const result = await apiFetch("/api/health");

      expect(result).toEqual({ ok: false, status: 500, error });
    });
  });

  describe("network errors", () => {
    it("returns synthetic ErrorBody when fetch throws", async () => {
      vi.mocked(fetch).mockRejectedValue(new TypeError("Failed to fetch"));

      const result = await apiFetch("/api/test");

      expect(result).toEqual({
        ok: false,
        status: 0,
        error: {
          code: "network_error",
          message: "Failed to fetch",
          details: null,
        },
      });
    });

    it("handles non-Error thrown values", async () => {
      vi.mocked(fetch).mockRejectedValue("unexpected string error");

      const result = await apiFetch("/api/test");

      expect(result).toEqual({
        ok: false,
        status: 0,
        error: {
          code: "network_error",
          message: "Network request failed",
          details: null,
        },
      });
    });
  });

  describe("204 No Content", () => {
    it("returns ok:true with no data property on 204", async () => {
      vi.mocked(fetch).mockResolvedValue(new Response(null, { status: 204 }));

      const result = await apiFetch("/api/items/1");

      expect(result).toEqual({ ok: true, status: 204 });
      expect("data" in result).toBe(false);
    });
  });

  describe("success response with non-JSON body", () => {
    it("returns parse_error when success response is not JSON", async () => {
      vi.mocked(fetch).mockResolvedValue(
        new Response("OK", {
          status: 200,
          headers: { "Content-Type": "text/plain" },
        }),
      );

      const result = await apiFetch("/api/test");

      expect(result.ok).toBe(false);
      if (!result.ok) {
        expect(result.status).toBe(200);
        expect(result.error.code).toBe("parse_error");
        expect(result.error.message).toContain("Content-Type: text/plain");
      }
    });
  });

  describe("non-JSON error responses", () => {
    it("returns parse_error when error response is not JSON", async () => {
      vi.mocked(fetch).mockResolvedValue(
        new Response("<html>502 Bad Gateway</html>", {
          status: 502,
          headers: { "Content-Type": "text/html" },
        }),
      );

      const result = await apiFetch("/api/test");

      expect(result.ok).toBe(false);
      if (!result.ok) {
        expect(result.status).toBe(502);
        expect(result.error.code).toBe("parse_error");
        expect(result.error.message).toContain("Content-Type: text/html");
      }
    });

    it("preserves status code when error body is empty", async () => {
      vi.mocked(fetch).mockResolvedValue(new Response("", { status: 503 }));

      const result = await apiFetch("/api/test");

      expect(result.ok).toBe(false);
      if (!result.ok) {
        expect(result.status).toBe(503);
        expect(result.error.code).toBe("parse_error");
        expect(result.error.message).toContain("Content-Type:");
      }
    });
  });
});
