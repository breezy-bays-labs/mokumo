//! Background sweep that bounds [`crate::PlatformState::reset_pins`] memory
//! at `~PIN_EXPIRY × issuance_rate`.
//!
//! Lazy eviction inside [`super::recover_complete`] handles the redeem
//! path — `DashMap::remove` is the atomic consume on every submission,
//! correct or not. This sweep handles the abandoned path: a user who
//! requests a PIN but never returns to redeem it. Without the sweep,
//! such records would accumulate until process restart.
//!
//! The task is `tokio::spawn`ed once at engine boot. Cancellation
//! flows through [`crate::PlatformState::shutdown`] so the task ends
//! cleanly when the engine stops.

use std::time::SystemTime;

use crate::PlatformState;
use crate::control_plane::auth::pending_reset::PIN_EXPIRY;

/// Tick interval between sweeps. Memory ceiling is therefore
/// `~(PIN_EXPIRY + SWEEP_INTERVAL) × issuance_rate` (a record issued
/// just after a sweep waits up to one extra interval before eviction).
const SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_mins(1);

/// Spawn the reset-pin sweep. Idempotent at startup — the engine calls
/// it once after [`PlatformState`] construction.
pub fn spawn(platform: &PlatformState) {
    let pins = platform.reset_pins.clone();
    let token = platform.shutdown.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = tokio::time::sleep(SWEEP_INTERVAL) => {
                    let now = SystemTime::now();
                    pins.retain(|_, entry| {
                        now.duration_since(entry.created_at)
                            .unwrap_or(std::time::Duration::ZERO)
                            < PIN_EXPIRY
                    });
                }
                () = token.cancelled() => break,
            }
        }
    });
}
