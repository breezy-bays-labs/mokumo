//! `ControlPlaneState` — the unified state slice consumed by every pure-fn
//! under `kikan::control_plane::*`.
//!
//! Extends [`PlatformState`] with the four auth rate limiters, a resolved
//! setup-wizard token (sourced via [`Graft::setup_token_source`] at boot),
//! the setup-in-progress latch, and the activity writer that admin-surface
//! operations need. Every field is O(1)-clonable (`Arc` or primitive) so
//! handlers and one-shot callers can keep a cheap Clone semantics on
//! `Router<ControlPlaneState>` and in-process CLI paths.
//!
//! Vertical-specific recovery vocabulary (reset-PIN stores, recovery-file
//! directories, concrete reset-entry shapes) does not live here; that
//! surface belongs to the graft via `Graft::recovery_dir`,
//! `Graft::setup_token_source`, and `Graft::valid_reset_pin_ids`
//! (see `adr-kikan-engine-vocabulary.md` § "Amendment 2026-04-22 (b)").

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::PlatformState;
use crate::activity::ActivityWriter;
use crate::rate_limit::RateLimiter;

/// Transport-neutral state for admin-surface control-plane operations.
///
/// Fields:
/// - `platform` — kikan-owned platform slice (DB pools, active profile,
///   setup flags, mDNS status, shutdown token, demo-install status).
/// - `login_limiter` / `recovery_limiter` / `regen_limiter` /
///   `switch_limiter` — per-concern in-memory rate limiters. All behind
///   `Arc` so the struct stays O(1) `Clone`.
/// - `setup_token` — opaque token gating the first-admin setup wizard.
///   Resolved at boot from `Graft::setup_token_source()` (read from a
///   file, cloned from an inline value, or left `None` when the vertical
///   disables the wizard). `None` after setup completes.
/// - `setup_in_progress` — atomic flag preventing concurrent bootstrap
///   attempts.
/// - `activity_writer` — shared writer used by mutation fns to append an
///   activity-log entry inside the same transaction as the mutation
///   (CLAUDE.md #12).
#[derive(Clone)]
pub struct ControlPlaneState {
    pub platform: PlatformState,
    pub login_limiter: Arc<RateLimiter>,
    pub recovery_limiter: Arc<RateLimiter>,
    pub regen_limiter: Arc<RateLimiter>,
    pub switch_limiter: Arc<RateLimiter>,
    pub setup_token: Option<Arc<str>>,
    pub setup_in_progress: Arc<AtomicBool>,
    pub activity_writer: Arc<dyn ActivityWriter>,
}
