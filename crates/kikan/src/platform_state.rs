//! Platform-state slice shared across kikan-owned handlers.
//!
//! `PlatformState` groups the fields that platform handlers — diagnostics,
//! demo reset, backup status, diagnostics bundle — need to function. Kept
//! separate from the vertical's `AppState` so kikan handlers stay free of
//! any I4-violating dependency on `mokumo-shop`.
//!
//! ## Wiring
//!
//! The vertical's `AppState` (in `mokumo_shop::state`) composes a
//! `PlatformState` and exposes it via `Graft::platform_state` so Axum
//! handlers can extract it directly:
//!
//! ```ignore
//! use axum::extract::State;
//! use kikan::PlatformState;
//!
//! async fn some_handler(State(platform): State<PlatformState>) { ... }
//! ```
//!
//! Fields here are intentionally platform-generic — no shop-vertical
//! identifiers. `MdnsStatus` is considered platform infra (LAN
//! discovery, not a shop concept).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use parking_lot::RwLock;
use sea_orm::DatabaseConnection;
use tokio_util::sync::CancellationToken;

use crate::db::DatabaseSetupError;
use crate::tenancy::SetupMode;

/// Re-initialize a profile database from a freshly-copied file.
///
/// Used by `platform::demo::demo_reset` after the demo sidecar has been
/// force-copied: the host wires a closure that opens the new pool, runs the
/// vertical migrator, and applies post-migration optimizations. Defined here
/// (instead of in `crate::platform::demo`) so [`PlatformState`] can carry it
/// without dragging the demo handler into the type's public surface.
pub trait ProfileDbInitializer: Send + Sync + 'static {
    fn initialize<'a>(
        &'a self,
        database_url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<DatabaseConnection, DatabaseSetupError>> + Send + 'a>>;
}

/// Type alias for the shared profile-DB initializer. Cloning is `Arc::clone`.
pub type SharedProfileDbInitializer = Arc<dyn ProfileDbInitializer>;

/// LAN-discovery status snapshot. Populated by the mDNS registration task
/// and read by diagnostics handlers.
#[derive(Debug, Clone)]
pub struct MdnsStatus {
    pub active: bool,
    pub hostname: Option<String>,
    pub port: u16,
    pub bind_host: String,
}

impl Default for MdnsStatus {
    fn default() -> Self {
        Self {
            active: false,
            hostname: None,
            port: 0,
            bind_host: "127.0.0.1".into(),
        }
    }
}

/// Shared, mutable mDNS status snapshot. Writers are the discovery task;
/// readers are diagnostics / server-info handlers.
pub type SharedMdnsStatus = Arc<RwLock<MdnsStatus>>;

impl MdnsStatus {
    pub fn shared() -> SharedMdnsStatus {
        Arc::new(RwLock::new(Self::default()))
    }
}

/// Platform-state slice — the subset of `AppState` that is kikan-owned.
///
/// Every field has O(1) `Clone` — either `Arc<T>`, a watch channel
/// receiver, a cancellation token, or `DatabaseConnection` (internally
/// `Arc`-wrapped). Cloning on every request via `FromRef` is cheap.
///
/// Fields:
/// - `data_dir` — root data directory (per-profile databases, logs, backups live under this).
/// - `demo_db` / `production_db` — open `SeaORM` pools for each profile.
/// - `active_profile` — currently active profile; non-poisoning `RwLock`.
/// - `shutdown` — platform shutdown token; cancelling drops the server.
/// - `started_at` — server boot instant; used for uptime reporting.
/// - `mdns_status` — shared LAN-discovery status snapshot.
/// - `demo_install_ok` — whether the demo profile has a valid admin seeded.
/// - `is_first_launch` — true until the first profile switch completes.
/// - `setup_completed` — true once the production setup wizard finishes.
/// - `profile_db_initializer` — vertical-supplied hook for re-opening and
///   re-migrating a profile database after sidecar copy (used by demo reset).
///   Boxed behind `Arc<dyn …>` so kikan holds no vertical migrator edge (I4).
#[derive(Clone)]
pub struct PlatformState {
    pub data_dir: PathBuf,
    pub demo_db: DatabaseConnection,
    pub production_db: DatabaseConnection,
    pub active_profile: Arc<RwLock<SetupMode>>,
    pub shutdown: CancellationToken,
    pub started_at: std::time::Instant,
    pub mdns_status: SharedMdnsStatus,
    pub demo_install_ok: Arc<AtomicBool>,
    pub is_first_launch: Arc<AtomicBool>,
    pub setup_completed: Arc<AtomicBool>,
    /// Vertical-supplied hook used by `platform::demo::demo_reset` to
    /// re-open and re-migrate the demo profile after the sidecar copy.
    /// Kept behind `Arc<dyn …>` so kikan does not depend on any vertical
    /// migrator (preserves I4).
    pub profile_db_initializer: SharedProfileDbInitializer,
}

impl PlatformState {
    /// Return the database connection for the given profile.
    pub fn db_for(&self, mode: SetupMode) -> &DatabaseConnection {
        match mode {
            SetupMode::Demo => &self.demo_db,
            SetupMode::Production => &self.production_db,
        }
    }

    /// Whether setup is complete for the currently active profile.
    ///
    /// Demo is always pre-seeded and never requires the setup wizard, so this
    /// returns `true` unconditionally in demo mode. Production reads the
    /// `setup_completed` flag set when the wizard finishes.
    pub fn is_setup_complete(&self) -> bool {
        match *self.active_profile.read() {
            SetupMode::Demo => true,
            SetupMode::Production => self
                .setup_completed
                .load(std::sync::atomic::Ordering::Acquire),
        }
    }
}
