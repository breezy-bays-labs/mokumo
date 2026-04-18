//! Pure-function admin-surface operations on the users vertical.
//!
//! Every fn here takes a `&ControlPlaneState` (+ a per-request
//! `&DatabaseConnection` when the operation runs against an arbitrary
//! profile DB) and returns `Result<_, ControlPlaneError>`. No transport
//! machinery — no `axum::*`, no `tower_sessions::*`, no `axum_login::*`.
//! The purity invariant is enforced by
//! `crates/kikan/tests/control_plane_purity.rs`.
//!
//! ## Call-site wiring
//!
//! - **HTTP adapter** (`kikan::platform::{auth,users}::*`) — Axum extractors
//!   resolve the caller via `axum_login::AuthSession` and the per-request
//!   DB via the `ProfileDb` extractor, then delegate here.
//! - **UDS adapter** (`kikan-admin-adapter`, PR-D) — same pattern against
//!   a Unix-socket listener; capability auth via fs-perms 0600.
//! - **In-process CLI** (`mokumo-server bootstrap`, `mokumo-server …`) —
//!   opens its own DB handle at startup and calls these fns directly.
//!
//! ## Session issuance stays in the adapter
//!
//! `verify_credentials` returns the authenticated user object on a
//! successful password match; it does NOT mint a session. Session
//! cookies and `AuthSession::login(&user)` stay in the HTTP adapter so
//! the `AuthenticatedUser` value can flow through transport-agnostic
//! paths (CLI dispatch, UDS) without a cookie jar.
//!
//! ## Legacy-conflict mapping
//!
//! User-admin mutations (soft-delete, role-update) may fail with
//! `DomainError::Conflict` from the last-admin guard in `UserService`.
//! The module-local `last_admin_conflict_to_control_plane` mapper routes
//! that specific conflict shape to
//! `ControlPlaneError::Conflict(ConflictKind::LastAdminProtected { message })`
//! so the wire tuple is preserved byte-for-byte. The mapper is NOT a
//! general helper — future control-plane modules (backup, profiles)
//! interpret `DomainError::Conflict` differently and write their own.

use mokumo_core::error::DomainError;
use sea_orm::DatabaseConnection;

use crate::auth::{
    AuthenticatedUser, Credentials, RoleId, SeaOrmUserRepo, User, UserId, UserService, password,
};
use crate::{ConflictKind, ControlPlaneError, ControlPlaneState, SetupMode};

/// Input for [`bootstrap_first_admin`].
#[derive(Debug, Clone)]
pub struct BootstrapInput {
    pub email: String,
    pub name: String,
    pub password: String,
}

/// Output of a successful bootstrap — the created admin user plus the
/// 10 plaintext recovery codes generated in the same transaction.
#[derive(Debug)]
pub struct BootstrapOutcome {
    pub user: User,
    pub recovery_codes: Vec<String>,
}

/// Create the first admin account on an empty user table, atomically
/// with the 10 initial recovery codes and an activity-log entry.
///
/// Idempotency: if any active admin already exists, returns
/// `ControlPlaneError::Conflict(ConflictKind::AlreadyBootstrapped)`. The
/// repo-level `bootstrap_admin_with_codes` enforces this atomically
/// inside the same transaction as the insert.
///
/// Runs against `production_db` unconditionally — demo-profile admins
/// are seeded, never bootstrapped.
pub async fn bootstrap_first_admin(
    state: &ControlPlaneState,
    input: BootstrapInput,
) -> Result<BootstrapOutcome, ControlPlaneError> {
    let repo = SeaOrmUserRepo::new(state.platform.production_db.clone());
    let (user, recovery_codes) = repo
        .bootstrap_admin_with_codes(&input.email, &input.name, &input.password)
        .await?;
    Ok(BootstrapOutcome {
        user,
        recovery_codes,
    })
}

/// Verify an email + password pair against the production users table.
///
/// Returns the `AuthenticatedUser` on a successful match. Session
/// issuance (cookie mint, `AuthSession::login`) stays in the HTTP
/// adapter — callers that authenticate over UDS or in-process do not
/// need cookies.
///
/// Never leaks whether the account exists: returns
/// `ControlPlaneError::PermissionDenied` both when the email is unknown
/// AND when the password is wrong, matching the existing
/// `AppError::Unauthorized(InvalidCredentials, ...)` wire semantics.
/// Inactive users are rejected the same way.
///
/// Does NOT check or record account-lockout state. Lockout escalation
/// (DB counters, `AccountLocked` response) stays in the HTTP adapter
/// so the CLI / UDS paths can decide independently whether they want
/// the same policy.
pub async fn verify_credentials(
    state: &ControlPlaneState,
    email: &str,
    password: String,
) -> Result<AuthenticatedUser, ControlPlaneError> {
    let repo = SeaOrmUserRepo::new(state.platform.production_db.clone());
    let lookup = repo
        .find_by_email_with_hash(email)
        .await
        .map_err(domain_error_to_control_plane)?;

    // Always run password verification — even when the email is not
    // found or the account is inactive — so the response time does not
    // leak whether an account exists. Pre-lift `Backend::authenticate`
    // returned early on the not-found path; this closes the timing
    // side-channel during the lift.
    let (user_opt, hash) = match lookup {
        Some((user, hash)) if user.is_active => (Some((user, hash.clone())), hash),
        Some((_, _)) => (None, dummy_hash().to_string()),
        None => (None, dummy_hash().to_string()),
    };

    let valid = password::verify_password(password, hash)
        .await
        .map_err(domain_error_to_control_plane)?;

    match (valid, user_opt) {
        (true, Some((user, hash))) => Ok(AuthenticatedUser::new(user, hash, SetupMode::Production)),
        _ => Err(ControlPlaneError::PermissionDenied),
    }
}

