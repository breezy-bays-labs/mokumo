//! Mokumo-vertical auth handlers that embed product policy and stay
//! shop-side of the kikan/application seam (ADR `adr-kikan-engine-vocabulary`):
//!
//! - `setup_router()` — `/api/setup` admin bootstrap.
//! - `regenerate_recovery_codes` — `/api/account/recovery-codes/regenerate`.
//! - `require_auth_with_demo_auto_login` — request-gating middleware that
//!   logs the demo admin in transparently in Demo profile.
//!
//! Login / logout / me are kikan-canonical (`kikan::platform::v1::auth`).
//! The legacy `/api/auth/{login,logout,me,forgot-password,reset-password}`
//! aliases are wired in [`crate::routes`] against the same kikan handlers
//! plus the [`super::reset`] compat shim.
//!
//! `ControlPlaneState` is consumed via `State<ControlPlaneState>` extractor;
//! [`require_auth_with_demo_auto_login`] takes `PlatformState`, wired via
//! `from_fn_with_state(state.platform_state(), …)`.

use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use axum_login::AuthSession;
use kikan::auth::SeaOrmUserRepo;
use kikan::control_plane;
use kikan::{AppError, ControlPlaneError, ControlPlaneState, PlatformState};
use kikan_types::SetupMode;
use kikan_types::auth::{RegenerateRecoveryCodesRequest, SetupRequest, SetupResponse};
use kikan_types::error::ErrorCode;

use crate::auth::{AuthenticatedUser, Backend};

/// Route path for the demo-reset handler. The auth-gate middleware allows
/// this path through even while the demo profile is mid-install, so shop
/// owners can always recover a broken demo database.
pub const DEMO_RESET_PATH: &str = "/api/demo/reset";

/// `axum-login`'s auth-session extractor, pinned to Mokumo's backend.
pub type AuthSessionType = AuthSession<Backend>;

/// Legacy `/api/auth/{forgot-password,reset-password}` sub-router.
/// Both handlers now route through the kikan recovery-session core and
/// consume only `ControlPlaneState`; the shop SPA's existing wire
/// contract is preserved unchanged through M0.
pub fn reset_router() -> Router<ControlPlaneState> {
    Router::new()
        .route("/forgot-password", post(super::reset::forgot_password))
        .route("/reset-password", post(super::reset::reset_password))
}

pub fn setup_router() -> Router<ControlPlaneState> {
    Router::new().route("/", post(setup))
}

/// Regenerate recovery codes for the authenticated user.
///
/// Intentional: this does NOT invalidate the user's existing sessions.
/// Session invalidation on credential change is deferred to M1 (per CAO + Ada review).
///
/// Adapter responsibilities: extract the caller from the session, run
/// the in-memory rate limiter, delegate the password-verify + regen
/// composite to the pure `control_plane::users::regenerate_recovery_codes`
/// fn, and map `ControlPlaneError` variants to the legacy wire shapes
/// (`PermissionDenied` → 401/`invalid_credentials`/"Invalid password";
/// `NotFound` → 500/"User not found"; `Internal` → 500/redacted).
pub async fn regenerate_recovery_codes(
    State(deps): State<ControlPlaneState>,
    auth_session: AuthSessionType,
    kikan::ProfileDb(db): kikan::ProfileDb,
    Json(req): Json<RegenerateRecoveryCodesRequest>,
) -> Result<Json<SetupResponse>, AppError> {
    let caller = auth_session
        .user
        .as_ref()
        .ok_or_else(|| AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into()))?
        .clone();

    if !deps
        .regen_limiter
        .check_and_record(&caller.user.id.to_string())
    {
        return Err(AppError::TooManyRequests(
            "Too many regeneration attempts. Try again later.".into(),
        ));
    }

    let recovery_codes =
        control_plane::users::regenerate_recovery_codes(&deps, &db, caller.user.id, req.password)
            .await
            .map_err(map_regenerate_error)?;

    Ok(Json(SetupResponse { recovery_codes }))
}

/// Map `ControlPlaneError` from the regen pure fn into the legacy wire
/// shapes. Preserves the pre-lift 401 "Invalid password" / 500 "User
/// not found" / 500 redacted-internal behavior, including the
/// regen-step-specific 500 message "Failed to regenerate recovery
/// codes" (distinguished via the `regen_failed:` anyhow tag set by
/// the pure fn — see `control_plane::users::regenerate_recovery_codes`).
fn map_regenerate_error(err: ControlPlaneError) -> AppError {
    match err {
        ControlPlaneError::PermissionDenied => {
            AppError::Unauthorized(ErrorCode::InvalidCredentials, "Invalid password".into())
        }
        ControlPlaneError::NotFound => AppError::InternalError("User not found".into()),
        ControlPlaneError::Internal(e) => {
            tracing::error!("Recovery code regeneration failed: {e:#}");
            if e.to_string().starts_with("regen_failed:") {
                AppError::InternalError("Failed to regenerate recovery codes".into())
            } else {
                AppError::InternalError("An internal error occurred".into())
            }
        }
        other => other.into(),
    }
}

