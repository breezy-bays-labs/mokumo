//! `PendingReset` — the Mokumo-side shape for a file-drop password-reset
//! entry.
//!
//! The reset flow writes an HTML file containing a 6-digit PIN to the
//! recovery directory and stashes the hashed PIN (with wall-clock issue
//! time) in a `DashMap<email, PendingReset>` keyed by email. Expired
//! entries are pruned lazily by the reset handler and periodically by
//! the PIN-sweep background task spawned in [`MokumoApp::spawn_background_tasks`].
//!
//! This type lives in mokumo-shop, not kikan, because the PIN format,
//! the hashed-PIN storage scheme, and the expiry window are all vertical
//! vocabulary. Kikan exposes the surface (`Graft::valid_reset_pin_ids`,
//! `Graft::recovery_dir`) without owning the storage.

/// A pending file-drop password reset entry — the hashed PIN plus the
/// wall-clock instant it was issued.
///
/// Expired entries are pruned lazily by the `reset_password` handler in
/// [`crate::auth_handlers::reset`] and by the PIN-sweep background task.
pub struct PendingReset {
    pub pin_hash: String,
    pub created_at: std::time::SystemTime,
}
