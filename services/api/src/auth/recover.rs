use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mokumo_db::user::repo::SeaOrmUserRepo;
use mokumo_types::auth::RecoverRequest;
use mokumo_types::error::ErrorCode;

use crate::SharedState;

use super::error_response;

pub async fn recover(
    State(state): State<SharedState>,
    Json(req): Json<RecoverRequest>,
) -> Response {
    // Intentionally returns 400 (not 429) so rate-limited responses are
    // indistinguishable from invalid-code responses (OWASP anti-enumeration).
    if !state.recovery_limiter.check_and_record(&req.email) {
        tracing::warn!(email = %req.email, "Recovery code rate limit exceeded");
        return error_response(
            StatusCode::BAD_REQUEST,
            ErrorCode::ValidationError,
            "Invalid or used recovery code",
        );
    }

    if req.new_password.chars().count() < 8 {
        return error_response(
            StatusCode::BAD_REQUEST,
            ErrorCode::ValidationError,
            "Password must be at least 8 characters",
        );
    }

    let repo = SeaOrmUserRepo::new(state.db.clone());

    match repo
        .verify_and_use_recovery_code(&req.email, &req.recovery_code, &req.new_password)
        .await
    {
        Ok(true) => {
            Json(serde_json::json!({"message": "Password reset successfully"})).into_response()
        }
        Ok(false) => error_response(
            StatusCode::BAD_REQUEST,
            ErrorCode::ValidationError,
            "Invalid or used recovery code",
        ),
        Err(e) => {
            tracing::error!("Recovery code verification failed: {e}");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalError,
                "An internal error occurred",
            )
        }
    }
}
