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

use kikan::platform_state::SharedProfileDbInitializer;
use kikan::rate_limit::RateLimiter;
use kikan::tenancy::SetupMode;
use kikan::{ActivityWriter, ControlPlaneState, PlatformState};
use parking_lot::RwLock;
use sea_orm::DatabaseConnection;
use tokio_util::sync::CancellationToken;

use crate::ws::ConnectionManager;

/// Domain-specific state for the Mokumo shop graft.
///
/// Fields here are shop-vertical concerns that don't belong in
/// `PlatformState` (kikan-owned) or `ControlPlaneState` (admin surface).
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
        self.control_plane.platform.db_for(mode)
    }

    pub fn is_setup_complete(&self) -> bool {
        self.control_plane.platform.is_setup_complete()
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.control_plane.platform.data_dir
    }

    pub fn demo_db(&self) -> &DatabaseConnection {
        &self.control_plane.platform.demo_db
    }

    pub fn production_db(&self) -> &DatabaseConnection {
        &self.control_plane.platform.production_db
    }

    pub fn active_profile(&self) -> &Arc<RwLock<SetupMode>> {
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

    pub fn reset_pins(&self) -> &Arc<dashmap::DashMap<String, kikan::PendingReset>> {
        &self.control_plane.reset_pins
    }

    pub fn recovery_dir(&self) -> &PathBuf {
        &self.control_plane.recovery_dir
    }

    pub fn setup_token(&self) -> &Option<String> {
        &self.control_plane.setup_token
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
