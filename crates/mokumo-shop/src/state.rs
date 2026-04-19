//! Composed application state for the Mokumo shop graft.
//!
//! `MokumoShopState` holds the domain-specific fields that are neither
//! platform (kikan) nor control-plane concerns. `MokumoState` composes
//! platform + control-plane + domain into the full application state
//! that axum handlers and middleware extract from.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use kikan::rate_limit::RateLimiter;
use kikan::{ControlPlaneState, PlatformState};
use tokio::sync::watch;

use crate::ws::ConnectionManager;

/// Domain-specific state for the Mokumo shop graft.
///
/// Fields here are shop-vertical concerns that don't belong in
/// `PlatformState` (kikan-owned) or `ControlPlaneState` (admin surface).
#[derive(Clone)]
pub struct MokumoShopState {
    /// WebSocket connection manager for real-time broadcast to shop UI.
    pub ws: Arc<ConnectionManager>,
    /// Watch receiver for the local IP address (refreshed periodically).
    pub local_ip: watch::Receiver<Option<IpAddr>>,
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

impl MokumoState {
    /// Return a reference to the platform-state slice.
    pub fn platform(&self) -> &PlatformState {
        &self.control_plane.platform
    }
}
