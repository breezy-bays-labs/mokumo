pub mod recover;
pub mod reset;

pub use kikan::auth::{AuthenticatedUser, Backend, Credentials, ProfileUserId};

use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_login::AuthSession;
use kikan::auth::{RoleId, SeaOrmUserRepo, UserId};
use kikan_types::auth::{
    LoginRequest, MeResponse, RegenerateRecoveryCodesRequest, SetupRequest, SetupResponse,
};
use kikan_types::error::ErrorCode;
use kikan_types::user::UserResponse;
use mokumo_core::activity::ActivityAction;

use crate::SharedState;
use crate::error::AppError;
use kikan::ProfileDb;

pub type AuthSessionType = AuthSession<Backend>;

pub fn auth_router() -> Router<SharedState> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/forgot-password", post(reset::forgot_password))
        .route("/reset-password", post(reset::reset_password))
        .route("/recover", post(recover::recover))
}

/// Separate router for /api/auth/me — must be behind the demo auto-login
/// middleware so that demo mode sessions are created before the auth check.
pub fn auth_me_router() -> Router<SharedState> {
    Router::new().route("/me", get(me))
}

pub fn setup_router() -> Router<SharedState> {
    Router::new().route("/", post(setup))
}

fn user_to_response(user: &kikan::auth::User) -> UserResponse {
    UserResponse {
        id: user.id.get(),
        email: user.email.clone(),
        name: user.name.clone(),
        role_name: match user.role_id {
            RoleId::ADMIN => "Admin".into(),
            RoleId::STAFF => "Staff".into(),
            RoleId::GUEST => "Guest".into(),
            _ => "Unknown".into(),
        },
        is_active: user.is_active,
        last_login_at: user.last_login_at.clone(),
        created_at: user.created_at.clone(),
        updated_at: user.updated_at.clone(),
        deleted_at: user.deleted_at.clone(),
    }
}

/// Login thresholds (LAN-mode policy per adr-kikan-deployment-modes).
/// In-memory limiter: 10 attempts / 15 min per email.
/// DB lockout: after 10 consecutive fails, lock for 15 min.
const LOGIN_LOCKOUT_THRESHOLD: i32 = 10;
const LOGIN_LOCKOUT_SECS: i64 = 15 * 60;

async fn login(
    State(state): State<SharedState>,
    mut auth_session: AuthSessionType,
    Json(req): Json<LoginRequest>,
) -> Result<Json<UserResponse>, AppError> {
    // Step 1: in-memory rate limit (fast, per-email).
    if !state.login_limiter.check_and_record(&req.email) {
        return Err(AppError::TooManyRequests(
            "Too many login attempts. Try again later.".into(),
        ));
    }

    // Login always authenticates against production_db (same as Backend::authenticate).
    let repo = SeaOrmUserRepo::new(state.production_db.clone());

    // Step 2: run authentication FIRST so argon2 cost is paid on every
    // request, regardless of whether the account is locked. Checking lockout
    // before argon2 leaks account state via response-time side-channel
    // (locked accounts return ~instantly while unlocked accounts wait on
    // password hashing). The lockout decision is applied after auth below.
    let creds = Credentials {
        email: req.email.clone(),
        password: req.password,
    };

    let auth_result = match auth_session.authenticate(creds).await {
        Ok(maybe_user) => maybe_user,
        Err(e) => {
            tracing::error!("Authentication error: {e}");
            return Err(AppError::InternalError("An internal error occurred".into()));
        }
    };

    // Step 3: fetch current lockout state. Timing of this query is uniform
    // whether the account is locked or not (indexed lookup by email).
    let lockout_state = match repo.find_lockout_state_by_email(&req.email).await {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("Failed to check lockout state: {e}");
            return Err(AppError::InternalError("An internal error occurred".into()));
        }
    };

    // Step 4: if the account is currently locked, reject regardless of auth
    // outcome. We do NOT create a session and do NOT clear the failed-attempt
    // counter — the lock must expire naturally or be cleared by an admin.
    if let Some((_, Some(ref locked_until))) = lockout_state
        && is_still_locked(locked_until)
    {
        return Err(AppError::AccountLocked(
            "Account locked due to too many failed login attempts. Try again later.".into(),
        ));
    }

    // Step 5: apply the auth result now that we know the account is not locked.
    let user = match auth_result {
        Some(user) => user,
        None => {
            return handle_failed_login(&repo, lockout_state.map(|(id, _)| id)).await;
        }
    };

    // Step 6: auth succeeded and account is not locked — clear failed-attempt
    // counter and create session. A clear failure here must abort the login so
    // we don't leave stale lockout state behind while still minting a session.
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