async fn setup(
    State(deps): State<ControlPlaneState>,
    mut auth_session: AuthSessionType,
    Json(req): Json<SetupRequest>,
) -> Result<(StatusCode, Json<SetupResponse>), AppError> {
    let outcome = control_plane::users::setup_admin(
        &deps,
        &req.admin_email,
        &req.admin_name,
        &req.admin_password,
        &req.setup_token,
    )
    .await
    .map_err(map_setup_error)?;

    // Persist active_profile = "production" and update in-memory so subsequent
    // requests (including the auto-login below) use the production database.
    let profile_path = deps.platform.data_dir.join("active_profile");
    if let Err(e) = tokio::fs::write(&profile_path, "production").await {
        tracing::warn!("Failed to persist active_profile after setup: {e}");
    }
    *deps.platform.active_profile.write() =
        kikan::tenancy::ProfileDirName::from(SetupMode::Production.as_dir_name());

    // Clear the first-launch flag so that GET /api/setup-status returns is_first_launch: false
    // for the lifetime of this server process. The profile_switch handler does the same on a
    // successful switch, but setup may complete without going through a profile switch (e.g.
    // scripted onboarding or direct API use that bypasses the welcome screen).
    let _ = deps.platform.is_first_launch.compare_exchange(
        true,
        false,
        Ordering::AcqRel,
        Ordering::Relaxed,
    );

    let repo = SeaOrmUserRepo::new(
        deps.platform
            .db_for("production")
            .cloned()
            .expect("production profile pool present in PlatformState"),
    );
    auto_login(&repo, &outcome.user, &mut auth_session).await;

    Ok((
        StatusCode::CREATED,
        Json(SetupResponse {
            recovery_codes: outcome.recovery_codes,
        }),
    ))
}

/// Map `ControlPlaneError` from `setup_admin` to the legacy wire shapes used
/// by the kikan `setup` HTTP handler. Preserves the pre-lift behavior:
///
/// - `AlreadyBootstrapped` → 403 "Setup already completed"
/// - `PermissionDenied`    → 401 `invalid_token` "Invalid setup token"
/// - `Validation`          → 422 with `{ "form": ["All fields are required"] }`
/// - `Internal`            → 409 "Setup failed — an admin account may already exist"
///
/// The Internal→409 mapping preserves the original handler behavior where a
/// DB failure during `create_admin_with_setup` was treated as a likely conflict.
fn map_setup_error(err: ControlPlaneError) -> AppError {
    match err {
        ControlPlaneError::Conflict(_) => {
            AppError::Forbidden(ErrorCode::Forbidden, "Setup already completed".into())
        }
        ControlPlaneError::PermissionDenied => {
            AppError::Unauthorized(ErrorCode::InvalidToken, "Invalid setup token".into())
        }
        ControlPlaneError::Validation { .. } => {
            AppError::Domain(kikan::error::DomainError::Validation {
                details: std::collections::HashMap::from([(
                    "form".into(),
                    vec!["All fields are required".into()],
                )]),
            })
        }
        ControlPlaneError::Internal(e) => {
            tracing::error!("Setup failed: {e}");
            AppError::Domain(kikan::error::DomainError::Conflict {
                message: "Setup failed — an admin account may already exist".into(),
            })
        }
        other => AppError::from(other),
    }
}

async fn auto_login(
    repo: &SeaOrmUserRepo,
    user: &kikan::auth::User,
    auth_session: &mut AuthSessionType,
) {
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
///    [`DEMO_RESET_PATH`], return 423 `DemoSetupRequired`. This guard is only active
///    in Demo profile; Production always boots with `demo_install_ok=true`.
/// 2. **Demo auto-login** — in Demo mode, if no session exists, log in the demo admin.
/// 3. **Login-required check** — reject with 401 if still unauthenticated.
///
/// This replaces the separate `login_required!` + demo auto-login layers because
/// `login_required!` checks the user from the incoming request, which doesn't
/// reflect a session created by a preceding middleware in the same request cycle.
pub async fn require_auth_with_demo_auto_login(
    State(platform): State<PlatformState>,
    mut auth_session: AuthSessionType,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    // Boot guard: reject all protected routes while demo installation is incomplete.
    // Only active in Demo profile — Production always boots with demo_install_ok=true
    // and the guard is skipped entirely when Production is active.
    // Exception: /api/demo/reset is the recovery mechanism — it must bypass the entire
    // auth chain (both the 423 guard and the demo auto-login) so it can be called even
    // when admin@demo.local is missing from the database.
    if platform.active_profile.read().as_str() == "demo"
        && !platform.demo_install_ok.load(Ordering::Acquire)
    {
        if request.uri().path() == DEMO_RESET_PATH {
            return next.run(request).await;
        }
        return AppError::DemoSetupRequired.into_response();
    }

    // Demo mode auto-login: create a session for the demo admin if not authenticated.
    // Uses find_by_email_with_hash to resolve user + hash in a single DB query
    // (avoids the 2-query path through auto_login → find_by_id_with_hash).
    if platform.active_profile.read().as_str() == "demo" && auth_session.user.is_none() {
        let repo = SeaOrmUserRepo::new(
            platform
                .db_for("demo")
                .cloned()
                .expect("demo profile pool present in PlatformState"),
        );
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
