# Changelog

All notable changes to Mokumo will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Added

- **Version CLI**: `mokumo --version` prints the version string; `mokumo version` prints extended build info including git hash, build date, target platform, and Rust version. (#405)
- **HTTP security headers**: every response now includes `Content-Security-Policy`, `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `X-XSS-Protection: 0`, and `Referrer-Policy: strict-origin-when-cross-origin`. `Strict-Transport-Security` is set conditionally when behind Cloudflare Tunnel. (#380)
- **Branded error page** shows the Mokumo logo, status code, and human-readable message for 400/401/403/404/5xx errors with navigation back to the dashboard.
- **Routing contract tests** verify unknown `/api/*` paths return JSON 404 and wrong HTTP methods return JSON 405 instead of silently serving SPA HTML. (#384)
- **Method-not-allowed fallback** returns structured JSON 405 responses for wrong HTTP methods on all API endpoints. (#384)

### Fixed

- **bdd-lint exit code** now fails when dead specs exceed a configurable threshold (`--max-dead-specs`), enabling it to function as a blocking CI gate. Previously always exited 0 regardless of findings. (#385)

### Changed

- **`no-explicit-any` lint rule** promoted from `warn` to `error` so type holes fail CI instead of silently accumulating. (#386)
- **CI quality gates** `bdd-lint` and `test-storybook` promoted from advisory to blocking — failures now prevent PR merge. `mutation-ts` remains advisory pending baseline stabilization. (#385)

- **Settings Shop page** LAN URL and IP address display now use the shared `CopyableUrl` component, removing duplicated inline copy logic. (#162)
- **Dashboard heading** now shows the configured shop name (falls back to "Your Shop" if none set). (#331)
- **Dashboard Getting Started card** now differentiates demo vs. production mode: demo users see a contextual CTA to explore sample data or switch to their production shop. (#331)
- **Shop Settings** subtitle updated to "Your shop details and network access." and now includes a read-only Shop Name card showing the mDNS slug. (#331)
- **System Settings** replaces the placeholder EmptyState with a plain h1 + subtitle: "Demo mode and profile switching." (#331)
- **Session invalidation on deploy**: `AuthUser::Id` changed from `i64` to `ProfileUserId(SetupMode, i64)` (compound user ID for dual-DB routing). Any sessions created before this deploy are invalidated on first request. Pre-release with no active users — one-time logout only. (#276)

### Fixed

- **Lock poisoning crash loop**: replaced `std::sync::RwLock` with `parking_lot::RwLock` in server state to prevent cascading panics if a write-side panic poisons the lock. (#374)
- Enter key now submits the regenerate recovery codes dialog; previously required a mouse click on the Regenerate button. (#278)
- `GET /api/setup-status` now returns `setup_mode: null` on a fresh install (when `setup_complete` is false) instead of always returning the current profile mode. (#348)
- `GET /api/setup-status` no longer aliases `production_setup_complete` from the active profile's setup state; it now queries the production database directly so the field is accurate when the demo profile is active. (#290)
- `GET /api/setup-status` now correctly returns `is_first_launch: false` after the setup wizard completes, even when no profile switch occurred first (e.g. scripted onboarding or direct API use). (#291)
- `GET /api/setup-status` now returns `setup_mode: null` on a fresh production install before the setup wizard has run, instead of incorrectly returning `"production"`. (#346)
- `reset-db` CLI now targets the correct profile database (`demo/mokumo.db` by default); use `--production` flag to reset the production profile with a stronger confirmation prompt (#258)

### Added

- **System Settings** now shows a "Production Mode — Active" indicator when running outside demo mode, symmetric to the existing demo-mode section. (#306)
- **Database identity guard**: Mokumo now rejects SQLite files that are not Mokumo databases at startup. A non-zero `PRAGMA application_id` that doesn't match Mokumo's registered value (`0x4D4B4D4F`) produces a clear error: "The database at {path} is not a Mokumo database. Check your --data-dir setting." (#308)
- **Schema compatibility guard**: Startup now detects when the database was created by a newer version of Mokumo (downgrade scenario). Demo databases are silently recreated from the bundled sidecar; production databases abort with an actionable message directing users to upgrade or restore from backup. (#309)
- **Human-readable migration error messages**: Migration failures now include the database path and a user-friendly message. Technical `DbErr` internals go to logs only. (#308)
- **PRAGMA `application_id` stamp**: New migration `m20260404_000000_set_pragmas` stamps all databases with `0x4D4B4D4F` ("MKMO"), making Mokumo databases identifiable by any SQLite browser tool.
- **PRAGMA `user_version` stamps**: Each migration stamps the schema version (1–7) for diagnostic visibility. The value is logged at startup and visible in SQLite browser tools.
- **Native error dialog on startup failure**: The Tauri desktop app now shows a native OS error dialog (NSAlert on macOS, MessageBox on Windows) when the server fails to initialize, before the webview opens.
- **Typed `server-error` Tauri event**: Restart-loop startup failures now emit a `ServerStartupError` event to the frontend webview for future recovery UI handling.

- Profile switching: `POST /api/profile/switch` endpoint switches the active profile between demo and production without a server restart. Rate-limited to 3 switches per 15 minutes. (#262)
- `GET /api/setup-status` now returns `is_first_launch`, `production_setup_complete`, and `shop_name` fields to support the welcome screen and profile switcher UX. (#262)
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

- Forgot password step 2 now tells Desktop users where to find the recovery file; Desktop app auto-opens the file on submission (#260)
- Forgot password now shows an error when the email address is not found, instead of silently advancing to step 2 (#260)
- Demo guide slides now show real screenshots instead of invisible HTML comment placeholders (#228)
- Demo reset shutdown delay increased from 100ms to 500ms to prevent truncated responses (#218)
- `demo-smoke` CI job now gates PR merge via verdict check (was `continue-on-error`, now enforced) (#218)
- Demo Guide button now opens in the system browser when running inside Tauri desktop app (#227)
- Print button on recovery codes screen now opens the system print dialog inside Tauri desktop app (#259)
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
