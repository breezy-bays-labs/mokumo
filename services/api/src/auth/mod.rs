pub mod backend;
pub mod recover;
pub mod reset;
pub mod user;

use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_login::AuthSession;
use mokumo_core::activity::ActivityAction;
use mokumo_core::user::RoleId;
use mokumo_core::user::traits::UserRepository;
use mokumo_db::user::repo::SeaOrmUserRepo;
use mokumo_types::auth::{
    LoginRequest, MeResponse, RegenerateRecoveryCodesRequest, SetupRequest, SetupResponse,
};
use mokumo_types::error::ErrorCode;
use mokumo_types::user::UserResponse;

use crate::SharedState;
use crate::error::AppError;
use crate::profile_db::ProfileDb;

use backend::{Backend, Credentials};

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

fn user_to_response(user: &mokumo_core::user::User) -> UserResponse {
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
    }
}

async fn login(
    State(state): State<SharedState>,
    mut auth_session: AuthSessionType,
    Json(req): Json<LoginRequest>,
) -> Result<Json<UserResponse>, AppError> {
    let repo = SeaOrmUserRepo::new(state.db_for(*state.active_profile.read()).clone());
    let creds = Credentials {
        email: req.email.clone(),
        password: req.password,
    };

    let user = match auth_session.authenticate(creds).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            log_failed_login(&repo, &req.email).await;
            return Err(AppError::Unauthorized(
                ErrorCode::InvalidCredentials,
                "Invalid email or password".into(),
            ));
        }
        Err(e) => {
            tracing::error!("Authentication error: {e}");
            return Err(AppError::InternalError("An internal error occurred".into()));
        }
    };

    if let Err(e) = auth_session.login(&user).await {
        tracing::error!("Session login error: {e}");
        return Err(AppError::InternalError("Failed to create session".into()));
    }

    let _ = repo
        .log_auth_activity(&user.user, ActivityAction::LoginSuccess)
        .await;

    Ok(Json(user_to_response(&user.user)))
}

async fn log_failed_login(repo: &SeaOrmUserRepo, email: &str) {
    if let Ok(Some(found)) = repo.find_by_email(email).await {
        let _ = repo
            .log_auth_activity(&found, ActivityAction::LoginFailed)
            .await;
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
    match mokumo_db::user::password::verify_password(req.password, password_hash).await {
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
    use mokumo_core::setup::SetupMode;
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
                mokumo_types::error::ErrorCode::Forbidden,
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
                mokumo_types::error::ErrorCode::Forbidden,
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
            mokumo_types::error::ErrorCode::Forbidden,
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
    user: &mokumo_core::user::User,
    auth_session: &mut AuthSessionType,
) {
    use mokumo_core::setup::SetupMode;
    let hash = match repo.find_by_id_with_hash(&user.id).await {
        Ok(Some((_, hash))) => hash,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("Auto-login after setup: failed to fetch user hash: {e}");
            return;
        }
    };
    let auth_user = user::AuthenticatedUser::new(user.clone(), hash, SetupMode::Production);
    if let Err(e) = auth_session.login(&auth_user).await {
        tracing::warn!("Auto-login after setup failed: {e}");
    }
}

/// Combined middleware: demo auto-login + login-required check.
///
/// In demo mode: if no user is authenticated, automatically log in the demo admin.
/// In all modes: reject the request with 401 if no user is authenticated after
/// the auto-login attempt.
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
    use mokumo_core::setup::SetupMode;

    // Boot guard: reject all protected routes while demo installation is incomplete.
    // Only active when the server is running in Demo profile — after setup switches
    // the active profile to Production, the flag is no longer relevant and the guard
    // is skipped entirely (the Production path always boots with demo_install_ok=true
    // but may transiently observe false if setup runs before the first profile write).
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
                let auth_user = user::AuthenticatedUser::new(user, hash, SetupMode::Demo);
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
