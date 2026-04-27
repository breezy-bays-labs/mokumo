//! Legacy `/api/auth/{forgot-password,reset-password}` compat shim.
//!
//! Both routes preserve their pre-existing wire shapes (the shop SPA
//! still calls them through M0) but route through the kikan recovery-
//! session core. The legacy `forgot-password` body is `{ email }`, and
//! the legacy response keeps the original `{ message, recovery_file_path }`
//! payload for clients that surface the file path to operators. The
//! legacy `reset-password` body is `{ email, pin, new_password }`; the
//! email→session_id resolution happens internally via
//! [`kikan::control_plane::auth::find_session_id_by_email`] (O(n) scan
//! over `reset_pins`, bounded by `~PIN_EXPIRY × issuance_rate`).
//!
//! New code should target the canonical
//! `/api/platform/v1/auth/recover/{request,complete}` URLs, which expose
//! the opaque session id directly and skip the email reverse-lookup.

use axum::Json;
use axum::extract::State;
use kikan::auth::recovery_artifact::RecoveryArtifactLocation;
use kikan::auth::{SeaOrmUserRepo, UserRepository};
use kikan::control_plane::auth::{find_session_id_by_email, recover_complete, recover_request};
use kikan::{AppError, ControlPlaneError, ControlPlaneState, ProfileDb};
use kikan_types::auth::{ForgotPasswordRequest, ResetPasswordRequest};
use kikan_types::error::ErrorCode;

pub async fn forgot_password(
    State(deps): State<ControlPlaneState>,
    ProfileDb(db): ProfileDb,
    Json(req): Json<ForgotPasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !deps.recovery_limiter.check_and_record(&req.email) {
        // Anti-enumeration: indistinguishable from any other invalid
        // recovery state. The legacy SPA flow tolerates a 400 here.
        return Err(AppError::BadRequest(
            ErrorCode::ValidationError,
            "Invalid or expired recovery session".into(),
        ));
    }

    let writer = deps
        .recovery_writer
        .as_ref()
        .ok_or_else(|| AppError::InternalError("Recovery flow is not configured".into()))?
        .clone();

    let outcome = recover_request(&deps.platform, &db, &req.email, |email, pin| {
        writer(email, pin)
    })
    .await
    .map_err(|e| match e {
        ControlPlaneError::Validation { .. } => AppError::BadRequest(
            ErrorCode::ValidationError,
            "Invalid or expired recovery session".into(),
        ),
        other => AppError::from(other),
    })?;

    let recovery_file_path = match outcome.location {
        RecoveryArtifactLocation::File { path } => path.to_string_lossy().into_owned(),
        RecoveryArtifactLocation::External { description } => description,
        _ => String::new(),
    };

    Ok(Json(serde_json::json!({
        "message": "If an account with that email exists, a recovery file has been placed on the server.",
        "recovery_file_path": recovery_file_path
    })))
}

pub async fn reset_password(
    State(deps): State<ControlPlaneState>,
    ProfileDb(db): ProfileDb,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = SeaOrmUserRepo::new(db.clone());
    let user_id = match repo.find_by_email(&req.email).await {
        Ok(Some(user)) => user.id,
        Ok(None) => {
            return Err(AppError::BadRequest(
                ErrorCode::ValidationError,
                "Invalid or expired recovery session".into(),
            ));
        }
        Err(e) => {
            tracing::error!("DB error during reset-password lookup: {e}");
            return Err(AppError::InternalError("An internal error occurred".into()));
        }
    };

    let session_id = find_session_id_by_email(&deps.platform, &user_id).ok_or_else(|| {
        AppError::BadRequest(
            ErrorCode::ValidationError,
            "Invalid or expired recovery session".into(),
        )
    })?;

    recover_complete(&deps.platform, &db, &session_id, req.pin, req.new_password)
        .await
        .map_err(|e| match e {
            ControlPlaneError::Validation { .. } => AppError::BadRequest(
                ErrorCode::ValidationError,
                "Invalid or expired recovery session".into(),
            ),
            other => AppError::from(other),
        })?;

    Ok(Json(
        serde_json::json!({"message": "Password reset successfully"}),
    ))
}