/// Return true if `locked_until` (ISO-8601 UTC string) is still in the future.
fn is_still_locked(locked_until: &str) -> bool {
    use chrono::{DateTime, Utc};
    match locked_until.parse::<DateTime<Utc>>() {
        Ok(expiry) => Utc::now() < expiry,
        Err(_) => false, // malformed timestamp — treat as expired
    }
}

/// Handle a failed authentication attempt.
///
/// If `user_id` is Some (the email matched a user), increment the failed-attempt
/// counter. When the counter reaches the lockout threshold, the account is locked
/// and HTTP 423 is returned. Otherwise HTTP 401 is returned. Audit logging
/// (LoginFailed / AccountLocked) is handled atomically inside
/// `record_failed_attempt` within the same DB transaction as the counter update.
///
/// If `user_id` is None (email not found), return 401 without revealing that the
/// account doesn't exist.
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
            // Return generic 401 — don't expose internal errors.
            Err(AppError::Unauthorized(
                ErrorCode::InvalidCredentials,
                "Invalid email or password".into(),
            ))
        }
    }
}

async fn logout(mut auth_session: AuthSessionType) -> Result<StatusCode, AppError> {
    match auth_session.logout().await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            tracing::error!("Logout error: {e}");
            Err(AppError::InternalError("Failed to destroy session".into()))
        }
    }
}

async fn me(
    State(state): State<SharedState>,
    auth_session: AuthSessionType,
    ProfileDb(db): ProfileDb,
) -> Result<Json<MeResponse>, AppError> {
    let user = auth_session.user.as_ref().ok_or_else(|| {
        AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into())
    })?;

    let setup_complete = state.is_setup_complete();
    let repo = SeaOrmUserRepo::new(db.clone());
    let recovery_codes_remaining = match repo.recovery_codes_remaining(&user.user.id).await {
        Ok(count) => count,
        Err(e) => {
            tracing::warn!(user_id = %user.user.id, "Failed to read recovery code count: {e}");
            0
        }
    };

    Ok(Json(MeResponse {
        user: user_to_response(&user.user),
        setup_complete,
        recovery_codes_remaining,
    }))
}

/// Regenerate recovery codes for the authenticated user.
///
/// Intentional: this does NOT invalidate the user's existing sessions.
/// Session invalidation on credential change is deferred to M1 (per CAO + Ada review).
pub async fn regenerate_recovery_codes(
    State(state): State<SharedState>,
    auth_session: AuthSessionType,
    ProfileDb(db): ProfileDb,
    Json(req): Json<RegenerateRecoveryCodesRequest>,
) -> Result<Json<SetupResponse>, AppError> {
    let user = auth_session
        .user
        .as_ref()
        .ok_or_else(|| AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into()))?
        .clone();

    // Rate limit check
    if !state
        .regen_limiter
        .check_and_record(&user.user.id.to_string())
    {
        return Err(AppError::TooManyRequests(
            "Too many regeneration attempts. Try again later.".into(),
        ));
    }

    let repo = SeaOrmUserRepo::new(db.clone());

    // Re-fetch password hash from DB (not session cache) per AuthnBackend ADR
    let password_hash = match repo.find_by_id_with_hash(&user.user.id).await {
        Ok(Some((_, hash))) => hash,
        Ok(None) => {
            return Err(AppError::InternalError("User not found".into()));
        }
        Err(e) => {
            tracing::error!("Failed to fetch user for regen: {e}");
            return Err(AppError::InternalError("An internal error occurred".into()));
        }
    };

    // Verify password
    match kikan::auth::password::verify_password(req.password, password_hash).await {
        Ok(true) => {}
        Ok(false) => {
            return Err(AppError::Unauthorized(
                ErrorCode::InvalidCredentials,
                "Invalid password".into(),
            ));
        }
        Err(e) => {
            tracing::error!("Password verification error: {e}");
            return Err(AppError::InternalError("An internal error occurred".into()));
        }
    }

    // Regenerate codes
    match repo.regenerate_recovery_codes(&user.user.id).await {
        Ok(recovery_codes) => Ok(Json(SetupResponse { recovery_codes })),
        Err(e) => {
            tracing::error!("Recovery code regeneration failed: {e}");
            Err(AppError::InternalError(
                "Failed to regenerate recovery codes".into(),
            ))
        }
    }
}

