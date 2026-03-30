use axum::Json;
use axum::extract::State;
use mokumo_db::user::repo::SeaOrmUserRepo;
use mokumo_types::auth::RecoverRequest;
use mokumo_types::error::ErrorCode;

use crate::SharedState;
use crate::error::AppError;
use crate::profile_db::ProfileDb;

pub async fn recover(
    State(state): State<SharedState>,
    ProfileDb(db): ProfileDb,
    Json(req): Json<RecoverRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Intentionally returns 400 (not 429) so rate-limited responses are
    // indistinguishable from invalid-code responses (OWASP anti-enumeration).
    if !state.recovery_limiter.check_and_record(&req.email) {
        tracing::warn!(email = %req.email, "Recovery code rate limit exceeded");
        return Err(AppError::BadRequest(
            ErrorCode::ValidationError,
            "Invalid or used recovery code".into(),
        ));
    }

    if req.new_password.chars().count() < 8 {
        return Err(AppError::BadRequest(
            ErrorCode::ValidationError,
            "Password must be at least 8 characters".into(),
        ));
    }

    let repo = SeaOrmUserRepo::new(db.clone());

    match repo
        .verify_and_use_recovery_code(&req.email, &req.recovery_code, &req.new_password)
        .await
    {
        Ok(true) => Ok(Json(
            serde_json::json!({"message": "Password reset successfully"}),
        )),
        Ok(false) => Err(AppError::BadRequest(
            ErrorCode::ValidationError,
            "Invalid or used recovery code".into(),
        )),
        Err(e) => {
            tracing::error!("Recovery code verification failed: {e}");
            Err(AppError::InternalError("An internal error occurred".into()))
        }
    }
}
