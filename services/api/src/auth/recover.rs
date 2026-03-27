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