async fn setup(
    State(state): State<SharedState>,
    mut auth_session: AuthSessionType,
    Json(req): Json<SetupRequest>,
) -> Result<(StatusCode, Json<SetupResponse>), AppError> {
    validate_setup_request(&state, &req)?;

    let setup_guard = SetupAttemptGuard::acquire(&state)?;

    let repo = SeaOrmUserRepo::new(state.production_db.clone());
    let (user, recovery_codes) = match repo
        .create_admin_with_setup(
            &req.admin_email,
            &req.admin_name,
            &req.admin_password,
            &req.shop_name,
        )
        .await
    {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Setup failed: {e}");
            return Err(AppError::Domain(
                mokumo_core::error::DomainError::Conflict {
                    message: "Setup failed — an admin account may already exist".into(),
                },
            ));
        }
    };

    setup_guard.complete();

    // Persist active_profile = "production" and update in-memory so subsequent
    // requests (including the auto-login below) use the production database.
    use kikan::SetupMode;
    let profile_path = state.data_dir.join("active_profile");
    if let Err(e) = tokio::fs::write(&profile_path, "production").await {
        tracing::warn!("Failed to persist active_profile after setup: {e}");
    }
    *state.active_profile.write() = SetupMode::Production;

    // Clear the first-launch flag so that GET /api/setup-status returns is_first_launch: false
    // for the lifetime of this server process. The profile_switch handler does the same on a
    // successful switch, but setup may complete without going through a profile switch (e.g.
    // scripted onboarding or direct API use that bypasses the welcome screen).
    let _ =
        state
            .is_first_launch
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed);

    auto_login(&repo, &user, &mut auth_session).await;

    Ok((StatusCode::CREATED, Json(SetupResponse { recovery_codes })))
}

struct SetupAttemptGuard {
    state: SharedState,
    completed: bool,
}

impl SetupAttemptGuard {
    fn acquire(state: &SharedState) -> Result<Self, AppError> {
        if state.setup_completed.load(Ordering::Acquire) {
            return Err(AppError::Forbidden(
                kikan_types::error::ErrorCode::Forbidden,
                "Setup already completed".into(),
            ));
        }

        if state
            .setup_in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(AppError::Domain(
                mokumo_core::error::DomainError::Conflict {
                    message: "Setup is already in progress".into(),
                },
            ));
        }

        if state.setup_completed.load(Ordering::Acquire) {
            state.setup_in_progress.store(false, Ordering::Release);
            return Err(AppError::Forbidden(
                kikan_types::error::ErrorCode::Forbidden,
                "Setup already completed".into(),
            ));
        }

        Ok(Self {
            state: state.clone(),
            completed: false,
        })
    }

    fn complete(mut self) {
        self.state.setup_completed.store(true, Ordering::Release);
        self.state.setup_in_progress.store(false, Ordering::Release);
        self.completed = true;
    }
}

impl Drop for SetupAttemptGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.state.setup_in_progress.store(false, Ordering::Release);
        }
    }
}