/// Reference argon2id PHC hash used on the unknown-email / inactive-user
/// paths of [`verify_credentials`] so `verify_password` always runs for
/// the same wall-clock shape. The value itself is meaningless — the
/// password "dummy" is never accepted because we discard the result of
/// the hash comparison when the user was missing or inactive.
fn dummy_hash() -> &'static str {
    use std::sync::OnceLock;
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        // Generated once per process via argon2 with default parameters.
        // Synchronous call is fine here: OnceLock ensures this runs at
        // most once, amortized across every verify_credentials call.
        password_auth::generate_hash("dummy-password-not-a-secret")
    })
}

/// Convenience: pass a `Credentials` struct directly (same semantics as
/// [`verify_credentials`]). Used by the HTTP `login` adapter which
/// already owns a `Credentials` value extracted from the request body.
pub async fn verify_credentials_struct(
    state: &ControlPlaneState,
    creds: Credentials,
) -> Result<AuthenticatedUser, ControlPlaneError> {
    verify_credentials(state, &creds.email, creds.password).await
}

/// Soft-delete a user. Admin-only operation.
///
/// Refuses to delete the last active admin — [`UserService`] enforces
/// that invariant and raises `DomainError::Conflict { message }`, which
/// this fn re-routes to
/// `ControlPlaneError::Conflict(ConflictKind::LastAdminProtected)`
/// preserving the original message verbatim on the wire.
pub async fn soft_delete_user(
    _state: &ControlPlaneState,
    db: &DatabaseConnection,
    target: UserId,
    caller: &AuthenticatedUser,
) -> Result<User, ControlPlaneError> {
    if caller.user.role_id != RoleId::ADMIN {
        return Err(ControlPlaneError::PermissionDenied);
    }
    UserService::new(SeaOrmUserRepo::new(db.clone()))
        .soft_delete_user(&target, caller.user.id)
        .await
        .map_err(last_admin_conflict_to_control_plane)
}

/// Update a user's role. Admin-only operation.
///
/// Refuses to demote the last active admin — same
/// `LastAdminProtected` routing as [`soft_delete_user`].
pub async fn update_user_role(
    _state: &ControlPlaneState,
    db: &DatabaseConnection,
    target: UserId,
    new_role: RoleId,
    caller: &AuthenticatedUser,
) -> Result<User, ControlPlaneError> {
    if caller.user.role_id != RoleId::ADMIN {
        return Err(ControlPlaneError::PermissionDenied);
    }
    UserService::new(SeaOrmUserRepo::new(db.clone()))
        .update_user_role(&target, new_role, caller.user.id)
        .await
        .map_err(last_admin_conflict_to_control_plane)
}

/// Regenerate the 10 recovery codes for a user, atomically with an
/// activity-log entry. The caller must re-supply their current password
/// — a stronger check than session-only because session cookies survive
/// credential rotation per the current AuthnBackend policy.
///
/// Rate-limiting is the adapter's job (the ControlPlaneState carries
/// the limiter; the HTTP handler calls `check_and_record` before
/// invoking this fn).
///
/// Returns the 10 new plaintext codes. The old batch is invalidated
/// atomically inside the repo method's transaction.
pub async fn regenerate_recovery_codes(
    _state: &ControlPlaneState,
    db: &DatabaseConnection,
    target: UserId,
    password_plaintext: String,
) -> Result<Vec<String>, ControlPlaneError> {
    let repo = SeaOrmUserRepo::new(db.clone());

    let (user, hash) = repo
        .find_by_id_with_hash(&target)
        .await
        .map_err(domain_error_to_control_plane)?
        .ok_or(ControlPlaneError::NotFound)?;

    // Inactive / soft-deleted accounts must not be able to rotate
    // recovery codes even if their stale session cookie survives.
    if !user.is_active {
        return Err(ControlPlaneError::PermissionDenied);
    }

    let valid = password::verify_password(password_plaintext, hash)
        .await
        .map_err(domain_error_to_control_plane)?;
    if !valid {
        return Err(ControlPlaneError::PermissionDenied);
    }

    repo.regenerate_recovery_codes(&target).await.map_err(|e| {
        // Tag the regen-step failure so `map_regenerate_error` can
        // restore the pre-lift 500 message "Failed to regenerate
        // recovery codes" on this arm. Other internal arms (hash
        // fetch, verify) flow through `domain_error_to_control_plane`
        // without the tag and render as generic "An internal error
        // occurred".
        match e {
            DomainError::Internal { message } => {
                ControlPlaneError::Internal(anyhow::anyhow!("regen_failed: {message}"))
            }
            other => domain_error_to_control_plane(other),
        }
    })
}

