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
//! ## Capability vs vocabulary
//!
//! The `pools`, `profile_dir_names`, `requires_setup_by_dir`, and
//! `db_filename` fields are *capability data*: boot-time snapshots sourced
//! from the `Graft` (a verticals `ProfileKind` enum, its `db_filename()`,
//! and its per-kind vocabulary hooks) and reduced to opaque `ProfileDirName`
//! keys. Kikan never names a specific profile — it iterates the keys the
//! graft handed it and looks up pools by string.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use parking_lot::RwLock;
use sea_orm::DatabaseConnection;
use tokio_util::sync::CancellationToken;

use crate::db::DatabaseSetupError;
use crate::tenancy::ProfileDirName;

/// Re-initialize a profile database from a freshly-copied file.
///
/// Used by the demo-reset handler after the demo sidecar has been
/// force-copied: the host wires a closure that opens the new pool, runs the
/// vertical migrator, and applies post-migration optimizations. Defined here
/// (instead of alongside the demo handler) so [`PlatformState`] can carry it
/// without dragging the handler into the type's public surface.
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
#[derive(Clone)]
pub struct PlatformState {
    /// Root data directory — per-profile DB files, logs, backups live here.
    pub data_dir: PathBuf,
    /// Profile DB filename sourced from `Graft::db_filename()` at boot.
    /// Kikan uses it to construct `{data_dir}/{dir_name}/{db_filename}` paths
    /// without naming the vertical file.
    pub db_filename: &'static str,
    /// Per-profile database connections, keyed by the opaque
    /// [`ProfileDirName`] sourced from `Graft::profile_dir_name`. Lookup
    /// by `&str` via `db_for`.
    pub pools: Arc<HashMap<ProfileDirName, DatabaseConnection>>,
    /// Currently active profile (opaque dir name). Non-poisoning `RwLock`.
    pub active_profile: Arc<RwLock<ProfileDirName>>,
    /// Stable-order snapshot of all profile directory names the graft declared.
    /// Used by enumerators (backup listing, diagnostics, profile listing) that
    /// iterate profiles without naming them.
    pub profile_dir_names: Arc<[ProfileDirName]>,
    /// Per-profile "does this profile require the setup wizard" — sourced
    /// from `Graft::requires_setup_wizard(&kind)` at boot. Drives
    /// `is_setup_complete`.
    pub requires_setup_by_dir: Arc<HashMap<ProfileDirName, bool>>,
    /// Directory name for the profile kind that credentialed login
    /// authenticates against — sourced from `Graft::auth_profile_kind`
    /// at boot and passed through `Graft::profile_dir_name`. Consumed by
    /// `engine::build_router` to bind `Backend<K>::auth_kind` via
    /// `K::from_str(...)`.
    pub auth_profile_kind_dir: ProfileDirName,
    pub shutdown: CancellationToken,
    pub started_at: std::time::Instant,
    pub mdns_status: SharedMdnsStatus,
    pub demo_install_ok: Arc<AtomicBool>,
    pub is_first_launch: Arc<AtomicBool>,
    pub setup_completed: Arc<AtomicBool>,
    /// Vertical-supplied hook used by the demo-reset handler to re-open
    /// and re-migrate a profile database after a sidecar copy. Boxed
    /// behind `Arc<dyn …>` so kikan does not depend on any vertical
    /// migrator (preserves I4).
    pub profile_db_initializer: SharedProfileDbInitializer,
}

impl PlatformState {
    /// Look up a profile pool by directory-name string. Returns `None` when
    /// the caller names a profile the graft never declared.
    pub fn db_for(&self, dir_name: &str) -> Option<&DatabaseConnection> {
        self.pools.get(dir_name)
    }

    /// Borrow the pool for the currently-active profile. Panics if the
    /// active profile key is not present in `pools`; boot code establishes
    /// that invariant.
    pub fn active_db(&self) -> DatabaseConnection {
        let active = self.active_profile.read();
        self.pools
            .get(&*active)
            .cloned()
            .expect("active profile pool present in platform state")
    }

    /// Whether setup is complete for the currently active profile.
    ///
    /// Driven entirely by `Graft::requires_setup_wizard` snapshots captured
    /// at boot. Profiles that do not require the setup wizard report
    /// complete unconditionally; profiles that do require it read the
    /// `setup_completed` flag set when the wizard finishes. Kikan never
    /// names a specific profile here.
    pub fn is_setup_complete(&self) -> bool {
        let active = self.active_profile.read();
        let requires = self
            .requires_setup_by_dir
            .get(&*active)
            .copied()
            .unwrap_or(false);
        !requires
            || self
                .setup_completed
                .load(std::sync::atomic::Ordering::Acquire)
    }
}