fn validate_setup_request(state: &SharedState, req: &SetupRequest) -> Result<(), AppError> {
    if state.setup_completed.load(Ordering::Acquire) {
        return Err(AppError::Forbidden(
            kikan_types::error::ErrorCode::Forbidden,
            "Setup already completed".into(),
        ));
    }

    let valid_token = state
        .setup_token
        .as_ref()
        .is_some_and(|t| t == &req.setup_token);
    if !valid_token {
        return Err(AppError::Unauthorized(
            ErrorCode::InvalidToken,
            "Invalid setup token".into(),
        ));
    }

    if req.admin_email.is_empty()
        || req.admin_password.is_empty()
        || req.admin_name.is_empty()
        || req.shop_name.is_empty()
    {
        return Err(AppError::Domain(
            mokumo_core::error::DomainError::Validation {
                details: std::collections::HashMap::from([(
                    "form".into(),
                    vec!["All fields are required".into()],
                )]),
            },
        ));
    }

    Ok(())
}

async fn auto_login(
    repo: &SeaOrmUserRepo,
    user: &kikan::auth::User,
    auth_session: &mut AuthSessionType,
) {
    use kikan::SetupMode;
    let hash = match repo.find_by_id_with_hash(&user.id).await {
        Ok(Some((_, hash))) => hash,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("Auto-login after setup: failed to fetch user hash: {e}");
            return;
        }
    };
    let auth_user = AuthenticatedUser::new(user.clone(), hash, SetupMode::Production);
    if let Err(e) = auth_session.login(&auth_user).await {
        tracing::warn!("Auto-login after setup failed: {e}");
    }
}

/// Combined middleware: 423 boot guard + demo auto-login + login-required check.
///
/// Execution order (all modes):
/// 1. **Boot guard** — if `demo_install_ok` is false and the path is not
///    `DEMO_RESET_PATH`, return 423 `DemoSetupRequired`. This guard is only active
///    in Demo profile; Production always boots with `demo_install_ok=true`.
/// 2. **Demo auto-login** — in Demo mode, if no session exists, log in the demo admin.
/// 3. **Login-required check** — reject with 401 if still unauthenticated.
///
/// This replaces the separate `login_required!` + demo auto-login layers because
/// `login_required!` checks the user from the incoming request, which doesn't
/// reflect a session created by a preceding middleware in the same request cycle.
pub async fn require_auth_with_demo_auto_login(
    State(state): State<SharedState>,
    mut auth_session: AuthSessionType,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    use kikan::SetupMode;

    // Boot guard: reject all protected routes while demo installation is incomplete.
    // Only active in Demo profile — Production always boots with demo_install_ok=true
    // and the guard is skipped entirely when Production is active.
    // Exception: /api/demo/reset is the recovery mechanism — it must bypass the entire
    // auth chain (both the 423 guard and the demo auto-login) so it can be called even
    // when admin@demo.local is missing from the database.
    if *state.active_profile.read() == SetupMode::Demo
        && !state
            .demo_install_ok
            .load(std::sync::atomic::Ordering::Acquire)
    {
        if request.uri().path() == crate::DEMO_RESET_PATH {
            return next.run(request).await;
        }
        return AppError::DemoSetupRequired.into_response();
    }

    // Demo mode auto-login: create a session for the demo admin if not authenticated.
    // Uses find_by_email_with_hash to resolve user + hash in a single DB query
    // (avoids the 2-query path through auto_login → find_by_id_with_hash).
    if *state.active_profile.read() == SetupMode::Demo && auth_session.user.is_none() {
        let repo = SeaOrmUserRepo::new(state.demo_db.clone());
        match repo.find_by_email_with_hash("admin@demo.local").await {
            Ok(Some((user, hash))) => {
                let auth_user = AuthenticatedUser::new(user, hash, SetupMode::Demo);
                if let Err(e) = auth_session.login(&auth_user).await {
                    tracing::warn!("Demo auto-login session creation failed: {e}");
                }
            }
            Ok(None) => {
                tracing::warn!("Demo auto-login: admin@demo.local not found in database");
                return AppError::ServiceUnavailable(
                    "Demo admin account not found. The demo database may be corrupted — try resetting.".into(),
                ).into_response();
            }
            Err(e) => {
                tracing::error!("Demo auto-login: failed to look up admin: {e}");
                return AppError::InternalError("An internal error occurred".into())
                    .into_response();
            }
        }
    }

    // Login-required check: reject if still not authenticated
    if auth_session.user.is_none() {
        return AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into())
            .into_response();
    }

    next.run(request).await
}
