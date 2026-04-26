//! Diagnostics surfaced by [`crate::Engine::boot`] for operator
//! consumption over the UDS admin surface.
//!
//! Currently scoped to sidecar recovery — the engine writes one entry
//! per profile kind that was force-copied from its bundled sidecar at
//! boot. The map remains empty for healthy installs; entries are
//! cleared only by restart (the next boot rebuilds the map from
//! scratch).

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One sidecar recovery diagnostic — emitted when the engine's
/// boot-time self-repair pass detects a missing or corrupt profile
/// database and force-copies a fresh one from the vertical's bundled
/// sidecar.
///
/// Surfaced via the UDS admin socket so an operator UI can render a
/// banner when bundled-data profiles were restored from the sidecar at
/// boot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarRecoveryDiagnostic {
    pub source: PathBuf,
    pub dest: PathBuf,
    pub bytes: u64,
    pub recovered_at: DateTime<Utc>,
}
