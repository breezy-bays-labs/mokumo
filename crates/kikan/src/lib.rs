//! Kikan — self-hosted application platform Engine.
//!
//! Owns tenancy, per-profile migrations, auth, activity log,
//! backup/restore, control-plane handlers, SeaORM pool init,
//! data-plane middleware, and the [`Engine`] + [`Graft`] + [`SubGraft`]
//! composition seam. Depends on nothing else in the workspace
//! (invariant I4); the Application (`mokumo-shop`) and SubGrafts
//! (`kikan-events`, `kikan-mail`, `kikan-scheduler`) compose in
//! through [`Graft`] / [`SubGraft`] at compile time.
//!
//! # Data plane
//!
//! [`DataPlaneConfig`] + [`DeploymentMode`] (see [`data_plane`]) drive the
//! HTTP middleware stack. Three postures — `Lan`, `Internet`, `ReverseProxy` —
//! pick cookie flags, CSRF gating, per-IP rate limiting, and `X-Forwarded-*`
//! trust. The per-mode matrix and layer order are documented at the
//! [`data_plane`] module level.
//!
//! # Startup and concurrent safety
//!
//! kikan provides **in-process** concurrent-safety for every SeaORM pool
//! it manages:
//!
//! - `PRAGMA journal_mode=WAL` — readers don't block writers; writers
//!   serialize through a single write lock. Applied by [`db::pragmas`]
//!   on every connection.
//! - `PRAGMA busy_timeout=5000` — a 5-second retry window before
//!   `SQLITE_BUSY` surfaces to the caller, enough for short write
//!   contention within a single Engine to self-resolve.
//! - Migrations run in `SqliteTransactionMode::Immediate` (see
//!   [`migrations::runner`]) — each migration acquires the write lock
//!   on `BEGIN` rather than upgrading lazily, so concurrent writes from
//!   application tasks cannot race a boot-time migration.
//!
//! **Cross-process** single-Engine-per-data-directory is the caller's
//! precondition — kikan does not enforce it. Two Engines booted
//! against the same data directory collide in three places that WAL
//! does not cover: sidecar swaps (demo reset, restore) manipulate the
//! database files outside SQLite's locking protocol; backup
//! destination filenames are app-chosen, so concurrent backups race
//! the filesystem rather than SQLite's locks; migration runs do
//! serialize through the write lock, but the losing Engine fails to
//! boot (spurious migration errors or `SQLITE_BUSY` once
//! `busy_timeout` elapses) rather than cooperating. The Application
//! is responsible for single-instance enforcement; see
//! `ops/decisions/mokumo/adr-process-lock-flock.md` for the
//! adapter-level mechanism.
//!
//! Place platform-shaped code here. Shop-vertical identifiers belong
//! in `mokumo-shop` (invariant I1, enforced by
//! `scripts/check-i1-domain-purity.sh`); shell adapters in
//! `kikan-tauri` / `kikan-socket` / `kikan-cli`. See
//! `ops/decisions/mokumo/adr-kikan-engine-vocabulary.md`.

pub mod activity;
pub mod actor;
pub mod app_error;
pub mod app_handle;
pub mod auth;
pub mod backup;
pub mod boot;
pub mod control_plane;
pub mod control_plane_error;
pub mod data_plane;
pub mod db;
pub mod engine;
pub mod error;
pub mod filter;
pub mod graft;
pub mod logging;
pub mod meta;
pub mod middleware;
pub mod migrations;
pub mod pagination;
pub mod platform;
pub mod platform_state;
pub mod profile_db;
pub mod rate_limit;
pub mod slug;
pub mod tenancy;

pub use activity::{ActivityLogEntry, ActivityWriter, SqliteActivityWriter};
pub use app_error::AppError;
pub use app_handle::AppHandleShim;
pub use auth::recovery_artifact::{RecoveryArtifactLocation, RecoveryError};
pub use boot::{BootConfig, RateLimitConfig, RateWindow};
pub use control_plane::{ControlPlaneState, PinId, PinIdError, SetupTokenSource};
pub use control_plane_error::{ConflictKind, ControlPlaneError};
pub use data_plane::{ConfigError, DataPlaneConfig, DeploymentMode, HostPattern, HostPatternError};
pub use engine::{Engine, EngineContext, Sessions};
pub use error::{
    ActivityWriteError, AppHandleError, DagError, DomainError, EngineError, MigrationError,
    TenancyError,
};
pub use graft::{Graft, SelfGraft, SidecarRecovery, SidecarRecoveryError, SubGraft};
pub use meta::{
    BUNDLE_MANIFEST_SCHEMA_VERSION, BootState, BootStateDetectionError, BundleBackupError,
    BundleManifest, BundleManifestEntry, BundleRestoreError, DbInBundle, Profile, ProfileRepo,
    ProfileRepoError, RestoreTarget, SeaOrmProfileRepo, SidecarRecoveryDiagnostic, create_bundle,
    detect_boot_state, restore_bundle,
};
pub use migrations::{GraftId, Migration, MigrationRef, MigrationTarget};
pub use platform_state::{MdnsStatus, PlatformState, SharedMdnsStatus};
pub use profile_db::{ActiveProfile, ProfileDb};
pub use slug::{MAX_SLUG_LEN, RESERVED_SLUGS, Slug, SlugError, derive_slug};
pub use tenancy::{ProfileId, Tenancy};

// The graft's `ProfileKind` is the vertical's concern — kikan itself
// does not name the type in production code (I1 strict). Wire DTOs that
// embed a concrete variant (test fixtures, shop adapters, CLI) import
// the type from `kikan-types` or the vertical directly.
