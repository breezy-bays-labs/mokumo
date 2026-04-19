# Changelog

All notable changes to Mokumo will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Performance

- **Composite index on `activity_log(created_at DESC, id DESC)`**: Eliminates filesort on paginated activity-log reads. `created_at` is stored at second precision, so batch inserts within the same second produce identical timestamps; the composite index gives the `ORDER BY created_at DESC, id DESC` tie-breaker an O(log n) path. (#567)

### Security

- **Login timing side-channel closed**: `verify_credentials` now runs argon2 against a reference hash on the unknown-email and inactive-account paths, so response time no longer reveals whether an email is registered. (#508)
- **Inactive accounts can no longer regenerate recovery codes**: `regenerate_recovery_codes` rejects inactive accounts before password verification. (#508)

### Fixed

- **Diagnostics surfaces real I/O errors**: `read_profile_diagnostics` now propagates `tokio::fs::metadata` errors instead of swallowing them; only `NotFound` continues to map to absent file size. (#508)
- **Diagnostics bundle no longer blocks the async executor or grows unbounded**: `build_bundle` and `collect_system_diagnostics` now run on the blocking thread pool. Bundle log inclusion is capped at `MAX_LOG_FILES=32` and `MAX_LOG_TOTAL_BYTES=64 MiB` (raw-byte cap enforced at read-time so a single huge line cannot exhaust memory); when the cap trips, a `logs/TRUNCATED` marker entry is written so the operator knows data was dropped. (#508)

### Added

- **Admin CLI profile/migrate/backup subcommands** (#511): `mokumo-server profile list|switch`, `mokumo-server migrate status`, and `mokumo-server backup list|create` subcommands with human-readable and `--json` output. All read operations use hybrid UDS-first/direct-DB-fallback pattern. New admin UDS endpoints: `GET /profiles`, `POST /profiles/switch`, `GET /migrate/status`, `GET /backups`, `POST /backups/create`. Structured CLI exit codes (10=daemon not running, 11=connection refused, 12=HTTP error, 13=request failed).
- **`mokumo-server` headless binary** (#510): Real clap dispatch with four subcommands — `serve` (default, starts Axum data plane on TCP + admin surface on Unix domain socket), `diagnose` (structured diagnostics with `--json`, works with or without a running daemon), `bootstrap` (create first admin account from `--email` + `--password-file`), and `backup` (create database backup). `serve --mode lan|loopback` controls bind address. `MOKUMO_DATA_DIR` env override respected. I3 invariant (zero Tauri deps) CI-enforced.
- **Unix domain socket admin surface** (#508, #510): `kikan-socket::serve_unix_socket()` serves an Axum router over a UDS at `{data_dir}/admin.sock` with mode 0600 (filesystem-permission auth). `mokumo-api::admin_uds::build_admin_uds_router()` provides `GET /health`, `GET /diagnostics`, `GET /diagnostics/bundle` without session middleware. Integration tests exercise the control plane handlers via the UDS path.
- **`kikan-cli` admin CLI library** (#510): UDS HTTP client (`UdsClient`) + `diagnose::run()` dispatch for the `mokumo-server diagnose` subcommand when the daemon is running.

### Internal

- **I1 purity fix — `shop_name` lifted out of kikan into the vertical adapter (PR-C)**: `shop_name` removed from `kikan_types::auth::SetupRequest`, `SeaOrmUserRepo::create_admin_with_setup` (was writing `INSERT OR REPLACE INTO settings ('shop_name', ?)` inside a kikan repo method), `validate_setup_request` (was checking `req.shop_name.is_empty()`), and the kikan `setup` handler. The `SetupAttemptGuard` logic extracted into a new pure fn `kikan::control_plane::users::setup_admin(&ControlPlaneState, email, name, password, setup_token) -> Result<SetupAdminOutcome, ControlPlaneError>`. The kikan `setup` handler becomes a thin wrapper around `setup_admin` that adds profile-path disk write, `is_first_launch` CAS, and auto-login. `services/api/src/setup.rs` introduces a local `vertical_setup_router()` (mounted instead of `kikan::platform::auth::setup_router()`) that accepts a `VerticalSetupRequest { shop_name, … }` wire type, calls `setup_admin`, then writes `shop_name` to `shop_settings` via SQLx. Wire behavior preserved byte-for-byte; BDD + regression tests updated. I1 CI check (`scripts/check-i1-domain-purity.sh`) and purity witness (`control_plane_purity`) both pass. (#508)
- **Profile switch pure-fn lift (Wave E, PR-C)**: Introduces `kikan::control_plane::profiles::switch_profile(&PlatformState, SetupMode, email) -> Result<SwitchOutcome, ControlPlaneError>` encapsulating the three transport-neutral operations of a profile switch: (1) user lookup in the target DB (`SeaOrmUserRepo::find_by_email_with_hash`), (2) atomic disk-persist of `active_profile` (write-tmp-then-rename), and (3) in-memory flip of `PlatformState::active_profile`. `SwitchOutcome` carries the `AuthenticatedUser` for the adapter's `auth_session.login` call and `previous_profile` for rollback if session operations fail. `services/api/src/profile_switch.rs` delegates steps 4–6 to the pure fn; rate limiting, CSRF/Origin validation, email resolution from session, session logout/login, `SESSION_KEY_PRODUCTION_EMAIL` carry-over, `demo_install_ok` revalidation, and `is_first_launch` CAS remain in the adapter as transport-coupled concerns. File-header doc comment in `profile_switch.rs` documents the adapter/pure-fn split boundary explicitly. Route shape, rate-limit behaviour, CSRF rules, session semantics, and wire responses unchanged; hurl/BDD green without edits. (#508 Wave E)
- **Diagnostics pure-fn lift (Wave C, PR-B)**: Relocates the diagnostics snapshot collection (`collect`) and bundle export (`build_bundle`) from `kikan::platform::{diagnostics, diagnostics_bundle}` into `kikan::control_plane::diagnostics` with transport-neutral signatures (`&PlatformState → Result<_, ControlPlaneError>`). The HTTP handlers are now thin delegations that only add the Axum response shell (`Json` for `GET /api/diagnostics`, `Content-Type`/`Content-Disposition` headers for `GET /api/diagnostics/bundle`). The log-redaction helpers (`redact_patterns`, `scrub_line`) and their 6 unit tests moved with the pure fn. Signature takes `&PlatformState` (not `&ControlPlaneState`) because diagnostics only reads platform fields — UDS/CLI callers holding a `ControlPlaneState` pass `&state.platform`. Route shapes, zip archive contents, metadata JSON layout, and redaction patterns unchanged; hurl/BDD green without edits. The Commitment 3 DTO keys (`uds_path`, `uds_mode`, `meta_db_ok`, `profiles[]`, etc.) are deferred to PR-D where UDS and CLI callers actually consume them. (#508)
- **User/auth pure-fn lift (Wave B, PR-B)**: Introduces `kikan::control_plane::users::*` (6 pure fns: `bootstrap_first_admin`, `verify_credentials`, `verify_credentials_struct`, `soft_delete_user`, `update_user_role`, `regenerate_recovery_codes`) over a new `ControlPlaneState` slice that extends `PlatformState` with the auth rate limiters (`login_limiter`, `recovery_limiter`, `regen_limiter`, `switch_limiter`, all `Arc<RateLimiter>`), `reset_pins`, `recovery_dir`, `setup_token`, `setup_in_progress`, and `activity_writer`. `kikan::platform::{auth, users}` handlers rewired as thin delegations: session issuance, rate-limit enforcement, lockout accounting, and HTTP-specific error messages ("Admin access required", "Invalid password") stay in the adapter via handler-local mappers (`map_user_admin_error`, `map_regenerate_error`); the pure-fn layer reports semantic variants only. `PendingReset` relocated from `platform::auth` to `control_plane::state`. `MokumoAppState::control_plane_state()` accessor mirrors `platform_state()`. Expands `ConflictKind` with `LastAdminProtected { message: String }` so the last-admin guard's 409 wire shape is preserved byte-for-byte with dynamic messaging; `message()` loosened to `&str`. `control_plane_error_variants.feature` Examples table refactored to one row per `ConflictKind`. Purity witness test (`crates/kikan/tests/control_plane_purity.rs`) bans `axum::/axum_login::/tower::/tower_sessions::/tower_http::/http::/mokumo_shop::` imports under `crates/kikan/src/control_plane/**`. (#508)
- **`profile_db_middleware` consolidated into `kikan::profile_db` (Wave A.5)**: Relocates the profile-routing middleware from `services/api/src/profile_db.rs` (deleted) into `crates/kikan/src/profile_db.rs` alongside the `ProfileDb` / `ActiveProfile` extractors. The middleware is parameterized on `State<PlatformState>` — it already only touched `db_for(mode)` and `active_profile`, both present on `PlatformState` since the platform-slice lift, so the prior file-header claim that it "cannot move into platform code without dragging AppState along" was stale. Mount site in `services/api::build_app_inner` updated from `state.clone()` + `profile_db::profile_db_middleware` to `state.platform_state()` + `kikan::profile_db::profile_db_middleware`; route shapes, layer ordering, and `profile_middleware.feature` BDD contract all unchanged. (#508)
- **UserRepo composite-method atomicity (Wave A.3b)**: Adds two new composite methods on `SeaOrmUserRepo` — `create_user_with_codes(&CreateUser, codes_count)` (generic user + recovery-codes batch with activity-log-in-txn) and `bootstrap_admin_with_codes(email, name, password)` (first-admin bootstrap with an `ALREADY_BOOTSTRAPPED` guard checked *inside* the transaction, so concurrent bootstraps race-safely). The third method named in the feature, `regenerate_codes_with_log`, is satisfied by the existing `regenerate_recovery_codes`. Introduces `kikan::auth::repo::BootstrapError` (`AlreadyBootstrapped`, `Domain(DomainError)`) with `From<BootstrapError> for ControlPlaneError` so handlers render the wire code without string-sniffing. Adds `ActivityAction::Bootstrap` (displays as `"bootstrap"`). Un-tags `user_repo_atomicity.feature` — all 6 scenarios pass (create success, validation rollback via `codes_count` out-of-range, regenerate success, regenerate log-failure rollback via a `RAISE(ABORT)` trigger on `activity_log`, bootstrap success, bootstrap `ALREADY_BOOTSTRAPPED` rejection). Adds `crates/kikan/tests/bdd_world/user_repo_atomicity_steps.rs` and `crates/kikan/tests/user_repo_trait_signature.rs` as a compile-time witness that `UserRepository` leaks no `sea_orm::DatabaseTransaction` or `sea_orm::TransactionTrait` into its public surface. Composite-method transactional atomicity is now enforced at both the structural (trait signature) and behavioral (BDD) levels. (#508)
- **Activity HTTP handler lift into `kikan::platform::activity_http`**: Relocates `services/api/src/activity/mod.rs` (~55 LOC) to `crates/kikan/src/platform/activity_http/mod.rs`. The `GET /api/activity` list endpoint (optional `entity_type`, `entity_id`, `page`, `per_page` filters) now lives alongside the other platform handlers. Module named `activity_http` to avoid collision with `kikan::activity` (the writer/repo). `activity_router()` is generic over state (`Router<S>`) because the handler uses only per-request extractors (`ProfileDb`, `Query`). services/api mount updated to `kikan::platform::activity_http::activity_router()`; `services/api/src/activity/` deleted. Route shape and pagination semantics unchanged. (#508)
- **User admin HTTP handler lift into `kikan::platform::users`**: Relocates `services/api/src/user/mod.rs` (~90 LOC) to `crates/kikan/src/platform/users/mod.rs` so the admin-only user mutation endpoints (`DELETE /api/users/{id}` soft delete, `PATCH /api/users/{id}/role` role update) live alongside the other platform handlers. `user_admin_router()` is generic over state (`Router<S>` where `S: Clone + Send + Sync + 'static`) because both handlers rely exclusively on per-request extractors (`AuthSession`, `ProfileDb`) — no singleton dependencies required. services/api mount updated from `user::router()` to `kikan::platform::users::user_admin_router()`. Route shapes, HTTP status codes, and auth/admin-guard semantics unchanged. `services/api/src/user/` deleted. Composite-method atomicity landed separately in Wave A.3b (see entry above). (#508)
- **Auth HTTP handler lift into `kikan::platform::auth`**: Relocates `services/api/src/auth/{mod,recover,reset}.rs` (~800 LOC) to `crates/kikan/src/platform/auth/*` so the auth / setup / account-recovery endpoints live alongside the other platform handlers (diagnostics, backup-status, demo-reset). Introduces `kikan::platform::auth::AuthRouterDeps` bundling `PlatformState` + auth-specific singletons (`login_limiter`, `recovery_limiter`, `regen_limiter`, `reset_pins`, `recovery_dir`, `setup_token`, `setup_in_progress`); services/api wires via `impl From<&MokumoAppState> for AuthRouterDeps` at mount time. The `require_auth_with_demo_auto_login` middleware now takes `State<PlatformState>` directly. Route shapes, HTTP status codes, and BDD contracts are unchanged — auth / setup / recovery BDD suites green without edits. `PendingReset` and `DEMO_RESET_PATH` re-exported from `kikan::platform::auth` for backward compatibility. Converts three limiters (`login_limiter`, `recovery_limiter`, `regen_limiter`) on `MokumoAppState` from `RateLimiter` to `Arc<RateLimiter>` so they clone cheaply into `AuthRouterDeps` while preserving shared state. (#508)
- **`MokumoApp: Graft` keystone**: Promotes the former `MokumoGraftBridge` (empty `type AppState = ()`) to `MokumoApp` with `type AppState = SharedState` (`= Arc<MokumoAppState>`). Renames `services/api::AppState` → `MokumoAppState` and `services/api::graft_bridge` module → `graft`. `MokumoApp::migrations()` keeps the per-profile SeaORM bridge unchanged (`schema_equivalence` test green). `build_state` and `data_plane_routes` are intentionally deferred — `services/api::build_app_inner` still owns router composition because `Engine::build_router`'s fixed layer stack (session → trace → host allow-list) does not accommodate `axum-login`'s `AuthManagerLayerBuilder`, `ProfileDbMiddleware`, or `security_headers`. The `build_router` rewire is tracked for a follow-up wave once the layer-ordering design pass lands. (#508)
- **`ControlPlaneError` narrow handler-level error type**: Introduces `kikan::ControlPlaneError` (5 variants: `NotFound`, `Conflict(ConflictKind)`, `Validation`, `PermissionDenied`, `Internal`) with `ConflictKind::AlreadyBootstrapped` for the first-admin bootstrap path. Both transport adapters render the same `(ErrorCode, http_status)` tuple — HTTP via `From<ControlPlaneError> for AppError`, UDS via direct `IntoResponse`. Pinned by `control_plane_error_variants.feature` (un-tagged) + `crates/kikan/tests/control_plane_error_variants.rs` table-driven test. Adds `ErrorCode::AlreadyBootstrapped` variant; regenerates TypeScript bindings. (#508)

### Changed

- **Docs**: CLAUDE.md §Project Structure, §Architecture, §Coding Standards, §What NOT to Do rewritten to reflect the post-Stage-3 topology. Retires references to the dissolved `crates/core`/`crates/db` layers, the three-layer ladder, and the auto-repair litmus test per `adr-mokumo-extensions`. Adds the Engine/Application/Graft vocabulary and invariants I1–I5. Fixes `crates/kikan-tauri` Cargo description (`control_plane` → `platform`) and `crates/mokumo-shop/src/lib.rs` crate-level doc (retires `mokumo-decor` anticipation). (#508, #510)
- **CI**: enforce kikan workspace boundary (I1-I5) — `kikan-invariants` and `kikan-musl-build` jobs in `quality.yml` plant tripwires for the kikan/garment/Tauri boundary. Backed by `scripts/check-i*.sh` with self-tests under `scripts/test/`. Includes the `cargo-deny`-tauri-ban behaviour from #543. (#513, #543)

### Fixed

- **Open Existing Shop — missing behaviors**: "Open Existing Shop" click now disables all welcome-screen buttons while navigation is in flight. Navigating directly to `/welcome/restore` without a pending file redirects back to `/welcome`. The restore banner on the login page is now dismissible. "Go to Sign In" label corrected to "Retry" after restart timeout. Copy aligned to "Validating your database…" and updated credential-warning text. (#464)

### Added
- **User management endpoints**: `DELETE /api/users/:id` (soft-delete) and `PATCH /api/users/:id/role` (role change). Both endpoints require admin caller auth and include a last-admin guard: attempting to delete or demote the only active admin returns `409 Conflict`. (#365)
- **Login rate limiting and account lockout**: Brute-force protection for the login endpoint. An in-memory per-email sliding-window limiter (10 attempts / 15 min) returns 429 `rate_limited` before any argon2 work is done. A DB-backed lockout counter increments on each failed credential attempt; after 10 consecutive failures the account is locked for 15 minutes (423 `account_locked`). Successful authentication resets the counter. Adds `failed_login_attempts` and `locked_until` columns to `users` via a new migration, and exposes `record_failed_attempt`, `clear_failed_attempts`, `find_lockout_state_by_email`, and `unlock_user` on `SeaOrmUserRepo`. (#495)
- **LAN access onboarding and settings toggle**: First-run setup wizard now has an explicit LAN access consent step ("Enable LAN Access" / "Not now") between the recovery codes and completion screens, so shop owners know why the OS may prompt for network permissions. Shop settings gains a LAN Access toggle to change the preference later. The preference (`lan_access_enabled`) is persisted in `kikan_meta` and gates mDNS registration at server start — LAN discovery is off by default until the owner enables it. Exposes `GET` / `PUT /api/settings/lan-access`. (#519)
- **`kikan-events` crate**: typed event bus over `tokio::sync::broadcast` with `BroadcastEventBus`, 4 event types (Lifecycle, Health, Migration, Profile), and full BDD coverage (#517)
- **`kikan-mail` crate**: async `Mailer` trait with `LettreMailer` (SMTP via lettre/rustls) and `CapturingMailer` test adapter, `OutgoingMail` pre-wired for future outbox pattern (#515)
- **`kikan-scheduler` crate**: async `Scheduler` trait with `ApalisScheduler` (SQLite-backed job persistence) and `ImmediateScheduler` test adapter (#516)

### Performance

- **Release profile tuning**: Applied `lto = true`, `codegen-units = 1`, `strip = true`, `opt-level = 3` to `[profile.release]`. Expected 20–40% binary size reduction and improved cold-start via cross-crate inlining. `panic = "abort"` omitted — Axum runs inside the Tauri process so any abort kills the desktop window. CI release job timeout increased to 60 minutes to accommodate longer LTO compile times. (#489)

### Changed

- **`mmap_size` PRAGMA is now platform-conditional**: enabled at 256 MB on Linux (where it provides clear read-throughput gains) and disabled on Windows and macOS. On Windows, the kernel cannot truncate memory-mapped files, so enabling mmap caused `incremental_vacuum` to silently fail to shrink the database file. On macOS, the unified buffer cache already handles the I/O coalescing mmap would add, making the benefit negligible. `mokumo doctor` now reports the effective `mmap_size` for the running platform. (#457)

### Fixed

- **Kikan Stage 1b review follow-up**: pre-migration backup now progresses at ~40 MB/s instead of ~80 KB/s (page batch 5→100, inter-step delay 250 ms→10 ms) — removes the multi-minute startup stall on mature shop databases. Legacy-migration backfill now uses `OnConflict::do_nothing()` instead of string-patching the generated SQL, and flat→production database migration uses atomic `fs::rename` instead of copy+remove so a mid-move crash can no longer leave two divergent database files. (#506)
- **Customer form sheet now shows safe error messages**: unknown or security-sensitive API errors (e.g. `internal_error`) display a user-friendly fallback message instead of raw backend text; known safe codes continue to surface the server message verbatim. (#529)
- **Profile switcher now shows server error messages**: rate-limited and other known API errors display the backend's message verbatim in a toast instead of a generic fallback. Both the direct switch and the unsaved-changes confirmation path are fixed. (#469)
- **QR code on Connect Your Team card now renders correctly**: replaced `onMount` with a reactive `$effect` so the QR code re-renders when the IP URL loads asynchronously. Added null guard, loading placeholder, and error fallback state. (#470)

### Added

- **Kikan platform crate (Stage 1a)**: New `crates/kikan/` crate with migration engine primitives — DAG resolver (Kahn's topological sort), migration runner with BEGIN IMMEDIATE transactions, Graft/SubGraft extension traits, Engine shell, Tenancy struct, and bootstrap tables. Zero files outside `crates/kikan/`. (#506)
- **Kikan Stage 1b — guard chain, graft bridge, SetupMode move**: Port startup guard functions (application_id check, pre-migration backup, auto-vacuum, schema compatibility) to `kikan::tenancy`. Add `MokumoGraftBridge` wrapping 8 SeaORM migrations as kikan `Migration` impls with automatic `seaql_migrations` backfill. Move `SetupMode` from `mokumo_core` to `kikan` (serde wire format preserved). Schema equivalence test verifies byte-identical application schema. (#506)
- **Startup error dialog now shows backup location**: When a migration fails during demo reset, the error dialog surfaces the path to the pre-migration backup so shop owners know where their data is safe. (#395)
- **Host-header allowlist (DNS-rebinding defense)**: Every HTTP request whose `Host` header is not `127.0.0.1`, `localhost`, or `[::1]` (any port) is rejected with 403 before reaching any handler or auth layer. Missing or multiple `Host` headers also fail closed. Implemented as a `tower::Layer` in `crates/kikan` — kikan's first production dependency. (#518)
- **Desktop server now binds `127.0.0.1:0`** (OS-assigned ephemeral loopback port) instead of `0.0.0.0:6565`, eliminating predictable-port scanning. `window.__MOKUMO_API_BASE__` is injected via Tauri `initialization_script` before SvelteKit mounts. Tauri capability ACL updated from 11 hardcoded ports to `http://127.0.0.1:*/*` wildcard. `apiBase()` typed accessor available at `apps/web/src/lib/api/base.ts` for future fetch call sites. **Note — LAN access**: the desktop app is now loopback-only; employee devices on LAN must use a browser on the shop machine until `mokumo-server` ships (#510). (#484)
- **`mokumo migrate status`**: New CLI subcommand that shows the current schema version, lists all applied migrations with timestamps, and lists any pending (unapplied) migrations. Useful for advanced users and CI pipelines that need to verify migration state before upgrades. (#406)
- **`--verbose` / `--quiet` global CLI flags**: `-v` sets the server tracing level to `debug`, `-vv` to `trace`, and `-q` suppresses all output except errors. Accepted in any position on the command line (global Clap args); override `RUST_LOG` for the server console layer on startup. (#407)
- **Boot-time install guard**: Health endpoint reports `install_ok` flag and `status: "degraded"` when the flag is false. Protected routes return 423 `DEMO_SETUP_REQUIRED` when the demo admin account is missing, inactive, soft-deleted, or has an empty password hash. Reset demo data to restore access. (#342)
- **Storage metrics in health and diagnostics**: Health endpoint reports `storage_ok` flag (false when disk space is low or the active database needs a VACUUM). Diagnostics endpoint gains `wal_size_bytes`, `vacuum_needed` per profile, and `disk_warning` on the system object. Doctor CLI now uses the shared `diagnose_database()` helper. PRAGMA optimize runs after each migration, every 2 hours, and at graceful shutdown. (#411)
- **Shop logo upload**: `POST /api/shop/logo` accepts PNG, JPEG, or WebP (≤ 2 MB, ≤ 2048×2048 px). `GET /api/shop/logo` serves the file publicly. `DELETE /api/shop/logo` removes it. Setup status includes `logo_url` for sidebar display. Sidebar profile trigger shows the custom logo or falls back to a Store glyph. Backup and restore preserve the logo file alongside the database. (#283)
- **Support-facing health surface**: `GET /api/diagnostics` now includes system-level signals — memory usage, disk space, hostname — so support can perform first-pass triage without SSH access. Build commit SHA is included for version tracking. (#319)
- **Diagnosis bundle export**: New `GET /api/diagnostics/bundle` endpoint assembles a downloadable ZIP containing app logs (up to 7 days, sensitive values scrubbed) and a `metadata.json` runtime snapshot. The Diagnostics card on the System Settings page gains an "Export Bundle" button. (#316)
- **Offline startup validation**: BDD scenarios and Hurl smoke test confirm the server boots and serves the internal shop API with zero internet access. mDNS registration failure degrades gracefully (logged warning, `mdns_active: false` in `/api/server-info` and `/api/diagnostics`) without blocking boot. (#315)
- **Support-visible diagnostics endpoint and settings card**: new `GET /api/diagnostics` returns app version, database schema/file size/WAL mode for both profiles, runtime state (uptime, active profile, setup flags, LAN host/port, mDNS), and OS family/arch. System Settings now renders a Diagnostics card with a "Copy as Markdown" button so shop owners can share state when troubleshooting. (#318)
- **Open Existing Shop**: welcome screen now includes a third button to restore a production shop from an existing `.db` backup file. The file is validated (application ID, integrity, schema compatibility), copied to the production slot, and the server restarts. Users then log in with their existing credentials. (#282)
- **Graceful shutdown with drain timeout**: Server now exits within 10 seconds of receiving a shutdown signal, even with in-flight requests. CLI handles both SIGINT (Ctrl+C) and SIGTERM on Unix. Desktop wraps server drain with a 10-second timeout. (#312)
- **Process lock with port info**: Lock file stores the bound port so conflict messages show the URL and suggest checking the system tray. `reset-db` conflict message includes the port. (#311)
- **mDNS retry with backoff**: When LAN discovery fails at startup, automatic retries at 60s/120s/300s intervals until registration succeeds or the server shuts down. (#314)
- **WebSocket shutdown broadcast**: Connected clients receive a `server_shutting_down` event before the close frame, enabling frontend disconnect banners.
- **System tray**: Closing the window hides to tray instead of quitting. Server keeps running in the background. Left-click or "Reopen Desktop App" menu item restores the window. "Quit Mokumo" triggers clean shutdown with confirmation dialog showing connected client count. macOS dock icon hides/restores automatically. (#408)
- **Tray status and info**: Dynamic tray icon (green/yellow/red) reflects mDNS status. Tray menu shows port, IP, mDNS hostname. "Open in Browser" menu item. Tooltip shows server URL and connected client count. (#408)
- **Quit flow**: Quit from tray menu or Cmd+Q/Alt+F4 shows confirmation dialog with client count. When window is hidden, sends OS notification instead. Linux no-tray degradation: close button triggers quit confirmation. (#408)
- **Connect Your Team card**: QR code and connection URLs on the dashboard with mDNS status indicator, troubleshooting guidance, and first-run nudge for new installs.
- **WebSocket disconnect banner**: Employees see a status banner when the server disconnects, with reconnection indicator and brief "Reconnected" confirmation.
- **Unsaved changes guard**: forms now block navigation (sidebar links, back button), browser tab close, and Tauri window close when there are unsaved changes. A confirmation dialog with "Cancel" / "Leave anyway" prevents accidental data loss. Applies to the customer form, setup wizard, and any future `use:formDirty` forms. (#420)
- **Structured JSON file logging** with daily rotation and 7-day retention (`max_log_files(7)`). Log files written to `{data_dir}/logs/` as newline-delimited JSON (NDJSON) including timestamp, level, target, span context, and message fields. Console output remains human-readable text. (#412, #317)
- **Version CLI**: `mokumo --version` prints the version string; `mokumo version` prints extended build info including git hash, build date, target platform, and Rust version. (#405)
- **`mokumo backup` CLI subcommand** creates a manual database backup using the SQLite Online Backup API. Supports `--output <path>` for custom location, verifies integrity with `PRAGMA integrity_check`, and prints path + size on success. Safe to run while the server is running. (#403)
- **`mokumo restore <path>` CLI subcommand** restores the database from a backup file. Verifies backup integrity before restoring, creates a safety backup of the current database, removes WAL sidecars, and refuses to run while the server is active (process lock check). (#404)
- **SQLite `auto_vacuum = INCREMENTAL`** PRAGMA with automatic upgrade of existing databases via one-time `VACUUM`. Ensures database files shrink after row deletions. (#424)
- **SQLite `mmap_size = 268435456`** (256 MB) PRAGMA for read performance via memory-mapped I/O. (#424)
- **`mokumo doctor` CLI subcommand** with `--fix` flag for database maintenance — reports auto_vacuum status, freelist fragmentation, and reclaims free pages on demand. (#424)
- **HTTP security headers**: every response now includes `Content-Security-Policy`, `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `X-XSS-Protection: 0`, and `Referrer-Policy: strict-origin-when-cross-origin`. `Strict-Transport-Security` is set conditionally when behind Cloudflare Tunnel. (#380)
- **Branded error page** shows the Mokumo logo, status code, and human-readable message for 400/401/403/404/5xx errors with navigation back to the dashboard.
- **Routing contract tests** verify unknown `/api/*` paths return JSON 404 and wrong HTTP methods return JSON 405 instead of silently serving SPA HTML. (#384)
- **Method-not-allowed fallback** returns structured JSON 405 responses for wrong HTTP methods on all API endpoints. (#384)

### Fixed

- **WebSocket disconnect banner on server death**: the banner now reliably fires when the server process is killed (SIGKILL), not just on graceful shutdown. A liveness timer (75 s, 2.5× the heartbeat interval) force-closes and reconnects the WebSocket when the server stops responding, covering silent-death and network partition scenarios. (#471)
- **Restore flow robustness**: rollback failures during restore now propagate as 500 errors with clear messages instead of silently leaving the filesystem in an inconsistent state. Sentinel write and rollback file deletions are now fully async (`tokio::fs`). Large file restores no longer time out (backup now completes in a single step). SQLite errors during integrity and schema checks now surface as `DatabaseCorrupt` (422) rather than generic 500. (#476)
- **bdd-lint exit code** now fails when dead specs exceed a configurable threshold (`--max-dead-specs`), enabling it to function as a blocking CI gate. Previously always exited 0 regardless of findings. (#385)

### CI

- **Gitleaks secret scanning**: pre-commit hook via lefthook and CI gate in `quality.yml` block PR merges when secrets are detected. Custom rules for Mokumo-specific patterns (`MOKUMO_SECRET`, `MOKUMO_API_KEY`, Stripe keys). (#413)

### Changed

- **Port exhaustion error message** now suggests `--port` flag and closing conflicting applications instead of a generic bind error. (#313)
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
