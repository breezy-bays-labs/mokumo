//! Vertical-extension surface for the file-drop password-reset flow.
//!
//! `kikan::control_plane::auth::recover_request` generates a 6-digit PIN,
//! hashes it for storage, and asks the active [`crate::Graft`] to put a
//! recovery artifact somewhere the operator can find it. What "somewhere"
//! means is vertical vocabulary — one vertical writes a branded HTML
//! file to a known directory; another might send an email or fire a
//! push notification. The contract surfaced here is intentionally narrow:
//! given an email, a PIN, and a directory the engine considers durable,
//! the graft returns *where it put the artifact* (or that no artifact
//! was produced) plus any I/O failure.
//!
//! Sync — the trait method is `fn`, not `async fn`, so file-based
//! implementations may use `std::fs` directly. Callers running inside a
//! tokio runtime should wrap in `spawn_blocking` for non-trivial writes;
//! a single-kilobyte HTML file is well below the threshold where
//! blocking the executor matters.

use std::path::PathBuf;

/// Where the vertical placed the recovery artifact, returned as
/// human-readable feedback for the operator.
///
/// Marked `#[non_exhaustive]` so future verticals can add transports
/// (e.g. `Email`, `PushNotification`) without a breaking change for
/// downstream callers that match on the variants.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum RecoveryArtifactLocation {
    /// The artifact is a file the operator can open locally.
    File { path: PathBuf },
    /// The artifact was delivered out-of-band (email, push, …).
    /// The string describes where to look for it.
    External { description: String },
    /// No artifact was produced. Useful for verticals that want
    /// silent enumeration-resistance — the caller still acts as if
    /// the request succeeded.
    None,
}

/// Failure modes for [`crate::Graft::write_recovery_artifact`].
///
/// `NotSupported` is the default-impl response — verticals that don't
/// implement the hook return it so the engine can degrade gracefully.
///
/// `Io` carries underlying filesystem errors when a file-based
/// implementation fails to write its artifact.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RecoveryError {
    #[error("vertical does not implement write_recovery_artifact")]
    NotSupported,
    #[error("recovery artifact I/O failed: {0}")]
    Io(#[from] std::io::Error),
}
