//! Pure-function control-plane operations for profile switching.
//!
//! ## Transport-neutral boundary
//!
//! `switch_profile` encapsulates the three persistence operations that are
//! independent of HTTP / session machinery:
//!
//! 1. **User lookup** — verify the target email resolves to an active user in
//!    the target profile DB, capturing the `AuthenticatedUser` the adapter
//!    will hand to `auth_session.login`.
//! 2. **Disk persist** — write `active_profile` atomically (tmp-then-rename)
//!    so a crash between operations leaves the on-disk selection consistent.
//! 3. **Memory flip** — update `PlatformState::active_profile`.
//!
//! ## Session glue stays in the adapter
//!
//! Rate limiting, CSRF/Origin validation, session logout, session login, and
//! the SESSION_KEY_PRODUCTION_EMAIL carry-over are inherently transport-coupled
//! and live in `services/api/src/profile_switch.rs`. That adapter calls this
//! fn after resolving the target email from the active session.
//!
//! ## Rollback contract
//!
//! `SwitchOutcome::previous_profile` gives the adapter a cheap rollback handle:
//! if the subsequent `auth_session.logout()` or `auth_session.login()` fails, the
//! adapter should restore `state.platform.active_profile` to `previous_profile`
//! and make a best-effort disk rollback. The adapter owns this recovery path
//! because session errors are transport-native.

use kikan_types::admin::ProfileSwitchAdminResponse;

use crate::auth::{AuthenticatedUser, SeaOrmUserRepo};
use crate::{ControlPlaneError, PlatformState, SetupMode};

/// Result of a successful `switch_profile` call.
///
/// `new_user` is ready to pass to `auth_session.login()` in the HTTP adapter.
/// `previous_profile` enables the adapter to roll back `active_profile` if the
/// subsequent session operations fail (see module doc).
pub struct SwitchOutcome {
    /// The user record the adapter should log in under the new profile.
    pub new_user: AuthenticatedUser,
    /// The profile that was active before this switch. Used for rollback only.
    pub previous_profile: SetupMode,
}

/// Resolve the target user, persist the profile selection to disk, and flip
/// the in-memory `active_profile` — atomically w.r.t. the disk file.
///
/// # Errors
///
/// - `ControlPlaneError::NotFound` — `email` does not exist in the target
///   profile DB (e.g. production not yet set up). The HTTP adapter maps this
///   to 503 for the profile-switch endpoint to preserve the existing wire
///   behaviour.
/// - `ControlPlaneError::Internal` — DB query failure or filesystem error
///   during the atomic rename. Both are unexpected at this call site.
pub async fn switch_profile(
    state: &PlatformState,
    target: SetupMode,
    email: &str,
) -> Result<SwitchOutcome, ControlPlaneError> {
    // Step 1: Look up the target user BEFORE touching disk or memory. If the
    // account does not exist the caller sees an error and the active profile is
    // left unchanged.
    let repo = SeaOrmUserRepo::new(state.db_for(target).clone());
    let (user_domain, hash) = repo
        .find_by_email_with_hash(email)
        .await
        .map_err(|e| {
            tracing::error!(
                target = ?target,
                %email,
                "switch_profile: DB error during user lookup: {e}"
            );
            ControlPlaneError::Internal(anyhow::anyhow!("User lookup failed: {e}"))
        })?
        .ok_or_else(|| {
            tracing::error!(
                target = ?target,
                %email,
                "switch_profile: target user not found in target DB"
            );
            ControlPlaneError::NotFound
        })?;
    let new_user = AuthenticatedUser::new(user_domain, hash, target);

    // Step 2: Persist active_profile to disk atomically. Write to a temp file
    // on the same filesystem (guarantees the rename is atomic on POSIX), then
    // rename over the destination. A crash between the write and the rename
    // leaves the tmp file behind; the next startup ignores it.
    let profile_path = state.data_dir.join("active_profile");
    let profile_tmp = state.data_dir.join("active_profile.tmp");
    tokio::fs::write(&profile_tmp, target.as_str())
        .await
        .map_err(|e| {
            tracing::error!(
                target = ?target,
                path = %profile_tmp.display(),
                "switch_profile: write active_profile.tmp failed: {e}"
            );
            ControlPlaneError::Internal(anyhow::anyhow!("Failed to persist profile selection: {e}"))
        })?;
    tokio::fs::rename(&profile_tmp, &profile_path)
        .await
        .map_err(|e| {
            tracing::error!(
                target = ?target,
                src = %profile_tmp.display(),
                dst = %profile_path.display(),
                "switch_profile: rename active_profile.tmp → active_profile failed: {e}"
            );
            ControlPlaneError::Internal(anyhow::anyhow!("Failed to persist profile selection: {e}"))
        })?;

    // Step 3: Flip the in-memory active_profile. Capture the previous value so
    // the adapter can roll back if session operations fail after this point.
    let previous_profile = {
        let mut guard = state.active_profile.write();
        let prev = *guard;
        *guard = target;
        prev
    };

    Ok(SwitchOutcome {
        new_user,
        previous_profile,
    })
}

/// Switch the active profile without user lookup — admin-only variant.
///
/// Performs steps 2+3 of `switch_profile` (disk persist + memory flip)
/// without step 1 (user lookup). On the UDS admin surface, filesystem
/// permissions are the auth layer — there is no session to carry a user.
///
/// # Errors
///
/// - `ControlPlaneError::Internal` — filesystem error during the atomic
///   rename (unexpected at this call site).
pub async fn switch_profile_admin(
    state: &PlatformState,
    target: SetupMode,
) -> Result<ProfileSwitchAdminResponse, ControlPlaneError> {
    // Step 2: Persist active_profile to disk atomically.
    let profile_path = state.data_dir.join("active_profile");
    let profile_tmp = state.data_dir.join("active_profile.tmp");
    tokio::fs::write(&profile_tmp, target.as_str())
        .await
        .map_err(|e| {
            tracing::error!(
                target = ?target,
                path = %profile_tmp.display(),
                "switch_profile_admin: write active_profile.tmp failed: {e}"
            );
            ControlPlaneError::Internal(anyhow::anyhow!("Failed to persist profile selection: {e}"))
        })?;
    tokio::fs::rename(&profile_tmp, &profile_path)
        .await
        .map_err(|e| {
            tracing::error!(
                target = ?target,
                src = %profile_tmp.display(),
                dst = %profile_path.display(),
                "switch_profile_admin: rename active_profile.tmp → active_profile failed: {e}"
            );
            ControlPlaneError::Internal(anyhow::anyhow!("Failed to persist profile selection: {e}"))
        })?;

    // Step 3: Flip the in-memory active_profile.
    let previous = {
        let mut guard = state.active_profile.write();
        let prev = *guard;
        *guard = target;
        prev
    };

    Ok(ProfileSwitchAdminResponse {
        previous,
        current: target,
    })
}
