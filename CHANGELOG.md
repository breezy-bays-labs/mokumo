# Changelog

All notable changes to Mokumo will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Added

- shadcn-svelte components: hover-card, carousel, drawer, menubar, calendar with Storybook stories (#247)
- Custom components: status-dot, spinner, split-button, choicebox, error-message, description, theme-switcher with Storybook stories (#247)
- Composite blocks: login-with-image, date-picker-input, sidebar-icon-only with Storybook stories (#247)
- Project banner component with success/warning/error/info variants — general-purpose notification bar (#247)
- Badge status variants: success, warning, error, info — uses design system status color tokens (#247)
- Mokumo ink cloud logo in sidebar header — wordmark when expanded, cloud icon when collapsed (#233)
- Favicon updated to Mokumo ink cloud
- Tauri desktop app icon replaced with ink cloud on primary blue background (#233)
- `mokumo reset-db` CLI command to delete database and start fresh (#166)
- Demo seed pipeline: `moon run web:seed-demo` produces a pre-seeded `demo.db` with 25 customers
- Enhanced customer fixture factory with weighted templates (full/standard/minimal) and hero customers
- Customer restore/unarchive: `PATCH /api/customers/{id}/restore` endpoint and UI Restore button on archived customer detail page
- Reusable `CopyableUrl` component with secure-context-aware clipboard error messages
- "Connect Your Team" card on dashboard showing LAN URL for multi-device access
- LAN URL display on setup wizard completion screen
- Post-recovery nudge toast after password reset via recovery code, with deep-link to regenerate codes
- Slidev demo guide infrastructure (`docs/demo-guide/`) for interactive milestone walkthroughs
- GitHub Pages deployment workflow for demo guide
- Help icon with demo guide link in sidebar footer (popover with external link)
- M0 demo walkthrough slides: 10 sections + 2 appendixes covering installation through LAN multi-client
- `Checklist.vue` Slidev component for step-by-step checklists in slides
- Getting Started section in README with download link and platform notes
- Dual-directory data layout: `data_dir/{demo,production}/mokumo.db` with separate `sessions.db` at root
- `SetupStatusResponse` typed API response with `setup_mode` field
- Automatic flat-to-dual migration for existing installations
- Demo sidecar auto-copy on first launch (copies bundled `demo.db` to data directory)
- Demo auto-login middleware: unauthenticated requests in demo mode automatically log in as demo admin
- `POST /api/demo/reset` endpoint to reset demo database to original sidecar state with graceful server restart
- `DemoResetResponse` typed API response
- Non-active profile database migrations at startup (both demo and production DBs stay up-to-date)
- Tauri sidecar bundling for demo database
- BDD test coverage for demo startup, demo authentication, and demo reset scenarios
- Demo mode banner in app shell: "You're exploring demo data" with link to Settings and dismiss button
- Demo mode section on System Settings page with "Reset Demo Data" button and confirmation dialog
- CI demo-smoke job: validates seed pipeline produces a valid demo database
- Tauri cross-platform release workflow (macOS ARM/Intel + Windows) triggered on `v*` tag push
- NSIS installer uses `currentUser` mode (no admin required on Windows)
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

- Auth and demo handlers now use `AppError` for consistent error responses with `Cache-Control: no-store` and structured `ErrorBody` (#248)
- Dashboard LAN URL now shows real server info from `/api/server-info` instead of `window.location.origin`
- Setup wizard hides token field when pre-filled via URL parameter, reveals on error
- Activity log entries now record the authenticated user's ID and type instead of hardcoded "system" for customer mutations
- Session store now uses a separate `sessions.db` database independent of the active profile; upgrading requires re-login
- Default bind address to `0.0.0.0` (all interfaces) for both desktop and CLI — enables LAN access and mDNS registration by default (use `--host 127.0.0.1` for local-only)

### Fixed

- Demo guide slides now show real screenshots instead of invisible HTML comment placeholders (#228)
- Demo reset shutdown delay increased from 100ms to 500ms to prevent truncated responses (#218)
- `demo-smoke` CI job now gates PR merge via verdict check (was `continue-on-error`, now enforced) (#218)
- Demo Guide button now opens in the system browser when running inside Tauri desktop app (#227)
- `waitForServer` no longer swallows unexpected errors (malformed URL, DNS failure) — only retries connection-refused and abort-timeout (#221)
- Timeout error message now includes the last error seen for easier debugging
- Logout button now calls `POST /api/auth/logout` to destroy the server session before redirecting to login, with error toast on failure (#230)
- Customer list now refreshes automatically after creating, editing, or archiving a customer (#231)
- Archiving all customers no longer strands users on the empty state — "Show archived" toggle is visible when archived customers exist (#229)
- `reset-db` no longer fails when the recovery directory (e.g. `~/Desktop`) is unreadable due to macOS permissions — the scan is skipped with a warning instead of aborting the entire reset (#226)
- Phone number and address fields now validate format on both frontend and backend — phone rejects non-phone characters, address rejects purely special-character strings (#232)
- Recovery code redemption now retries on any SQLite contention error (not just the update step), preventing "database is locked" failures under concurrent access (#207)
- TOCTOU port race in E2E test infrastructure: `startAxumServer` now parses the actual bound port from Axum's log output instead of assuming the probed port was claimed successfully (#210)
- Customer mutations (create, update, soft-delete) now execute within atomic transactions — the mutation and its activity log entry either both persist or neither does, preventing orphaned records
- `ErrorBody.details` now always serializes as `null` when absent (not omitted from JSON)
- Settings LAN port E2E test no longer flaky in CI — replaced manual element iteration with auto-retrying Playwright assertion
