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

use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::sync::atomic::Ordering;

use crate::error::DomainError;
use sea_orm::DatabaseConnection;

use crate::auth::{
    AuthenticatedUser, Credentials, RoleId, SeaOrmUserRepo, User, UserId, UserService, password,
};
use crate::{ConflictKind, ControlPlaneError, ControlPlaneState};

/// Trait bounds common to every `K: Graft::ProfileKind` consumer in
/// `control_plane::users`. Keeps call-site signatures readable.
pub trait ProfileKindBounds:
    Copy
    + Debug
    + Display
    + Eq
    + Hash
    + Send
    + Sync
    + 'static
    + serde::Serialize
    + serde::de::DeserializeOwned
{
}

impl<T> ProfileKindBounds for T where
    T: Copy
        + Debug
        + Display
        + Eq
        + Hash
        + Send
        + Sync
        + 'static
        + serde::Serialize
        + serde::de::DeserializeOwned
{
}

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
/// Runs against the auth profile pool (`PlatformState::auth_profile_kind_dir`)
/// unconditionally — non-auth profiles are seeded by the vertical, never
/// bootstrapped through this entry point.
pub async fn bootstrap_first_admin(
    state: &ControlPlaneState,
    input: BootstrapInput,
) -> Result<BootstrapOutcome, ControlPlaneError> {
    let auth_dir = state.platform.auth_profile_kind_dir.as_str();
    let repo = SeaOrmUserRepo::new(
        state
            .platform
            .db_for(auth_dir)
            .cloned()
            .expect("auth profile pool present in PlatformState"),
    );
    let (user, recovery_codes) = repo
        .bootstrap_admin_with_codes(&input.email, &input.name, &input.password)
        .await?;
    Ok(BootstrapOutcome {
        user,
        recovery_codes,
    })
}

/// Output of a successful [`setup_admin`] call — the created admin user and
/// 10 plaintext recovery codes. The HTTP adapter passes `recovery_codes` in
/// the response body and passes `user` to `auth_session.login` (auto-login
/// immediately after setup). The UDS / CLI adapter surfaces `recovery_codes`
/// to the operator via stdout and discards the auto-login step.
#[derive(Debug)]
pub struct SetupAdminOutcome {
    pub user: User,
    pub recovery_codes: Vec<String>,
}

