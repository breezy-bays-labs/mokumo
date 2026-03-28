# Changelog

All notable changes to Mokumo will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Added

- Post-recovery nudge toast after password reset via recovery code, with deep-link to regenerate codes
- Slidev demo guide infrastructure (`docs/demo-guide/`) for interactive milestone walkthroughs
- GitHub Pages deployment workflow for demo guide

### Changed

- Activity log entries now record the authenticated user's ID and type instead of hardcoded "system" for customer mutations

### Added

- Recovery code regeneration from Settings > Account with password confirmation, atomic invalidation, and 10 new codes with download/print
- Low-count recovery code warning banner in app shell (shown when < 3 codes remaining, dismissable per session)
- `recovery_codes_remaining` field in `/api/auth/me` response
- `POST /api/account/recovery-codes/regenerate` endpoint with rate limiting (3/hour)
- Customer management UI: list page with search/filter/pagination, detail page with tab navigation (overview, activity, contacts, artwork, pricing, communication), create/edit form sheet, and archive flow
- Server-side customer search across display name, company name, and email
- Per-vertical frontend module pattern: API wrapper, Zod schemas, context class, tab navigation
- WebSocket broadcast infrastructure for real-time server-to-client updates
- `BroadcastEvent` wire format with version field for forward compatibility
- `ConnectionManager` with pre-serialized fan-out (`Arc<str>`) for efficient broadcasting
- TypeScript WebSocket client with automatic reconnection and exponential backoff
- Graceful shutdown with close frame 1001 (Going Away) on server stop
- Debug endpoints for connection count and broadcast testing (debug builds only)
- 9 BDD scenarios covering connection, broadcast, and shutdown behavior
- 12 Vitest tests covering client reconnection, backoff, and error handling
- Origin header validation on WebSocket endpoint to prevent cross-site hijacking
- Standardized API error responses with `ErrorBody` wire format (`code`, `message`, `details`)
- Domain error hierarchy: `DomainError` (core) -> `AppError` (api) -> `ErrorBody` (wire)
- Internal/database error redaction — sensitive details logged server-side, generic message returned to clients
- `PageParams` with clamped pagination (page >= 1, per_page 1..100, defaults 1/25)
- `PaginatedList<T>` generic pagination response type with computed `total_pages`
- `IncludeDeleted` soft-delete filter enum (excludes by default)
- `PaginationParams` Axum query extractor bridging HTTP params to domain types
- `apiFetch<T>` typed frontend fetch utility with discriminated union responses
- TypeScript bindings for `ErrorBody` and `PaginatedList<T>` via ts-rs
- JSON 404 responses for unmatched API routes (instead of serving the SPA shell)
- BDD feature files specifying error, pagination, and response convention behaviors

### Changed

- Default bind address to `0.0.0.0` (all interfaces) for both desktop and CLI — enables LAN access and mDNS registration by default (use `--host 127.0.0.1` for local-only)

### Fixed

- Customer mutations (create, update, soft-delete) now execute within atomic transactions — the mutation and its activity log entry either both persist or neither does, preventing orphaned records
- `ErrorBody.details` now always serializes as `null` when absent (not omitted from JSON)
- Settings LAN port E2E test no longer flaky in CI — replaced manual element iteration with auto-retrying Playwright assertion
