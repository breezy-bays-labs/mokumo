import type { ErrorBody } from "./types/ErrorBody";

export type ApiResult<T> =
  | { ok: true; status: 204; data: undefined }
  | { ok: true; data: T }
  | { ok: false; status: number; error: ErrorBody };

export async function apiFetch<T>(url: string, options?: RequestInit): Promise<ApiResult<T>> {
  try {
    const response = await fetch(url, options);

    if (response.ok) {
      if (response.status === 204) {
        return { ok: true, status: 204, data: undefined };
      }
      try {
        const data: T = await response.json();
        return { ok: true, data };
      } catch {
        return {
          ok: false,
          status: response.status,
          error: {
            code: "parse_error",
            message: "Server returned a non-JSON success response",
            details: null,
          },
        };
      }
    }

    try {
      const error: ErrorBody = await response.json();
      return { ok: false, status: response.status, error };
    } catch {
      return {
        ok: false,
        status: response.status,
        error: {
          code: "parse_error",
          message: "Server returned a non-JSON error response",
          details: null,
        },
      };
    }
  } catch (err) {
    return {
      ok: false,
      status: 0,
      error: {
        code: "network_error",
        message: err instanceof Error ? err.message : "Network request failed",
        details: null,
      },
    };
  }
}
