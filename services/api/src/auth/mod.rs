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
use mokumo_types::error::{ErrorBody, ErrorCode};
use mokumo_types::user::UserResponse;

use crate::SharedState;

use backend::{Backend, Credentials};

pub type AuthSessionType = AuthSession<Backend>;

pub fn auth_router() -> Router<SharedState> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/forgot-password", post(reset::forgot_password))
        .route("/reset-password", post(reset::reset_password))
        .route("/recover", post(recover::recover))
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

fn error_response(status: StatusCode, code: ErrorCode, message: &str) -> Response {
    (
        status,
        Json(ErrorBody {
            code,
            message: message.into(),
            details: None,
        }),
    )
        .into_response()
}

async fn login(
    State(state): State<SharedState>,
    mut auth_session: AuthSessionType,
    Json(req): Json<LoginRequest>,
) -> Response {
    let repo = SeaOrmUserRepo::new(state.db.clone());
    let creds = Credentials {
        email: req.email.clone(),
        password: req.password,
    };

    let user = match auth_session.authenticate(creds).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            log_failed_login(&repo, &req.email).await;
            return error_response(
                StatusCode::UNAUTHORIZED,
                ErrorCode::InvalidCredentials,
                "Invalid email or password",
            );
        }
        Err(e) => {
            tracing::error!("Authentication error: {e}");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalError,
                "An internal error occurred",
            );
        }
    };

    if let Err(e) = auth_session.login(&user).await {
        tracing::error!("Session login error: {e}");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::InternalError,
            "Failed to create session",
        );
    }

    let _ = repo
        .log_auth_activity(&user.user, ActivityAction::LoginSuccess)
        .await;

    Json(user_to_response(&user.user)).into_response()
}

async fn log_failed_login(repo: &SeaOrmUserRepo, email: &str) {
    if let Ok(Some(found)) = repo.find_by_email(email).await {
        let _ = repo
            .log_auth_activity(&found, ActivityAction::LoginFailed)
            .await;
    }
}

async fn logout(mut auth_session: AuthSessionType) -> Response {
    match auth_session.logout().await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("Logout error: {e}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalError,
                "Failed to destroy session",
            )
        }
    }
}

async fn me(State(state): State<SharedState>, auth_session: AuthSessionType) -> Response {
    match auth_session.user {
        Some(ref user) => {
            let setup_complete = state.setup_completed.load(Ordering::Relaxed);
            let repo = SeaOrmUserRepo::new(state.db.clone());
            let recovery_codes_remaining = match repo.recovery_codes_remaining(&user.user.id).await
            {
                Ok(count) => count,
                Err(e) => {
                    tracing::warn!(user_id = %user.user.id, "Failed to read recovery code count: {e}");
                    0
                }
            };
            Json(MeResponse {
                user: user_to_response(&user.user),
                setup_complete,
                recovery_codes_remaining,
            })
            .into_response()
        }
        None => error_response(
            StatusCode::UNAUTHORIZED,
            ErrorCode::Unauthorized,
            "Not authenticated",
        ),
    }
}

pub async fn regenerate_recovery_codes(
    State(state): State<SharedState>,
    auth_session: AuthSessionType,
    Json(req): Json<RegenerateRecoveryCodesRequest>,
) -> Response {
    let user = match auth_session.user {
        Some(ref u) => u.clone(),
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                ErrorCode::Unauthorized,
                "Not authenticated",
            );
        }
    };

    // Rate limit check
    if !state
        .regen_limiter
        .check_and_record(&user.user.id.to_string())
    {
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::RateLimited,
            "Too many regeneration attempts. Try again later.",
        );
    }

    let repo = SeaOrmUserRepo::new(state.db.clone());

    // Re-fetch password hash from DB (not session cache) per AuthnBackend ADR
    let password_hash = match repo.find_by_id_with_hash(&user.user.id).await {
        Ok(Some((_, hash))) => hash,
        Ok(None) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalError,
                "User not found",
            );
        }
        Err(e) => {
            tracing::error!("Failed to fetch user for regen: {e}");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalError,
                "An internal error occurred",
            );
        }
    };

    // Verify password
    match mokumo_db::user::password::verify_password(req.password, password_hash).await {
        Ok(true) => {}
        Ok(false) => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                ErrorCode::InvalidCredentials,
                "Invalid password",
            );
        }
        Err(e) => {
            tracing::error!("Password verification error: {e}");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalError,
                "An internal error occurred",
            );
        }
    }

    // Regenerate codes
    match repo.regenerate_recovery_codes(&user.user.id).await {
        Ok(recovery_codes) => Json(SetupResponse { recovery_codes }).into_response(),
        Err(e) => {
            tracing::error!("Recovery code regeneration failed: {e}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalError,
                "Failed to regenerate recovery codes",
            )
        }
    }
}

