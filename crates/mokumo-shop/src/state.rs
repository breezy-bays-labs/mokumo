//! Composed application state for the Mokumo shop graft.
//!
//! `MokumoShopState` holds the domain-specific fields that are neither
//! platform (kikan) nor control-plane concerns. `MokumoState` composes
//! platform + control-plane + domain into the full application state
//! that axum handlers and middleware extract from.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use dashmap::DashMap;
use kikan::platform_state::SharedProfileDbInitializer;
use kikan::rate_limit::RateLimiter;
use kikan::{ActivityWriter, ControlPlaneState, PlatformState};
use kikan_types::SetupMode;
use parking_lot::RwLock;
use sea_orm::DatabaseConnection;
use tokio_util::sync::CancellationToken;

use crate::auth::PendingReset;
use crate::ws::ConnectionManager;

/// Domain-specific state for the Mokumo shop graft.
///
/// Fields here are shop-vertical concerns that don't belong in
/// `PlatformState` (kikan-owned) or `ControlPlaneState` (admin surface).
///
/// Session 3 absorbs the file-drop-reset-PIN map and recovery-file
/// directory from `ControlPlaneState`. Both are vertical vocabulary —
/// kikan exposes the recovery-dir path via `Graft::recovery_dir` but
/// does not store it. The PIN hash/expiry format (`PendingReset`) is
/// mokumo's shape.
#[derive(Clone)]
pub struct MokumoShopState {
    /// WebSocket connection manager for real-time broadcast to shop UI.
    pub ws: Arc<ConnectionManager>,
    /// Local IP address, refreshed periodically by `spawn_background_tasks`.
    pub local_ip: Arc<RwLock<Option<IpAddr>>>,
    /// Prevents concurrent restore operations.
    pub restore_in_progress: Arc<AtomicBool>,
    /// Rate limiter for restore attempts (5 per hour, shared across validate + restore).
    pub restore_limiter: Arc<RateLimiter>,
    /// In-memory store for file-drop password reset PINs, keyed by email.
    /// Swept lazily on read and periodically by the PIN-sweep background
    /// task.
    pub reset_pins: Arc<DashMap<String, PendingReset>>,
    /// Directory where recovery files are dropped for the file-drop
    /// password reset flow. Resolved via `crate::startup::resolve_recovery_dir`
    /// at `build_domain_state` time. Arc so clone is a refcount bump.
    pub recovery_dir: Arc<PathBuf>,
    /// Debug-only WebSocket heartbeat interval in milliseconds.
    #[cfg(debug_assertions)]
    pub ws_ping_ms: Option<u64>,
}

/// Full composed application state: platform + control-plane + domain.
///
/// Always consumed behind `Arc` (see `SharedMokumoState`) so per-request
/// cloning via `FromRef` is O(1).
pub struct MokumoState {
    pub control_plane: ControlPlaneState,
    pub domain: MokumoShopState,
}

/// The `AppState` type used by `MokumoApp: Graft`.
pub type SharedMokumoState = Arc<MokumoState>;

// ── Convenience accessors ─────────────────────────────────────────────
//
// These delegate to the composed sub-states so handler code can use
// `state.data_dir()` instead of `state.control_plane.platform.data_dir`.
// Transitional: handlers will access sub-states directly once they
// move to their final crate homes in PR 3/4.

impl MokumoState {
    // ── Platform accessors ────────────────────────────────────────────

    pub fn platform_state(&self) -> PlatformState {
        self.control_plane.platform.clone()
    }

    pub fn control_plane_state(&self) -> ControlPlaneState {
        self.control_plane.clone()
    }

    pub fn db_for(&self, mode: SetupMode) -> &DatabaseConnection {
        self.control_plane
            .platform
            .db_for(mode.as_dir_name())
            .expect("mokumo SetupMode variant always present in PlatformState pools")
    }

    pub fn is_setup_complete(&self) -> bool {
        self.control_plane.platform.is_setup_complete()
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.control_plane.platform.data_dir
    }

    pub fn demo_db(&self) -> &DatabaseConnection {
        self.control_plane
            .platform
            .db_for("demo")
            .expect("demo profile pool present in PlatformState")
    }

    pub fn production_db(&self) -> &DatabaseConnection {
        self.control_plane
            .platform
            .db_for("production")
            .expect("production profile pool present in PlatformState")
    }

