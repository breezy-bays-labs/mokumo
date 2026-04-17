import type { ErrorBody } from "$lib/types/kikan/ErrorBody";
import type { ErrorCode } from "$lib/types/kikan/ErrorCode";
import { toast } from "$lib/components/toast";

/**
 * Error codes whose server-provided message is safe to surface verbatim.
 * Security-sensitive codes (unauthorized, internal_error, etc.) fall through
 * to the caller-supplied fallback to avoid leaking implementation details.
 *
 * Typed as Set<ErrorCode> so the compiler catches invalid entries (typos are
 * a compile error). New error codes default to the caller-supplied fallback —
 * intentional: unknown codes are treated as security-sensitive until
 * explicitly allow-listed here.
 */
const USER_VISIBLE_CODES: Set<ErrorCode> = new Set([
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
]);

/**
 * Show an error toast for an API error response.
 *
 * Well-known user-facing error codes display the server's `message` verbatim.
 * All other codes (internal_error, unauthorized, parse_error, etc.) show
 * the caller-supplied fallback to avoid leaking implementation details.
 */
export function toastApiError(error: ErrorBody | null | undefined, fallback: string): void {
  if (!error) {
    toast.error(fallback);
    return;
  }
  const message = USER_VISIBLE_CODES.has(error.code) ? error.message : fallback;
  toast.error(message);
}