// --- legacy-shape mappers ---

/// Map a `DomainError` raised by the user-admin last-admin guard into
/// a wire-preserving `ControlPlaneError`. The `Conflict` arm routes
/// through `ConflictKind::LastAdminProtected { message }` so the
/// caller-supplied message (e.g. "Cannot delete the last admin
/// account. Assign another admin first.") rides through byte-for-byte.
fn last_admin_conflict_to_control_plane(err: DomainError) -> ControlPlaneError {
    match err {
        DomainError::Conflict { message } => {
            ControlPlaneError::Conflict(ConflictKind::LastAdminProtected { message })
        }
        other => domain_error_to_control_plane(other),
    }
}

/// Generic `DomainError` → `ControlPlaneError` for cases where the
/// domain conflict does NOT carry legacy last-admin semantics. Used by
/// password-verify and lookup paths where `Conflict` does not arise.
fn domain_error_to_control_plane(err: DomainError) -> ControlPlaneError {
    match err {
        DomainError::NotFound { .. } => ControlPlaneError::NotFound,
        DomainError::Conflict { message } => {
            // Unexpected: no known user-admin path raises this branch.
            // Route to Internal so the ADR-pinned `(ErrorCode, status)`
            // tuples stay accurate even if a caller passes this helper
            // a conflict from outside the last-admin guard.
            ControlPlaneError::Internal(anyhow::anyhow!("unmapped conflict: {message}"))
        }
        DomainError::Validation { details } => {
            // Deterministic field pick: sort by key so repeated
            // conversions with the same input pick the same field.
            let mut entries: Vec<_> = details.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let (field, message) = entries
                .into_iter()
                .next()
                .map(|(f, msgs)| (f, msgs.into_iter().next().unwrap_or_default()))
                .unwrap_or_else(|| ("request".into(), "validation failed".into()));
            ControlPlaneError::Validation { field, message }
        }
        DomainError::Internal { message } => ControlPlaneError::Internal(anyhow::anyhow!(message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_admin_conflict_routes_to_last_admin_protected() {
        let err = last_admin_conflict_to_control_plane(DomainError::Conflict {
            message: "Cannot delete the last admin account. Assign another admin first.".into(),
        });
        match err {
            ControlPlaneError::Conflict(ConflictKind::LastAdminProtected { message }) => {
                assert_eq!(
                    message,
                    "Cannot delete the last admin account. Assign another admin first."
                );
            }
            other => panic!("expected LastAdminProtected, got {other:?}"),
        }
    }

    #[test]
    fn domain_not_found_maps_to_not_found() {
        let err = domain_error_to_control_plane(DomainError::NotFound {
            entity: "user",
            id: "42".into(),
        });
        assert!(matches!(err, ControlPlaneError::NotFound));
    }

    #[test]
    fn domain_validation_picks_first_field_deterministically() {
        let mut details = std::collections::HashMap::new();
        details.insert("zebra".to_string(), vec!["must be positive".to_string()]);
        details.insert("apple".to_string(), vec!["required".to_string()]);
        let err = domain_error_to_control_plane(DomainError::Validation { details });
        match err {
            ControlPlaneError::Validation { field, message } => {
                assert_eq!(field, "apple", "alphabetical sort picks 'apple' first");
                assert_eq!(message, "required");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn domain_internal_preserves_message() {
        let err = domain_error_to_control_plane(DomainError::Internal {
            message: "db offline".into(),
        });
        match err {
            ControlPlaneError::Internal(e) => assert!(e.to_string().contains("db offline")),
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    #[test]
    fn generic_conflict_routes_to_internal_not_last_admin() {
        // The generic mapper must NOT invent a LastAdminProtected for
        // conflicts that flow through it. Caller modules with a specific
        // conflict meaning must use their own mapper (see
        // `last_admin_conflict_to_control_plane`).
        let err = domain_error_to_control_plane(DomainError::Conflict {
            message: "unexpected".into(),
        });
        match err {
            ControlPlaneError::Internal(_) => {}
            other => panic!("expected Internal, got {other:?}"),
        }
    }
}
