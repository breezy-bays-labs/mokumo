//! Recovery-session pure-fn core.
//!
//! Two functions: [`recover_request`] mints a session for a known email
//! (or a synthesized session-id-only response for an unknown one, to
//! preserve enumeration resistance), and [`recover_complete`] consumes a
//! session by validating a 6-digit PIN and updating the user's password.
//!
//! Storage is [`crate::PlatformState::reset_pins`], an in-memory
//! `DashMap<RecoverySessionId, PendingReset>`. Eviction is two-tiered:
//! atomic `remove` on every redeem attempt (lazy) plus a 60s background
//! sweep (see [`super::sweep`]) that bounds memory at
//! `~PIN_EXPIRY × issuance_rate`.
//!
//! ## Anti-enumeration
//!
//! [`recover_request`] always returns a [`RecoverySessionId`] and a
//! [`RecoveryArtifactLocation`] regardless of whether `email` matches a
//! known user. On unknown emails the engine synthesises a session id
//! with no DashMap entry; any later [`recover_complete`] against that
//! token fails uniformly with [`ControlPlaneError::Validation`] and the
//! same wire shape as a wrong PIN. Callers cannot distinguish
//! "unknown email" from "wrong PIN" from "expired session".
//!
//! Both the artifact write and the Argon2id PIN hash run on every
//! request — known and unknown — so the response time profile is
//! identical regardless of whether the email matches a user. The
//! Argon2id work for unknown emails is discarded; that wasted CPU is
//! the cost of flattening the timing oracle.
//!
//! ## Concurrency
//!
//! `DashMap::remove(&session_id)` is the atomic compare-and-swap. Two
//! concurrent `recover_complete` calls with the same session id race —
//! exactly one wins the `remove`; the other gets `None` and the uniform
//! 400. The winning thread either consumes the entry (success or 3rd
//! failed attempt) or re-inserts with `attempts += 1`. The increment
//! cannot race with another concurrent submission because the entry is
//! held in local-fn scope between the `remove` and the conditional
//! `insert`, which closes a TOCTOU window the previous email-keyed
//! design had open.

use std::time::SystemTime;

use sea_orm::DatabaseConnection;

use super::pending_reset::{MAX_PIN_ATTEMPTS, PIN_EXPIRY, PendingReset, RecoverySessionId};
use crate::ControlPlaneError;
use crate::PlatformState;
use crate::auth::UserRepository;
use crate::auth::password;
use crate::auth::recovery_artifact::{RecoveryArtifactLocation, RecoveryError};
use crate::auth::{SeaOrmUserRepo, UserId};

/// Successful return shape for [`recover_request`].
#[derive(Debug)]
pub struct RecoverRequestOutcome {
    /// Opaque high-entropy token the SPA carries to `recover/complete`.
    /// Always present, even for unknown emails (synthesised) — see
    /// module-level "anti-enumeration".
    pub session_id: RecoverySessionId,
    /// Where the vertical placed the artifact. For unknown emails the
    /// engine returns the location the vertical *would have used*, so
    /// the response shape is identical to the known-email path.
    pub location: RecoveryArtifactLocation,
}

/// Mint a recovery session for `email` and write the operator-facing
/// recovery artifact via the configured writer closure.
///
/// `write_artifact` is the closure installed at boot via
/// [`crate::BootConfig::with_recovery_writer`]; passing a closure
/// rather than `&dyn Graft` keeps `control_plane` free of the trait
/// import that would otherwise couple the pure layer to the vertical
/// extension surface.
///
/// Both `write_artifact` and the Argon2id PIN hash run unconditionally
/// — see "Anti-enumeration" in the module docs. The hash is discarded
/// when no user matches.
pub async fn recover_request<F>(
    state: &PlatformState,
    db: &DatabaseConnection,
    email: &str,
    write_artifact: F,
) -> Result<RecoverRequestOutcome, ControlPlaneError>
where
    F: FnOnce(&str, &str) -> Result<RecoveryArtifactLocation, RecoveryError>,
{
    let repo = SeaOrmUserRepo::new(db.clone());
    let user = repo
        .find_by_email(email)
        .await
        .map_err(|e| ControlPlaneError::Internal(anyhow::anyhow!(e)))?;

    let pin = generate_pin();
    let location =
        write_artifact(email, &pin).map_err(|e| ControlPlaneError::Internal(anyhow::anyhow!(e)))?;

    let pin_hash = password::hash_password(pin)
        .await
        .map_err(|e| ControlPlaneError::Internal(anyhow::anyhow!(e)))?;

    let session_id = RecoverySessionId::generate();

    let artifact_path = match &location {
        RecoveryArtifactLocation::File { path } => Some(path.clone()),
    };

    if let Some(user) = user {
        // Invalidate any prior session minted for the same user. Without
        // this, repeated `recover_request` calls for one user accumulate
        // entries; the legacy `reset-password` shim's
        // `find_session_id_by_email` returns the first match and may
        // pick a stale session whose PIN no longer matches the artifact
        // (since the latest write overwrote the file at the same path).
        state.reset_pins.retain(|_, entry| entry.user_id != user.id);
        state.reset_pins.insert(
            session_id.clone(),
            PendingReset {
                pin_hash,
                user_id: user.id,
                created_at: SystemTime::now(),
                attempts: 0,
                artifact_path,
            },
        );
    } else {
        // Unknown email: synthesised session, no DashMap entry. The
        // throwaway artifact written above for timing uniformity has no
        // owner and would otherwise accumulate one file per unique
        // unknown email. Best-effort delete it now — the response is
        // already shaped, and deletion is a microsecond syscall (well
        // below the Argon2id-dominated response time).
        if let Some(path) = &artifact_path
            && let Err(e) = std::fs::remove_file(path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!(
                path = %path.display(),
                "recover_request: failed to clean up unknown-email artifact: {e}"
            );
        }
    }

    Ok(RecoverRequestOutcome {
        session_id,
        location,
    })
}

