use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use axum::Json;
use axum::extract::State;
use kikan_types::auth::{ForgotPasswordRequest, ResetPasswordRequest};
use kikan_types::error::ErrorCode;

use super::PendingReset;
use kikan::auth::password;
use kikan::auth::{SeaOrmUserRepo, UserRepository};
use kikan::{AppError, ProfileDb};

use crate::state::SharedMokumoState;

const PIN_EXPIRY: Duration = Duration::from_secs(15 * 60);

fn hash_email_for_recovery_file(email: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in email.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn recovery_file_path_for_email(recovery_dir: &Path, email: &str) -> PathBuf {
    recovery_dir.join(format!(
        "mokumo-recovery-{}.html",
        hash_email_for_recovery_file(email)
    ))
}

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
    State(state): State<SharedMokumoState>,
    ProfileDb(db): ProfileDb,
    Json(req): Json<ForgotPasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = SeaOrmUserRepo::new(db.clone());
    let recovery_dir = state.recovery_dir();

    match repo.find_by_email(&req.email).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            // Return the same JSON shape as the known-email path to prevent enumeration.
            // This endpoint will be internet-accessible via Cloudflare Tunnel (M4).
            tracing::debug!(
                email_hash = %hash_email_for_recovery_file(&req.email),
                "forgot-password: no account found"
            );
            let dummy_path = recovery_file_path_for_email(recovery_dir, &req.email);
            return Ok(Json(serde_json::json!({
                "message": "If an account with that email exists, a recovery file has been placed on the server.",
                "recovery_file_path": dummy_path.to_string_lossy()
            })));
        }
        Err(e) => {
            tracing::error!("DB error during forgot-password lookup: {e}");
            return Err(AppError::InternalError("An internal error occurred".into()));
        }
    }

    let pin: String = {
        use rand::RngExt;
        let mut rng = rand::rng();
        format!("{:06}", rng.random_range(0..1_000_000u32))
    };

    let pin_hash = password::hash_password(pin.clone()).await.map_err(|e| {
        tracing::error!("PIN hash failed: {e}");
        AppError::InternalError("An internal error occurred".into())
    })?;

    if let Err(e) = tokio::fs::create_dir_all(recovery_dir).await {
        tracing::error!(
            "Failed to create recovery dir {}: {e}",
            recovery_dir.display()
        );
        return Err(AppError::InternalError("An internal error occurred".into()));
    }
    let file_path = recovery_file_path_for_email(recovery_dir, &req.email);
    if let Err(e) = tokio::fs::write(&file_path, recovery_html(&pin)).await {
        tracing::error!("Failed to write recovery file {}: {e}", file_path.display());
        return Err(AppError::InternalError("An internal error occurred".into()));
    }

    state.reset_pins().insert(
        req.email.clone(),
        PendingReset {
            pin_hash,
            created_at: SystemTime::now(),
        },
    );

    let path_str = file_path.to_string_lossy().into_owned();
    Ok(Json(serde_json::json!({
        "message": "If an account with that email exists, a recovery file has been placed on the server.",
        "recovery_file_path": path_str
    })))
}

pub async fn reset_password(
    State(state): State<SharedMokumoState>,
    ProfileDb(db): ProfileDb,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Atomic consume: `DashMap::remove` is the compare-and-swap primitive
    // for the PIN entry. Two concurrent `reset_password` requests with the
    // same email each call `remove`; exactly one wins and gets the entry,
    // the other gets `None` and fails with "No reset request found".
    //
    // This makes the reset PIN one-shot by construction — a wrong or
    // expired PIN consumes the entry just like a successful reset does.
    // Users who fumble their PIN (or whose PIN is stolen and attempted
    // first by an attacker) must request a new one via `forgot_password`.
    // The issuance rate-limiter, not the verifier, is what gates abuse.
    let reset_pins = state.reset_pins();
    let Some((_, entry)) = reset_pins.remove(&req.email) else {
        return Err(AppError::BadRequest(
            ErrorCode::ValidationError,
            "No reset request found".into(),
        ));
    };
    let PendingReset {
        pin_hash,
        created_at,
    } = entry;

    let elapsed = SystemTime::now()
        .duration_since(created_at)
        .unwrap_or(Duration::ZERO);
    if elapsed > PIN_EXPIRY {
        return Err(AppError::BadRequest(
            ErrorCode::ValidationError,
            "PIN expired".into(),
        ));
    }

    let valid = password::verify_password(req.pin.clone(), pin_hash)
        .await
        .map_err(|e| {
            tracing::error!("PIN verify failed: {e}");
            AppError::InternalError("An internal error occurred".into())
        })?;

    if !valid {
        return Err(AppError::BadRequest(
            ErrorCode::ValidationError,
            "Invalid PIN".into(),
        ));
    }

    let repo = SeaOrmUserRepo::new(db.clone());
    let user = match repo.find_by_email(&req.email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return Err(AppError::BadRequest(
                ErrorCode::ValidationError,
                "No reset request found".into(),
            ));
        }
        Err(e) => {
            tracing::error!("DB error during reset-password lookup: {e}");
            return Err(AppError::InternalError("An internal error occurred".into()));
        }
    };

    repo.update_password(&user.id, &req.new_password)
        .await
        .map_err(|e| {
            tracing::error!("Failed to update password: {e}");
            AppError::InternalError("Failed to update password".into())
        })?;

    let file_path = recovery_file_path_for_email(state.recovery_dir(), &req.email);
    if let Err(e) = std::fs::remove_file(&file_path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            path = %file_path.display(),
            "reset_password: failed to remove recovery file: {e}"
        );
    }

    Ok(Json(
        serde_json::json!({"message": "Password reset successfully"}),
    ))
}

#[cfg(test)]
mod tests {
    use super::recovery_file_path_for_email;
    use std::path::Path;

    #[test]
    fn recovery_file_path_is_stable_for_same_email() {
        let first = recovery_file_path_for_email(Path::new("/tmp"), "admin@shop.local");
        let second = recovery_file_path_for_email(Path::new("/tmp"), "admin@shop.local");
        assert_eq!(first, second);
    }

    #[test]
    fn recovery_file_path_differs_between_users() {
        let first = recovery_file_path_for_email(Path::new("/tmp"), "admin@shop.local");
        let second = recovery_file_path_for_email(Path::new("/tmp"), "staff@shop.local");
        assert_ne!(first, second);
    }
}