    /// Active profile read lock — returns the `SetupMode` variant after
    /// round-tripping the kikan-side `ProfileDirName` through `FromStr`.
    ///
    /// `Engine::boot` validates the round-trip at startup, so a `None`
    /// return here would signal kikan bookkeeping drift (a stale
    /// `active_profile` file not matching any declared kind). Callers that
    /// want a fallible read use this; callers that want the legacy
    /// "silent fallback to Demo" call [`Self::active_profile_mode_or_demo`]
    /// — and should prefer the fallible variant in new code.
    pub fn active_profile_mode_opt(&self) -> Option<SetupMode> {
        use std::str::FromStr;
        let active = self.control_plane.platform.active_profile.read().clone();
        match SetupMode::from_str(active.as_str()) {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::error!(
                    dir = active.as_str(),
                    "active_profile_mode: kikan-side dir does not parse to SetupMode: {e}"
                );
                None
            }
        }
    }

    /// Active profile as `SetupMode`, falling back to `Demo` if the
    /// kikan-side dir does not round-trip (with a `tracing::error!`).
    /// Preferred new code uses [`Self::active_profile_mode_opt`]; this
    /// stays for handlers that need a concrete variant and can accept the
    /// Demo fallback semantics.
    pub fn active_profile_mode(&self) -> SetupMode {
        self.active_profile_mode_opt().unwrap_or(SetupMode::Demo)
    }

    /// Write-access to the kikan-side active-profile lock. Callers that
    /// mutate this must set the opaque `ProfileDirName`; kikan no longer
    /// stores `SetupMode`.
    pub fn active_profile(&self) -> &Arc<RwLock<kikan::tenancy::ProfileDirName>> {
        &self.control_plane.platform.active_profile
    }

    pub fn shutdown(&self) -> &CancellationToken {
        &self.control_plane.platform.shutdown
    }

    pub fn started_at(&self) -> std::time::Instant {
        self.control_plane.platform.started_at
    }

    pub fn mdns_status(&self) -> &kikan::SharedMdnsStatus {
        &self.control_plane.platform.mdns_status
    }

    pub fn demo_install_ok(&self) -> &Arc<AtomicBool> {
        &self.control_plane.platform.demo_install_ok
    }

    pub fn is_first_launch(&self) -> &Arc<AtomicBool> {
        &self.control_plane.platform.is_first_launch
    }

    pub fn setup_completed(&self) -> &Arc<AtomicBool> {
        &self.control_plane.platform.setup_completed
    }

    pub fn profile_db_initializer(&self) -> &SharedProfileDbInitializer {
        &self.control_plane.platform.profile_db_initializer
    }

    // ── Control-plane accessors ───────────────────────────────────────

    pub fn login_limiter(&self) -> &Arc<RateLimiter> {
        &self.control_plane.login_limiter
    }

    pub fn recovery_limiter(&self) -> &Arc<RateLimiter> {
        &self.control_plane.recovery_limiter
    }

    pub fn regen_limiter(&self) -> &Arc<RateLimiter> {
        &self.control_plane.regen_limiter
    }

    pub fn switch_limiter(&self) -> &Arc<RateLimiter> {
        &self.control_plane.switch_limiter
    }

    pub fn reset_pins(&self) -> &Arc<DashMap<String, PendingReset>> {
        &self.domain.reset_pins
    }

    pub fn recovery_dir(&self) -> &PathBuf {
        &self.domain.recovery_dir
    }

    pub fn setup_token(&self) -> Option<&str> {
        self.control_plane.setup_token.as_deref()
    }

    pub fn setup_in_progress(&self) -> &Arc<AtomicBool> {
        &self.control_plane.setup_in_progress
    }

    pub fn activity_writer(&self) -> &Arc<dyn ActivityWriter> {
        &self.control_plane.activity_writer
    }

    // ── Domain accessors ──────────────────────────────────────────────

    pub fn ws(&self) -> &Arc<ConnectionManager> {
        &self.domain.ws
    }

    pub fn local_ip(&self) -> &Arc<RwLock<Option<IpAddr>>> {
        &self.domain.local_ip
    }

    pub fn restore_in_progress(&self) -> &Arc<AtomicBool> {
        &self.domain.restore_in_progress
    }

    pub fn restore_limiter(&self) -> &Arc<RateLimiter> {
        &self.domain.restore_limiter
    }

    #[cfg(debug_assertions)]
    pub fn ws_ping_ms(&self) -> Option<u64> {
        self.domain.ws_ping_ms
    }
}
