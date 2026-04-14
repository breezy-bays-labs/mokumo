import { describe, it, expect, vi, beforeEach } from "vitest";

const { mockError } = vi.hoisted(() => ({ mockError: vi.fn() }));
vi.mock("$lib/components/toast", () => ({ toast: { error: mockError } }));

import { toastApiError } from "./error-toast";
import type { ErrorBody } from "$lib/types/ErrorBody";
import type { ErrorCode } from "$lib/types/ErrorCode";

describe("toastApiError", () => {
  beforeEach(() => mockError.mockClear());

  // ── Null / undefined guard ────────────────────────────────────────────────

  it("uses fallback when error is null (network failure, no structured response)", () => {
    toastApiError(null, "Something went wrong.");
    expect(mockError).toHaveBeenCalledWith("Something went wrong.");
  });

  it("uses fallback when error is undefined", () => {
    toastApiError(undefined, "Something went wrong.");
    expect(mockError).toHaveBeenCalledWith("Something went wrong.");
  });

  // ── Allow-list: every code that should surface the server message ─────────
  //
  // Each entry is tested individually so that removing any one code from
  // USER_VISIBLE_CODES is caught. The TypeScript compiler validates that the
  // codes are valid ErrorCode variants, but it does NOT enforce that a new
  // backend code is added to the allow-list — only these tests do.

  const ALLOW_LISTED_CODES: ErrorCode[] = [
    "rate_limited",
    "invalid_credentials",
    "not_found",
    "conflict",
    "validation_error",
    "method_not_allowed",
    "setup_failed",
    "missing_field",
    "production_db_exists",
    "not_mokumo_database",
    "database_corrupt",
    "schema_incompatible",
    "restore_in_progress",
    "shop_logo_requires_production_profile",
    "logo_format_unsupported",
    "logo_too_large",
    "logo_dimensions_exceeded",
    "logo_malformed",
    "shop_logo_not_found",
  ];

  it.each(ALLOW_LISTED_CODES)("surfaces server message for allow-listed code: %s", (code) => {
    const error: ErrorBody = {
      code,
      message: `Server message for ${code}`,
      details: null,
    };
    toastApiError(error, "Fallback");
    expect(mockError).toHaveBeenCalledWith(`Server message for ${code}`);
  });

  // ── Fallback: security-sensitive codes must never leak server details ──────

  it("uses fallback for a security-sensitive code (internal_error)", () => {
    const error: ErrorBody = {
      code: "internal_error",
      message: "panic at src/handlers/profile.rs:42 — index out of bounds",
      details: null,
    };
    toastApiError(error, "Something went wrong. Please try again.");
    expect(mockError).toHaveBeenCalledWith("Something went wrong. Please try again.");
  });

  it("uses fallback for unauthorized (avoids leaking auth details)", () => {
    const error: ErrorBody = {
      code: "unauthorized",
      message: "JWT expired at 2026-04-14T00:00:00Z",
      details: null,
    };
    toastApiError(error, "Session expired. Please log in again.");
    expect(mockError).toHaveBeenCalledWith("Session expired. Please log in again.");
  });
});