async fn setup(
    State(state): State<SharedState>,
    mut auth_session: AuthSessionType,
    Json(req): Json<SetupRequest>,
) -> Response {
    if let Some(err) = validate_setup_request(&state, &req) {
        return err;
    }

    let setup_guard = match SetupAttemptGuard::acquire(&state) {
        Ok(guard) => guard,
        Err(err) => return *err,
    };

    let repo = SeaOrmUserRepo::new(state.db.clone());
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
            return error_response(
                StatusCode::CONFLICT,
                ErrorCode::SetupFailed,
                "Setup failed — an admin account may already exist",
            );
        }
    };

    setup_guard.complete();
    auto_login(&repo, &user, &mut auth_session).await;

    (StatusCode::CREATED, Json(SetupResponse { recovery_codes })).into_response()
}

struct SetupAttemptGuard {
    state: SharedState,
    completed: bool,
}

impl SetupAttemptGuard {
    fn acquire(state: &SharedState) -> Result<Self, Box<Response>> {
        if state.setup_completed.load(Ordering::Acquire) {
            return Err(Box::new(error_response(
                StatusCode::FORBIDDEN,
                ErrorCode::Forbidden,
                "Setup already completed",
            )));
        }

        if state
            .setup_in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Box::new(error_response(
                StatusCode::CONFLICT,
                ErrorCode::Conflict,
                "Setup is already in progress",
            )));
        }

        if state.setup_completed.load(Ordering::Acquire) {
            state.setup_in_progress.store(false, Ordering::Release);
            return Err(Box::new(error_response(
                StatusCode::FORBIDDEN,
                ErrorCode::Forbidden,
                "Setup already completed",
            )));
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

fn validate_setup_request(state: &SharedState, req: &SetupRequest) -> Option<Response> {
    if state.setup_completed.load(Ordering::Relaxed) {
        return Some(error_response(
            StatusCode::FORBIDDEN,
            ErrorCode::Forbidden,
            "Setup already completed",
        ));
    }

    let valid_token = state
        .setup_token
        .as_ref()
        .is_some_and(|t| t == &req.setup_token);
    if !valid_token {
        return Some(error_response(
            StatusCode::UNAUTHORIZED,
            ErrorCode::InvalidToken,
            "Invalid setup token",
        ));
    }

    if req.admin_email.is_empty()
        || req.admin_password.is_empty()
        || req.admin_name.is_empty()
        || req.shop_name.is_empty()
    {
        return Some(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ValidationError,
            "All fields are required",
        ));
    }

    None
}

async fn auto_login(
    repo: &SeaOrmUserRepo,
    user: &mokumo_core::user::User,
    auth_session: &mut AuthSessionType,
) {
    let hash = match repo.find_by_id_with_hash(&user.id).await {
        Ok(Some((_, hash))) => hash,
        _ => return,
    };
    let auth_user = user::AuthenticatedUser::new(user.clone(), hash);
    if let Err(e) = auth_session.login(&auth_user).await {
        tracing::warn!("Auto-login after setup failed: {e}");
    }
}
