//! `POST /api/platform/v1/auth/login` — credentialed login.
//!
//! Generic over the graft's profile kind `K`. The auth pool is sourced
//! via [`crate::PlatformState::auth_profile_kind_dir`] (snapshot of
//! `Graft::auth_profile_kind().to_string()` at boot); the handler never
//! names a vertical profile literal.
//!
//! Adapter responsibilities (the pure-fn layer cannot see):
//! - in-memory rate limit (`login_limiter.check_and_record`)
//! - DB-backed lockout state (read + record-failed-attempt + clear-on-success)
//! - session issuance (`AuthSession::login(&user)`)
//! - activity log (`LoginSuccess`)
//!
//! Credential verification + timing-side-channel mitigation (uniform argon2
//! cost on unknown-email / inactive paths) live in
//! [`crate::control_plane::users::verify_credentials_struct`]. The lockout
//! check runs *after* credential verification so the response time does
//! not vary based on whether the account is currently locked.

use axum::Json;
use axum::extract::State;
use axum_login::AuthSession;
use kikan_types::activity::ActivityAction;
use kikan_types::auth::LoginRequest;
use kikan_types::error::ErrorCode;
use kikan_types::user::UserResponse;

use crate::auth::{Backend, Credentials, SeaOrmUserRepo, UserId};
use crate::control_plane;
use crate::control_plane::users::ProfileKindBounds;
use crate::{AppError, ControlPlaneError, ControlPlaneState};

use super::user_to_response;

/// LAN-mode lockout policy. After `LOGIN_LOCKOUT_THRESHOLD` consecutive
/// failures the account is locked for `LOGIN_LOCKOUT_SECS` seconds.
const LOGIN_LOCKOUT_THRESHOLD: i32 = 10;
const LOGIN_LOCKOUT_SECS: i64 = 15 * 60;

pub async fn login<K: ProfileKindBounds>(
    State(deps): State<ControlPlaneState>,
    mut auth_session: AuthSession<Backend<K>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<UserResponse>, AppError> {
    if !deps.login_limiter.check_and_record(&req.email) {
        return Err(AppError::TooManyRequests(
            "Too many login attempts. Try again later.".into(),
        ));
    }

    let auth_kind = auth_session.backend.auth_kind();
    let auth_dir = deps.platform.auth_profile_kind_dir.as_str();
    let repo = SeaOrmUserRepo::new(
        deps.platform
            .db_for(auth_dir)
            .cloned()
            .expect("auth profile pool present in PlatformState"),
    );

    // Run authentication FIRST so argon2 cost is paid on every request,
    // regardless of whether the account is locked. Checking lockout
    // before argon2 leaks account state via response-time side-channel.
    let creds = Credentials {
        email: req.email.clone(),
        password: req.password,
    };
    let auth_result =
        match control_plane::users::verify_credentials_struct(&deps, creds, auth_kind).await {
            Ok(user) => Some(user),
            Err(ControlPlaneError::PermissionDenied) => None,
            Err(e) => {
                tracing::error!("Authentication error: {e}");
                return Err(AppError::InternalError("An internal error occurred".into()));
            }
        };

    let lockout_state = match repo.find_lockout_state_by_email(&req.email).await {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("Failed to check lockout state: {e}");
            return Err(AppError::InternalError("An internal error occurred".into()));
        }
    };

    if let Some((_, Some(ref locked_until))) = lockout_state
        && is_still_locked(locked_until)
    {
        return Err(AppError::AccountLocked(
            "Account locked due to too many failed login attempts. Try again later.".into(),
        ));
    }

    let Some(user) = auth_result else {
        return handle_failed_login(&repo, lockout_state.map(|(id, _)| id)).await;
    };

    if let Err(e) = repo.clear_failed_attempts(user.user.id).await {
        tracing::error!(user_id = %user.user.id, "Failed to clear lockout state: {e}");
        return Err(AppError::InternalError("Failed to finalize login".into()));
    }

    if let Err(e) = auth_session.login(&user).await {
        tracing::error!("Session login error: {e}");
        return Err(AppError::InternalError("Failed to create session".into()));
    }

    let _ = repo
        .log_auth_activity(&user.user, ActivityAction::LoginSuccess)
        .await;

    Ok(Json(user_to_response(&user.user)))
}

/// True iff `locked_until` (ISO-8601 UTC) is still in the future.
/// A malformed timestamp is treated as expired so a corrupt row can never
/// strand a real user.
fn is_still_locked(locked_until: &str) -> bool {
    use chrono::{DateTime, Utc};
    match locked_until.parse::<DateTime<Utc>>() {
        Ok(expiry) => Utc::now() < expiry,
        Err(_) => false,
    }
}

/// Handle a failed authentication attempt.
///
/// `Some(user_id)` increments the failed-attempt counter and locks the
/// account on threshold; `None` (unknown email) returns 401 without
/// revealing that the account does not exist. Audit logging is atomic
/// inside `record_failed_attempt`.
async fn handle_failed_login(
    repo: &SeaOrmUserRepo,
    user_id: Option<UserId>,
) -> Result<Json<UserResponse>, AppError> {
    let Some(uid) = user_id else {
        return Err(AppError::Unauthorized(
            ErrorCode::InvalidCredentials,
            "Invalid email or password".into(),
        ));
    };

    match repo
        .record_failed_attempt(uid, LOGIN_LOCKOUT_THRESHOLD, LOGIN_LOCKOUT_SECS)
        .await
    {
        Ok((_, Some(_))) => Err(AppError::AccountLocked(
            "Account locked due to too many failed login attempts. Try again later.".into(),
        )),
        Ok((_, None)) => Err(AppError::Unauthorized(
            ErrorCode::InvalidCredentials,
            "Invalid email or password".into(),
        )),
        Err(e) => {
            tracing::error!("Failed to record failed login attempt: {e}");
            Err(AppError::Unauthorized(
                ErrorCode::InvalidCredentials,
                "Invalid email or password".into(),
            ))
        }
    }
}