/// Validate a session-id + PIN pair and update the user's password.
///
/// Returns:
/// - `Ok(())` on success.
/// - `Err(Validation { … })` for: missing session, expired session,
///   wrong PIN, attempts exhausted. All four wire-shapes are identical
///   on purpose (anti-enumeration).
/// - `Err(Internal)` on infrastructure failure.
///
/// The DashMap entry is consumed atomically — see module-level
/// "Concurrency". An Argon2id KDF runs unconditionally so timing is
/// uniform between "session exists" and "session synthesised by an
/// anti-enumeration `recover_request` call against an unknown email".
pub async fn recover_complete(
    state: &PlatformState,
    db: &DatabaseConnection,
    session_id: &RecoverySessionId,
    pin: String,
    new_password: String,
) -> Result<(), ControlPlaneError> {
    let entry_opt = state.reset_pins.remove(session_id);

    // Run an Argon2id KDF unconditionally. The success path verifies
    // the user-supplied PIN against the stored hash; the missing-session
    // path burns the same KDF cost via a throwaway `hash_password` so a
    // synthesised session_id (for an unknown email) cannot be
    // distinguished by response time from a known-session-wrong-PIN
    // attempt.
    let valid = match entry_opt.as_ref() {
        Some((_, e)) => password::verify_password(pin, e.pin_hash.clone())
            .await
            .map_err(|err| ControlPlaneError::Internal(anyhow::anyhow!(err)))?,
        None => {
            password::hash_password(pin)
                .await
                .map_err(|err| ControlPlaneError::Internal(anyhow::anyhow!(err)))?;
            false
        }
    };

    let Some((_, entry)) = entry_opt else {
        return Err(invalid_session());
    };

    if entry
        .created_at
        .elapsed()
        .map(|d| d > PIN_EXPIRY)
        .unwrap_or(true)
    {
        return Err(invalid_session());
    }

    if !valid {
        let next_attempts = entry.attempts.saturating_add(1);
        if next_attempts < MAX_PIN_ATTEMPTS {
            state.reset_pins.insert(
                session_id.clone(),
                PendingReset {
                    attempts: next_attempts,
                    ..entry
                },
            );
        }
        return Err(invalid_session());
    }

    let repo = SeaOrmUserRepo::new(db.clone());
    repo.update_password(&entry.user_id, &new_password)
        .await
        .map_err(|e| ControlPlaneError::Internal(anyhow::anyhow!(e)))?;

    // Best-effort cleanup of the recovery artifact on success. Matches
    // the pre-promotion behavior the legacy reset_password handler had,
    // and avoids leaving a plaintext PIN file on disk after the operator
    // has consumed it. Failures are logged at warn — the redemption has
    // already succeeded and the response should not surface I/O noise.
    if let Some(path) = entry.artifact_path.as_ref()
        && let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            path = %path.display(),
            "recover_complete: failed to remove recovery artifact: {e}"
        );
    }

    Ok(())
}

/// Find the recovery session id minted for `user_id`. O(n) scan over
/// `reset_pins` — n is bounded by `~PIN_EXPIRY × issuance_rate` and is
/// effectively a handful of entries in single-tenant deployments.
///
/// Returned for the legacy `/api/auth/reset-password` shim, which takes
/// `{ email, pin, new_password }` over the wire and resolves the email
/// to a session id internally before delegating to [`recover_complete`].
pub fn find_session_id_by_email(
    state: &PlatformState,
    user_id: &UserId,
) -> Option<RecoverySessionId> {
    state
        .reset_pins
        .iter()
        .find(|entry| &entry.value().user_id == user_id)
        .map(|entry| entry.key().clone())
}

/// Uniform "session not valid" error.
///
/// Variants of "session not found", "session expired", "wrong PIN", and
/// "attempts exhausted" all map to this single error value so the
/// adapter cannot accidentally branch the wire shape on the rejection
/// reason.
fn invalid_session() -> ControlPlaneError {
    ControlPlaneError::Validation {
        field: "form".into(),
        message: "Invalid or expired recovery session".into(),
    }
}

/// Generate a uniformly-distributed 6-digit PIN, zero-padded.
fn generate_pin() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    format!("{:06}", rng.random_range(0..1_000_000u32))
}