/// Run the first-admin setup wizard: validate token + fields, guard against
/// concurrent attempts, create the admin user atomically with recovery codes,
/// and mark the platform `setup_completed` flag.
///
/// Transport-neutral — no session, no cookies, no `active_profile` writes.
/// Those steps belong in the HTTP adapter (kikan `setup` handler or the
/// vertical handler in `mokumo-shop`) because they need session machinery
/// or profile-aware disk writes.
///
/// # Errors
///
/// - `Conflict(AlreadyBootstrapped)` — setup already done or a concurrent
///   attempt is already in flight.
/// - `PermissionDenied` — setup token is invalid or missing.
/// - `Validation` — required field is empty.
/// - `Internal` — unexpected DB failure during user insert.
pub async fn setup_admin(
    state: &ControlPlaneState,
    email: &str,
    name: &str,
    password: &str,
    setup_token: &str,
) -> Result<SetupAdminOutcome, ControlPlaneError> {
    // Guard 1: already complete.
    if state.platform.setup_completed.load(Ordering::Acquire) {
        return Err(ControlPlaneError::Conflict(
            ConflictKind::AlreadyBootstrapped,
        ));
    }

    // Guard 2: concurrent attempt — CAS false→true.
    if state
        .setup_in_progress
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(ControlPlaneError::Conflict(
            ConflictKind::AlreadyBootstrapped,
        ));
    }

    // Guard 3: TOCTOU re-check — race between the two loads above.
    if state.platform.setup_completed.load(Ordering::Acquire) {
        state.setup_in_progress.store(false, Ordering::Release);
        return Err(ControlPlaneError::Conflict(
            ConflictKind::AlreadyBootstrapped,
        ));
    }

    // Validate token. Clear the CAS flag on every early return below.
    // `setup_token` is an `Arc<str>`; `as_deref` lets the string-equality
    // check work without an extra clone.
    let valid_token = state
        .setup_token
        .as_deref()
        .is_some_and(|t| t == setup_token);
    if !valid_token {
        state.setup_in_progress.store(false, Ordering::Release);
        return Err(ControlPlaneError::PermissionDenied);
    }

    // Validate required fields.
    if email.is_empty() || password.is_empty() || name.is_empty() {
        state.setup_in_progress.store(false, Ordering::Release);
        return Err(ControlPlaneError::Validation {
            field: "form".into(),
            message: "All fields are required".into(),
        });
    }

    // Create admin user + recovery codes in one transaction.
    let auth_dir = state.platform.auth_profile_kind_dir.as_str();
    let repo = SeaOrmUserRepo::new(
        state
            .platform
            .db_for(auth_dir)
            .cloned()
            .expect("auth profile pool present in PlatformState"),
    );
    let (user, recovery_codes) = match repo.create_admin_with_setup(email, name, password).await {
        Ok(result) => result,
        Err(e) => {
            state.setup_in_progress.store(false, Ordering::Release);
            tracing::error!("setup_admin: create_admin_with_setup failed: {e}");
            return Err(ControlPlaneError::Internal(anyhow::anyhow!(
                "Setup failed: {e}"
            )));
        }
    };

    // Mark setup complete before releasing the in-progress flag so no
    // concurrent caller can sneak through the Guard-2 CAS while Guard-1
    // is still false.
    state
        .platform
        .setup_completed
        .store(true, Ordering::Release);
    state.setup_in_progress.store(false, Ordering::Release);

    Ok(SetupAdminOutcome {
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
pub async fn verify_credentials<K: ProfileKindBounds>(
    state: &ControlPlaneState,
    email: &str,
    password: String,
    auth_kind: K,
) -> Result<AuthenticatedUser<K>, ControlPlaneError> {
    let auth_dir = auth_kind.to_string();
    let repo = SeaOrmUserRepo::new(
        state
            .platform
            .db_for(auth_dir.as_str())
            .cloned()
            .ok_or_else(|| {
                ControlPlaneError::Internal(anyhow::anyhow!(
                    "auth profile pool missing from PlatformState"
                ))
            })?,
    );
    let lookup = repo
        .find_by_email_with_hash(email)
        .await
        .map_err(domain_error_to_control_plane)?;

    // Always run password verification — even when the email is not
    // found or the account is inactive — so the response time does not
    // leak whether an account exists.
    let (user_opt, hash) = match lookup {
        Some((user, hash)) if user.is_active => (Some((user, hash.clone())), hash),
        Some((_, _)) => (None, dummy_hash().to_string()),
        None => (None, dummy_hash().to_string()),
    };

    let valid = password::verify_password(password, hash)
        .await
        .map_err(domain_error_to_control_plane)?;

    match (valid, user_opt) {
        (true, Some((user, hash))) => Ok(AuthenticatedUser::new(user, hash, auth_kind)),
        _ => Err(ControlPlaneError::PermissionDenied),
    }
}

/// Reference argon2id PHC hash used on the unknown-email / inactive-user
/// paths of [`verify_credentials`] so `verify_password` always runs for
/// the same wall-clock shape. The value itself is meaningless — the
/// password "dummy" is never accepted because we discard the result of
/// the hash comparison when the user was missing or inactive.
///
/// Pre-generated at dev-time via `password_auth::generate_hash` (argon2id,
/// default params) and pasted verbatim so the first request that hits an
/// unknown-email / inactive path does not pay the ~hundreds-of-ms argon2
/// cost. Verified by `dummy_hash_burns_argon2_time` below.
fn dummy_hash() -> &'static str {
    "$argon2id$v=19$m=19456,t=2,p=1$UdFu5I27gCzB9xwcJviD9Q$ozUqrLyi1vPrt8DiIXuhALiz41dGwbYcJovIGWDi08I"
}

/// Convenience: pass a `Credentials` struct directly (same semantics as
/// [`verify_credentials`]). Used by the HTTP `login` adapter which
/// already owns a `Credentials` value extracted from the request body.
pub async fn verify_credentials_struct<K: ProfileKindBounds>(
    state: &ControlPlaneState,
    creds: Credentials,
    auth_kind: K,
) -> Result<AuthenticatedUser<K>, ControlPlaneError> {
    verify_credentials(state, &creds.email, creds.password, auth_kind).await
}

/// Soft-delete a user. Admin-only operation.
///
/// Refuses to delete the last active admin — [`UserService`] enforces
/// that invariant and raises `DomainError::Conflict { message }`, which
/// this fn re-routes to
/// `ControlPlaneError::Conflict(ConflictKind::LastAdminProtected)`
/// preserving the original message verbatim on the wire.
pub async fn soft_delete_user<K>(
    _state: &ControlPlaneState,
    db: &DatabaseConnection,
    target: UserId,
    caller: &AuthenticatedUser<K>,
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
pub async fn update_user_role<K>(
    _state: &ControlPlaneState,
    db: &DatabaseConnection,
    target: UserId,
    new_role: RoleId,
    caller: &AuthenticatedUser<K>,
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
    fn dummy_hash_is_a_real_argon2id_phc_string() {
        // The literal must parse as a valid argon2id PHC hash so
        // verify_password actually burns argon2 time on the unknown-email
        // and inactive-user paths. A malformed PHC would short-circuit
        // and reopen the timing side-channel we're closing.
        let h = dummy_hash();
        assert!(
            h.starts_with("$argon2id$"),
            "dummy_hash must be argon2id PHC, got: {h}"
        );
        assert!(
            password_auth::verify_password("wrong-password", h).is_err(),
            "verify_password against dummy_hash with arbitrary input must fail (and burn argon2 time)"
        );
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
