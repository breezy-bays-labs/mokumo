//! POST /api/setup — first-admin setup wizard HTTP adapter (vertical layer).
//!
//! ## Split: vertical adapter vs. pure control-plane fn
//!
//! This module is the **HTTP adapter** for the setup wizard. It owns the
//! transport-coupled and shop-vertical concerns that cannot live in the
//! kikan pure-fn layer (invariant I1):
//!
//! - Accepting `shop_name` from the request body.
//! - Persisting `shop_name` to `shop_settings` (vertical persistence —
//!   kikan's pure-fn layer is shop-agnostic per I1).
//! - Mapping `ControlPlaneError` variants to the legacy wire shapes.
//! - Persisting `active_profile = "production"` to disk after setup.
//! - Flipping `active_profile` in memory.
//! - Clearing the `is_first_launch` flag.
//! - Auto-login of the newly created admin.
//!
//! The three transport-neutral steps (duplicate-setup guard, token
//! validation, admin user creation with recovery codes, `setup_completed`
//! flag) are delegated to `kikan::control_plane::users::setup_admin`.

use std::sync::atomic::Ordering;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum_login::AuthSession;
use kikan::auth::{AuthenticatedUser, Backend, SeaOrmUserRepo};
use kikan::{AppError, ControlPlaneError, ControlPlaneState, SetupMode};
use kikan_types::auth::SetupResponse;
use kikan_types::error::ErrorCode;
use serde::Deserialize;

/// Wire type for POST /api/setup.
///
/// Extends the kikan-agnostic admin fields (`admin_email`, `admin_name`,
/// `admin_password`, `setup_token`) with `shop_name` — shop-vertical data
/// that kikan must not own (invariant I1). This struct is defined in the
/// vertical adapter, not in `kikan-types`.
#[derive(Debug, Deserialize)]
struct VerticalSetupRequest {
    shop_name: String,
    admin_name: String,
    admin_email: String,
    admin_password: String,
    setup_token: String,
}

pub fn vertical_setup_router() -> Router<ControlPlaneState> {
    Router::new().route("/", post(vertical_setup))
}

async fn vertical_setup(
    State(deps): State<ControlPlaneState>,
    mut auth_session: AuthSession<Backend>,
    Json(req): Json<VerticalSetupRequest>,
) -> Result<(StatusCode, Json<SetupResponse>), AppError> {
    // Validate shop_name here — kikan's setup_admin fn does not receive it
    // (I1). If shop_name is missing the response matches the "empty fields"
    // validation shape the handler has always returned.
    if req.shop_name.is_empty() {
        return Err(AppError::Domain(
            mokumo_core::error::DomainError::Validation {
                details: std::collections::HashMap::from([(
                    "form".into(),
                    vec!["All fields are required".into()],
                )]),
            },
        ));
    }

    // Pure-fn: token + field validation, concurrent-attempt guard, user
    // creation, setup_completed flag.
    let outcome = kikan::control_plane::users::setup_admin(
        &deps,
        &req.admin_email,
        &req.admin_name,
        &req.admin_password,
        &req.setup_token,
    )
    .await
    .map_err(map_setup_error)?;

    // Persist shop_name to the shop_settings table (vertical concern).
    // Best-effort — log and continue if it fails; the admin user and
    // setup_completed flag are already committed.
    let pool = deps.platform.production_db.get_sqlite_connection_pool();
    if let Err(e) = sqlx::query(
        "INSERT INTO shop_settings (id, shop_name) VALUES (1, ?)
         ON CONFLICT(id) DO UPDATE SET shop_name = excluded.shop_name",
    )
    .bind(&req.shop_name)
    .execute(pool)
    .await
    {
        tracing::warn!("setup: failed to persist shop_name to shop_settings: {e}");
    }

    // Persist active_profile = "production" atomically (tmp-then-rename,
    // matching the pattern in `switch_profile`) and flip in-memory only on
    // success so a crash or write failure does not leave the process in
    // Production mode while the disk still says Demo.
    let profile_path = deps.platform.data_dir.join("active_profile");
    let profile_tmp = deps.platform.data_dir.join("active_profile.tmp");
    match async {
        tokio::fs::write(&profile_tmp, "production").await?;
        tokio::fs::rename(&profile_tmp, &profile_path).await
    }
    .await
    {
        Ok(()) => *deps.platform.active_profile.write() = SetupMode::Production,
        Err(e) => tracing::warn!("setup: failed to persist active_profile: {e}"),
    }

    // Clear the first-launch flag so GET /api/setup-status reflects
    // is_first_launch: false for the lifetime of this process.
    let _ = deps.platform.is_first_launch.compare_exchange(
        true,
        false,
        Ordering::AcqRel,
        Ordering::Relaxed,
    );

    // Auto-login: mint a session for the new admin so the browser is
    // immediately authenticated without a separate login round-trip.
    let repo = SeaOrmUserRepo::new(deps.platform.production_db.clone());
    match repo.find_by_id_with_hash(&outcome.user.id).await {
        Ok(Some((_, hash))) => {
            let auth_user =
                AuthenticatedUser::new(outcome.user.clone(), hash, SetupMode::Production);
            if let Err(e) = auth_session.login(&auth_user).await {
                tracing::warn!("setup: auto-login failed: {e}");
            }
        }
        Ok(None) => tracing::warn!("setup: auto-login skipped — user not found by id"),
        Err(e) => tracing::warn!("setup: auto-login skipped — hash lookup failed: {e}"),
    }

    Ok((
        StatusCode::CREATED,
        Json(SetupResponse {
            recovery_codes: outcome.recovery_codes,
        }),
    ))
}

/// Map `ControlPlaneError` from `setup_admin` to the legacy wire shapes.
/// Preserves the pre-I1 HTTP wire behavior byte-for-byte.
///
/// - `AlreadyBootstrapped` (Conflict) → 403 "Setup already completed"
/// - `PermissionDenied`               → 401 `invalid_token`
/// - `Validation`                     → 422 with form-level details
/// - `Internal`                       → 409 "Setup failed — may already exist"
fn map_setup_error(err: ControlPlaneError) -> AppError {
    match err {
        ControlPlaneError::Conflict(_) => {
            AppError::Forbidden(ErrorCode::Forbidden, "Setup already completed".into())
        }
        ControlPlaneError::PermissionDenied => {
            AppError::Unauthorized(ErrorCode::InvalidToken, "Invalid setup token".into())
        }
        ControlPlaneError::Validation { .. } => {
            AppError::Domain(mokumo_core::error::DomainError::Validation {
                details: std::collections::HashMap::from([(
                    "form".into(),
                    vec!["All fields are required".into()],
                )]),
            })
        }
        ControlPlaneError::Internal(e) => {
            tracing::error!("Setup failed: {e}");
            AppError::Domain(mokumo_core::error::DomainError::Conflict {
                message: "Setup failed — an admin account may already exist".into(),
            })
        }
        other => AppError::from(other),
    }
}
