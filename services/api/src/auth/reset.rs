use std::time::{Duration, SystemTime};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mokumo_core::user::traits::UserRepository;
use mokumo_db::user::password;
use mokumo_db::user::repo::SeaOrmUserRepo;
use mokumo_types::auth::{ForgotPasswordRequest, ResetPasswordRequest};
use mokumo_types::error::ErrorCode;

use crate::{PendingReset, SharedState};

use super::error_response;

const PIN_EXPIRY: Duration = Duration::from_secs(15 * 60);

fn recovery_html(pin: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>Mokumo Password Reset</title></head>
<body style="font-family:sans-serif;text-align:center;padding:4rem">
<h1>Mokumo Password Reset</h1>
<p>Enter this PIN in the application to reset your password:</p>
<p style="font-size:3rem;letter-spacing:0.5rem;font-weight:bold">{pin}</p>
<p style="color:#888">This PIN expires in 15 minutes.</p>
</body>
</html>"#
    )
}

pub async fn forgot_password(
    State(state): State<SharedState>,
    Json(req): Json<ForgotPasswordRequest>,
) -> Response {
    let repo = SeaOrmUserRepo::new(state.db.clone());

    let user = repo.find_by_email(&req.email).await.ok().flatten();

    if let Some(_user) = user {
        let pin: String = {
            use rand::Rng;
            let mut rng = rand::rng();
            format!("{:06}", rng.random_range(0..1_000_000u32))
        };

        let pin_hash = match password::hash_password(pin.clone()).await {
            Ok(hash) => hash,
            Err(e) => {
                tracing::error!("PIN hash failed: {e}");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::InternalError,
                    "An internal error occurred",
                );
            }
        };

        state.reset_pins.insert(
            req.email.clone(),
            PendingReset {
                pin_hash,
                created_at: SystemTime::now(),
            },
        );

        let dir = &state.recovery_dir;
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::error!("Failed to create recovery dir {}: {e}", dir.display());
        }
        let file_path = dir.join("mokumo-recovery.html");
        if let Err(e) = std::fs::write(&file_path, recovery_html(&pin)) {
            tracing::error!("Failed to write recovery file: {e}");
        }
    }

    // Always return 200 to avoid leaking which emails exist
    Json(serde_json::json!({"message": "Recovery file placed"})).into_response()
}

pub async fn reset_password(
    State(state): State<SharedState>,
    Json(req): Json<ResetPasswordRequest>,
) -> Response {
    let entry = state.reset_pins.get(&req.email);
    let (pin_hash, created_at) = match entry {
        Some(ref e) => (e.pin_hash.clone(), e.created_at),
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ErrorCode::ValidationError,
                "No reset request found",
            );
        }
    };
    drop(entry);

    let elapsed = SystemTime::now()
        .duration_since(created_at)
        .unwrap_or(Duration::ZERO);
    if elapsed > PIN_EXPIRY {
        state.reset_pins.remove(&req.email);
        return error_response(
            StatusCode::BAD_REQUEST,
            ErrorCode::ValidationError,
            "PIN expired",
        );
    }

    let valid = match password::verify_password(req.pin.clone(), pin_hash).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("PIN verify failed: {e}");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalError,
                "An internal error occurred",
            );
        }
    };

    if !valid {
        return error_response(
            StatusCode::BAD_REQUEST,
            ErrorCode::ValidationError,
            "Invalid PIN",
        );
    }

    let repo = SeaOrmUserRepo::new(state.db.clone());
    let user = match repo.find_by_email(&req.email).await {
        Ok(Some(u)) => u,
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                ErrorCode::ValidationError,
                "No reset request found",
            );
        }
    };

    if let Err(e) = repo.update_password(&user.id, &req.new_password).await {
        tracing::error!("Failed to update password: {e}");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::InternalError,
            "Failed to update password",
        );
    }

    state.reset_pins.remove(&req.email);
    let file_path = state.recovery_dir.join("mokumo-recovery.html");
    let _ = std::fs::remove_file(file_path);

    Json(serde_json::json!({"message": "Password reset successfully"})).into_response()
}
