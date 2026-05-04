# Kikan Platform Language

> Glossary of platform vocabulary used inside `crates/kikan/` and its sibling adapter / SubGraft crates. **This file travels with `crates/kikan/` post-beta extraction (Mokumo M16)** — it should remain accurate for any future kikan consumer, not just Mokumo. Vertical-domain language (customer, garment, quote, …) lives in [`/LANGUAGE.md`](../../LANGUAGE.md), not here.
>
> Pair with [`ARCHITECTURE.md`](../../ARCHITECTURE.md) for structural detail and [`/CONTEXT.md`](../../CONTEXT.md) for navigation across the doc set.

## How to read this glossary

- Terms are grouped by surface area (composition seam, tenancy, control / data plane, …) and alphabetical within each group.
- Each entry names the canonical Rust symbol or wire-type (when there is one) and its module path.
- Boundary terms — vocabulary the kikan glossary shares with the consuming vertical — get their own [Boundary terms](#boundary-terms) section at the end with explicit cross-references to the vertical glossary.

---

## 1. Composition seam

How the engine and the application meet at compile time.

### Application

The vertical that grafts onto the engine — Mokumo today, a future second consumer post-beta. The Application supplies the `Graft` impl. Kikan never names a specific Application by identifier (invariant I1).

### BootConfig

`kikan::BootConfig` — engine boot-time configuration. Carries the `Graft`, registered `SubGraft` instances, and `DataPlaneConfig`. Constructed by the binary; consumed by `Engine::boot`.

### Engine

`kikan::Engine<G: Graft>` — the platform abstraction. Composes platform tables, vertical tables, SubGraft contributions, the data-plane router, the control-plane handlers, and the activity log into one runtime. `Engine::boot` runs the per-profile migration DAG, opens pools, validates the boot state, and returns a ready engine.

### EngineContext

`kikan::EngineContext` — handle returned from `Engine::boot`. Exposes the active `PlatformState`, the composed `axum::Router`, and shutdown hooks. The binary owns it for the process lifetime.

### Graft

`kikan::Graft` — the application's contract with the engine. Trait with associated types for app-chosen state (`AppState`) and tenant-kind enum (`ProfileKind`), plus methods for the per-profile DB filename, recovery directory, vertical migrations, and data-plane routes. Dependency inversion: kikan depends on the abstraction; the application supplies the concrete values.

### PlatformState

`kikan::PlatformState` — runtime state held by the engine: pools, mDNS status, sessions store, control-plane state. Accessible through Axum extractors.

### SelfGraft

`kikan::SelfGraft` — kikan's own contribution to the per-profile migration DAG. Distinct from `Graft` in that it carries only platform-owned tables (`users`, `activity_log`, `profile_active_extensions`, `schema_version`).

### SubGraft

`kikan::SubGraft` — opt-in platform satellite. Each is its own crate (`kikan-events`, `kikan-mail`, `kikan-scheduler`) and contributes some mix of migrations, runtime state, and routes. Apps register the ones they need at `BootConfig` construction time. Builder-time activation, not Cargo-feature activation, so the same binary can run different SubGraft sets at runtime.

---

## 2. Tenancy and profiles

Multi-tenant SQLite: one platform DB plus N per-profile DBs.

### Active DB

The per-profile database currently bound to a request — `mokumo.db` for Mokumo, whatever the consuming vertical's `Graft::db_filename()` returns for a future consumer. The engine resolves it through `Tenancy` and serves it via the `ProfileDb` extractor. Distinct from `meta.db`, which is platform-owned and shared across profiles.

### Active Profile

`kikan::profile_db::ActiveProfile` — the profile currently bound to the request. Set by tenancy resolution before handlers run.

### Profile

`kikan::meta::Profile` — a tenant unit. Each profile has its own per-profile DB, its own backup chain, and its own migration history. Stored in `meta.db.profiles` with a `slug`, a display name, a `kind` (the application's `ProfileKind`, stringified), timestamps, and an archive flag.

### ProfileDb

`kikan::ProfileDb` — Axum extractor that yields the SeaORM connection bound to the active profile's DB. Handlers operating on per-profile data take `ProfileDb` as an argument; switching profiles is a single connection-routing change.

### ProfileDirName

`kikan::tenancy::ProfileDirName` — newtype wrapping the on-disk directory name for a profile. Derived deterministically from the slug; never user-supplied freeform text.

### ProfileId

`kikan::ProfileId<K>` — typed identifier carrying the application's `ProfileKind` `K` so downstream code can pattern-match on tenant kind without a stringly-typed lookup.

### ProfileKind

The application-defined enum of tenant kinds. Mokumo's is `kikan_types::SetupMode` (`Demo`, `Production`); a different application would supply a different enum. Kikan stores it via the `Display` + `FromStr` bounds on the associated type and never names a variant in production code (invariant I1/strict).

### ProfileRepo

`kikan::meta::ProfileRepo` — repository trait for `meta.db.profiles`. `SeaOrmProfileRepo` is the production impl.

### Slug

`kikan::Slug` — short, URL-safe profile identifier. Validated against `RESERVED_SLUGS` and `MAX_SLUG_LEN`.

### Tenancy

`kikan::Tenancy` — service that resolves an inbound request to its active profile. Reads the `Host:` header (LAN), `X-Forwarded-Host` (reverse proxy), or session state, against the configured host-allow-list.

---

## 3. Control plane and data plane

Two logical surfaces, one Axum router. See [`ARCHITECTURE.md` §4](../../ARCHITECTURE.md#4-control-plane-vs-data-plane).

### Control Plane

Admin operations: profile lifecycle, migrate / dry-run, backup / restore, diagnostics, audit, recovery. Implemented as plain `async fn` returning `Result<T, ControlPlaneError>` in `kikan::control_plane::*`. Reachable only over loopback (Tauri webview) or a Unix domain socket at mode 0600 (CLI). LAN clients are blocked by the host-allow-list middleware.

### ControlPlaneError

`kikan::ControlPlaneError` — narrow handler-level error type. Variants are mapped to `(ErrorCode, http_status)` pairs by the data-plane HTTP adapter and rendered directly by the UDS adapter; both transports yield the same wire shape, pinned by `control_plane_error_variants.feature`.

### ControlPlaneState

`kikan::ControlPlaneState` — admin-state container passed into control-plane handlers. Holds repo handles, the activity writer, and the auth backend.

### Data Plane

Business surface: customers, quotes, invoices, kanban, products, inventory, sessions, auth. Reachable from all client surfaces per the active `DeploymentMode`. Vertical routes contributed via `Graft::data_plane_routes`; platform routes (auth, health, users) contributed by kikan.

### DataPlaneConfig

`kikan::DataPlaneConfig` — runtime middleware configuration. Carries `DeploymentMode`, host patterns, rate-limit windows, and CSRF posture. The middleware stack reads from it; nothing else has to.

### DeploymentMode

`kikan::DeploymentMode` — enum: `Lan`, `Internet`, `ReverseProxy`. Selects cookie flags, CSRF gating, per-IP rate limiting, mDNS, and host-allow-list shape at boot. A fourth `TailscaleMesh` mode is post-M00 work. See [`ARCHITECTURE.md` §3](../../ARCHITECTURE.md#3-deployment-topology) for the matrix.

### HostPattern

`kikan::HostPattern` — entry in the host-allow-list. A pattern is either a literal host or a controlled wildcard; `loopback` and `{shop}.local` are common entries.

### SpaMount

`kikan::data_plane::spa::SpaMount` — handle returned when an SPA source is mounted into the data-plane router. Determines cache behavior and the JSON-404 catch-all on `/api/**`.

### SpaSource

`kikan::data_plane::spa::SpaSource` — adapter trait for SPA delivery. `kikan-spa-sveltekit` provides the SvelteKit impls; the binary picks one (embedded or on-disk) and injects via `MokumoApp::with_spa_source`. Kept out of `kikan` itself so the engine builds without `apps/web/build/` (invariant I5).

---

## 4. Auth, sessions, and recovery

### AuthenticatedUser

`kikan::auth::AuthenticatedUser<K>` — Axum extractor for a logged-in user. Generic over the application's `ProfileKind` `K`.

### Backend

`kikan::auth::Backend<K>` — `axum-login` backend. Verifies credentials, loads users, owns rate-limit interaction with login attempts.

### DEFAULT_MAX_ATTEMPTS / DEFAULT_WINDOW

`kikan::rate_limit::{DEFAULT_MAX_ATTEMPTS, DEFAULT_WINDOW}` — fallback constants used when a `RateLimiter` is constructed without explicit per-call tuning (currently 5 attempts per 15-min sliding window). Production callers (`Engine::boot` for the login limiter) typically pass explicit values; the defaults exist for tests and for any future caller that does not need a tuned policy.

### Credentials

`kikan::auth::Credentials` — login input. Email + password.

### PIN_EXPIRY

`kikan::control_plane::auth::pending_reset::PIN_EXPIRY` — wall-clock TTL on a pending password-reset record (currently 15 min). Operator must redeem the file-drop PIN within this window or restart the recovery flow. Bounds the worst-case memory footprint of `PlatformState::reset_pins` together with the per-session sweep.

### PinId

`kikan::PinId` — short, opaque identifier for a recovery PIN drop. Validated by `PinIdError`.

### Recovery Artifact

A file the operator writes into the application's `Graft::recovery_dir(profile_id)` to authorize a recovery operation. Kikan owns the watching + validation mechanism; the application owns the directory location.

### RecoveryArtifactLocation

`kikan::RecoveryArtifactLocation` — typed pointer to where on disk the artifact is expected.

### RecoveryError

`kikan::RecoveryError` — error union returned from recovery operations. Maps to a uniform 400 across all rejection modes per the recovery-token security pattern (TOCTOU-safe atomic remove+reinsert in `DashMap`).

### RecoverySessionId

The opaque identifier issued at the start of a recovery flow. It is the storage key, *not* the user's email — so an attacker cannot enumerate emails from rejected sessions.

### Sessions

`kikan::Sessions` — `tower-sessions` SQLite store wrapped in an `Arc`. Owns ephemeral session state (`session.db`); login / logout never touches the platform or vertical DBs.

### SetupTokenSource

`kikan::SetupTokenSource` — input variants for the first-run bootstrap token (PIN file, env var, etc.).

### User / UserId / RoleId

`kikan::auth::{User, UserId, RoleId}` — platform user model. Lives in `meta.db.users`, never per-profile. A user can be granted access to multiple profiles within one platform install.

### UserRepository / SeaOrmUserRepo

`kikan::auth::{UserRepository, SeaOrmUserRepo}` — repository trait + production impl. Composite operations (set-role-and-log, reset-password-and-log) run inside transactions so the activity-log row is atomic with the mutation.

### UserService

`kikan::auth::UserService<R>` — domain service over a `UserRepository`. Where role / permission logic lives.

---

## 5. Activity and audit

### Activity Log

The kikan-owned audit table. Mokumo and any future vertical write to it through the same writer trait; queries through the same repository. Lives in each per-profile DB (not `meta.db`).

### ActivityAction

`kikan_types::ActivityAction` — wire enum naming the kind of action recorded (`UserCreated`, `LoginSucceeded`, `BackupStarted`, …). Lives in `kikan-types` because both kikan and the SPA name it.

### ActivityLogEntry

`kikan::ActivityLogEntry` — row shape. Carries `actor_id` (a transport-native string tag, **not** an FK to users — `'system'` for system-initiated actions), `action`, `resource_kind`, `resource_id`, `metadata`, and a timestamp.

### ActivityLogRepository

`kikan::activity::ActivityLogRepository` — read-side trait over the activity log.

### ActivityWriter

`kikan::ActivityWriter` — adapter-side trait that adapter repos call to insert log rows *inside the same transaction* as the mutation they record. This makes logging part of the mutation contract — atomicity is guaranteed by the adapter, not the service layer.

### SqliteActivityWriter / SqliteActivityLogRepo

`kikan::SqliteActivityWriter` and `kikan::activity::SqliteActivityLogRepo` — SQLite implementations.

---

## 6. Migrations, backups, restore

### Backup

A `VACUUM INTO`-snapshot of `meta.db` and the active per-profile DB written *before* every migration batch. Filename pattern: `backups/{db}.bak-vX.Y.Z-{timestamp}.db`. See [`ARCHITECTURE.md` §7](../../ARCHITECTURE.md#7-upgrade-safety-model).

### BackupResult / BackupError

`kikan::backup::{BackupResult, BackupError}` — outcome and error union from `create_backup`.

### Boot State

`kikan::BootState` — detected state at boot: pristine, healthy, partially-migrated, sidecar-recovery-needed. Drives setup wizard routing on first run.

### KIKAN_APPLICATION_ID / MOKUMO_APPLICATION_ID

`kikan::db::KIKAN_APPLICATION_ID` and `kikan::tenancy::guards::MOKUMO_APPLICATION_ID` — frozen `0x4D4B_4D4F` (`MKMO`) value stamped into `PRAGMA application_id` on every kikan-managed SQLite file. Read by `check_application_id` before opening a pool to refuse non-kikan databases. The Rust symbol may be renamed; the on-disk byte value may not — existing installs depend on it. The two symbols share one value pending the legacy `MOKUMO_APPLICATION_ID` cleanup.

### BootStateDetectionError

Error returned when boot-state detection itself fails (corrupted `meta.db`, unreadable directory).

### Bundle / BundleManifest / BundleManifestEntry

`kikan::meta::{create_bundle, restore_bundle, BundleManifest, BundleManifestEntry}` — multi-DB backup bundle for full-install snapshots. Used by the diagnostics / support bundle and full-restore paths.

### DbInBundle

`kikan::DbInBundle<'a>` — pointer to a single DB file inside a bundle.

### GraftId

`kikan::GraftId` — opaque ownership tag on a migration. Lets the runner build a per-profile DAG that mixes platform migrations (`SelfGraft`), vertical migrations (`Graft::migrations`), and SubGraft migrations.

### Migration

`kikan::Migration` — trait every migration implements. Must return `Some(true)` from `use_transaction()` (atomic SQLite migrations). The runner acquires every connection in the pool simultaneously to make `FK=ON` apply uniformly.

### MigrationConn

`kikan::migrations::MigrationConn` — wrapper over a `DatabaseTransaction` that exposes the SeaORM `SchemaManager` to migrations.

### MigrationRef

`kikan::MigrationRef` — DAG node: `{id, target, after}` plus the migration itself.

### MigrationTarget

`kikan::MigrationTarget` — enum: `Meta` (the platform DB) vs `Profile` (per-profile DB). The runner routes each migration to its target.

### Restore / RestoreResult / RestoreError

`kikan::backup::{restore_from_backup, RestoreResult, RestoreError}` — the reverse operation. Validates the source through `validate_candidate` before writing.

### RestoreTarget

`kikan::RestoreTarget` — destination of a restore operation (a specific profile, or the meta DB).

### SidecarRecovery / SidecarRecoveryDiagnostic

`kikan::{SidecarRecovery, SidecarRecoveryDiagnostic}` — sidecar swap mechanism for demo-reset and restore flows; runs outside SQLite's locking protocol, so requires single-Engine-per-data-directory.

### UpgradeOutcome / UpgradeError

`kikan::meta::upgrade::{UpgradeOutcome, UpgradeError}` — boot-time upgrade result.

---

## 7. Eventing

`kikan-events` — SubGraft.

### BroadcastEventBus

`kikan_events::BroadcastEventBus` — Tokio broadcast wrapper. Default `EventBus` implementation.

### Event

`kikan_events::Event` — marker trait (`Clone + Send + Sync + 'static`).

### EventBus

`kikan_events::bus::EventBus` — pub/sub abstraction. Apps depend on the trait, not a specific impl.

### EventBusError

Error returned by publish operations.

### EventBusSubGraft

`kikan_events::EventBusSubGraft` — registration handle. Apps add it via `BootConfig::with_subgraft`.

### FanoutChannel

`kikan_events::FanoutChannel<T>` — generic fan-out channel used internally by the bus.

### HealthEvent / LifecycleEvent / MigrationEvent / ProfileEvent

Domain event types published by kikan: HealthResolver state transitions, lifecycle callbacks, migration progress, and profile create / archive / switch events.

---

## 8. Mail

`kikan-mail` — SubGraft.

### CapturingMailer

Test-only `Mailer` impl that captures messages into an in-memory vec. Used by integration tests.

### EmailAddress

`kikan_mail::EmailAddress` — newtype over a validated address. Constructed via `EmailAddress::parse`.

### LettreMailer

Production SMTP mailer over `lettre`.

### MailError

Error union for mail operations.

### Mailer

The trait every mailer implements. `send(message: OutgoingMail) -> Result<(), MailError>`.

### MailerSubGraft

Registration handle.

### OutgoingMail

`kikan_mail::OutgoingMail` — wire shape for a message to send (from / to / subject / body / template hints).

### SmtpConfig

`kikan_mail::SmtpConfig` — SMTP server configuration.

---

## 9. Scheduler

`kikan-scheduler` — SubGraft.

### ApalisScheduler

Production `Scheduler` impl over the `apalis` crate.

### ImmediateScheduler

Synchronous in-process scheduler. Runs jobs on submission. Useful for tests and for shop owners who don't need a background worker.

### JobId

Opaque identifier for an enqueued job.

### JobPayload

Trait every job payload implements (`Serialize + Deserialize + Send + Sync + 'static`).

### PendingJob

`kikan_scheduler::PendingJob` — wire shape for an enqueued-but-not-yet-run job.

### Scheduler

The trait. `schedule(job)`, `cancel(id)`, `pending_jobs()`.

### SchedulerError

Error union.

### SchedulerSubGraft

Registration handle.

---

## 10. SPA delivery

`kikan-spa-sveltekit` — adapter crate. Two `SpaSource` impls:

### CompositeSpaSource

`kikan::data_plane::spa::CompositeSpaSource` — composite that overlays multiple sources (e.g. a tenant-specific override on top of the default SPA build).

### SvelteKitSpa

`kikan_spa_sveltekit::SvelteKitSpa<A: RustEmbed>` — embedded-SPA source for single-binary delivery. The binary supplies a `RustEmbed` of `apps/web/build/`.

### SvelteKitSpaDir

`kikan_spa_sveltekit::SvelteKitSpaDir { dir }` — on-disk source for `mokumo-server --spa-dir <PATH>`. Boot-validates `<PATH>/index.html` then mounts.

---

## 11. Diagnostics, server info, and version

### AppDiagnostics

`kikan_types::AppDiagnostics` — top-level diagnostics envelope (system + database + runtime + os).

### KikanVersionResponse

`kikan_types::KikanVersionResponse` — wire shape for `GET /api/platform/v1/kikan-version`.

### MdnsStatus / SharedMdnsStatus

`kikan::{MdnsStatus, SharedMdnsStatus}` — mDNS advertise state (off, advertising, error).

### ServerInfoResponse

Wire shape for the public server-info endpoint (no auth required).

---

## 12. Errors

### AppError

`kikan::AppError` — wide HTTP transport error. Renders `(ErrorCode, http_status, ErrorBody)` to the client. Distinct from `ControlPlaneError` (narrow, handler-level); `From<ControlPlaneError> for AppError` bridges them on the HTTP path.

### DomainError

`kikan::DomainError` — generic over a domain-specific error. Wraps a domain error in an HTTP-transport-shaped result without leaking the domain enum to the wire.

### ErrorBody / ErrorCode

`kikan_types::{ErrorBody, ErrorCode}` — wire shape: `{"code": "...", "message": "...", "details": null}`. Hurl smoke tests assert on `$.code`; Hurl assertions on `$.error` will silently pass on a missing field, so the convention is `$.code`.

---

## Boundary terms

Vocabulary that the kikan glossary shares with [`/LANGUAGE.md`](../../LANGUAGE.md) — the seam where platform language meets vertical language. Each entry below names the kikan-side concept and points at the vertical-side counterpart.

| Term | Kikan side | Vertical side |
|---|---|---|
| **Profile** | The tenant unit. Kikan owns `Profile`, `ProfileId`, `ProfileRepo`, the meta-DB row shape, the lifecycle (create / archive / switch). See §2 above. | The vertical names its profiles via `ProfileKind`. Mokumo's is `SetupMode` with variants `Demo` / `Production`. See [`/LANGUAGE.md` § Setup and profile](../../LANGUAGE.md). |
| **ProfileKind** | An associated type on `Graft`. Kikan stores it through `Display` / `FromStr` and pattern-matches on it via `Graft` hooks (`all_profile_kinds`, `default_profile_kind`, `requires_setup_wizard`, `auth_profile_kind`). | The vertical defines the concrete enum. See [`/LANGUAGE.md` § Setup and profile](../../LANGUAGE.md). |
| **Active DB** | Per-profile DB whose filename comes from `Graft::db_filename()`. Kikan owns init, pragmas, pool, migration runner, backup. See §2 above. | The vertical chooses the filename. Mokumo returns `"mokumo.db"`. See [`/LANGUAGE.md` § Setup and profile](../../LANGUAGE.md). |
| **Migration** | Kikan owns the `Migration` trait, the per-profile migration DAG runner, transactional application, and `MigrationTarget` routing. See §6 above. | The vertical contributes its migrations through `Graft::migrations()`. SubGrafts contribute their own. See [`/LANGUAGE.md` § Migration](../../LANGUAGE.md). |
| **User** | Platform user model in `meta.db.users`. Kikan owns `User`, `UserId`, `Backend`, `UserService`, `UserRepository`, password hashing, and login rate-limiting. See §4 above. | The vertical's auth handlers route through the kikan `Backend`. Mokumo extends auth UX (recovery, reset, sidecar PIN) but never the user model. See [`/LANGUAGE.md` § Auth](../../LANGUAGE.md). |
| **Activity Log** | Kikan owns the table shape, `ActivityWriter`, `ActivityLogRepository`, and the SQLite impls. See §5 above. | The vertical emits log rows from adapter-layer mutations *inside the same transaction* as the mutation. Action variants live in `kikan_types::ActivityAction`. See [`/LANGUAGE.md` § Activity](../../LANGUAGE.md). |
| **Recovery Artifact** | Kikan owns the watching mechanism, validation, and `RecoveryArtifactLocation` typing. See §4 above. | The vertical chooses the directory via `Graft::recovery_dir(profile_id)` and the artifact format. See [`/LANGUAGE.md` § Auth](../../LANGUAGE.md). |
| **Setup Token** | Kikan owns `SetupTokenSource` and the bootstrap flow. | The vertical decides whether its profiles need a setup wizard via `Graft::requires_setup_wizard(profile_kind)`. See [`/LANGUAGE.md` § Setup](../../LANGUAGE.md). |

---

## When this glossary is wrong

If a term in this file disagrees with the code: the code wins, this file is stale. Open a PR that updates the glossary in the same diff as the code change. The Synchronized-Docs rule in [`AGENTS.md`](../../AGENTS.md#synchronized-docs) calls this out as a paired-files invariant — a `pub` symbol added under `crates/kikan-*` should arrive with its glossary entry, not "in a follow-up."
