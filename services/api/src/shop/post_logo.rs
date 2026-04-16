use axum::extract::State;
use axum::http::StatusCode;
use axum_login::AuthSession;
use kikan::SetupMode;
use mokumo_core::shop::{LogoError, LogoValidator};
use mokumo_types::error::ErrorCode;
use std::time::SystemTime;
use tokio::fs;

use crate::SharedState;
use crate::auth::backend::Backend;
use crate::error::AppError;

/// POST /api/shop/logo — upload or replace the shop logo.
///
/// Only available on the production profile. Validates format (PNG/JPEG/WebP),
/// size (≤ 2 MiB), and dimensions (≤ 2048×2048) before writing to disk.
/// On extension change, sweeps the old logo file after renaming the new one.
pub async fn post_logo(
    auth_session: AuthSession<Backend>,
    State(state): State<SharedState>,
    mut multipart: axum::extract::Multipart,
) -> Result<StatusCode, AppError> {
    // 1. Require production profile
    if *state.active_profile.read() != SetupMode::Production {
        return Err(AppError::Forbidden(
            ErrorCode::ShopLogoRequiresProductionProfile,
            "Logo upload requires the production profile".into(),
        ));
    }

    // 2. Rate limit — keyed on authenticated user ID
    let actor_id = auth_session
        .user
        .as_ref()
        .ok_or_else(|| AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into()))?
        .user
        .id
        .to_string();

    if !state.logo_upload_limiter.check_and_record(&actor_id) {
        return Err(AppError::TooManyRequests(
            "Too many logo upload attempts. Try again later.".into(),
        ));
    }

    // 3. Read the "logo" field from multipart
    let bytes = loop {
        match multipart.next_field().await.map_err(|e| {
            tracing::warn!("multipart error: {e}");
            AppError::BadRequest(
                ErrorCode::MissingField,
                "Failed to read multipart data".into(),
            )
        })? {
            None => {
                return Err(AppError::BadRequest(
                    ErrorCode::MissingField,
                    "Required field 'logo' is missing".into(),
                ));
            }
            Some(field) if field.name() == Some("logo") => {
                break field.bytes().await.map_err(|e| {
                    tracing::warn!("failed to read logo field: {e}");
                    AppError::BadRequest(ErrorCode::MissingField, "Failed to read logo data".into())
                })?;
            }
            Some(_) => continue, // skip unrecognised fields
        }
    };

    // 4. Validate: format, size, dimensions
    let validated = LogoValidator::validate(bytes.to_vec()).map_err(|e| match e {
        LogoError::FormatUnsupported { .. } => AppError::UnprocessableEntity(
            ErrorCode::LogoFormatUnsupported,
            "Only PNG, JPEG, or WebP files are accepted.".into(),
        ),
        LogoError::TooLarge => AppError::UnprocessableEntity(
            ErrorCode::LogoTooLarge,
            "File is too large. Max 2 MB.".into(),
        ),
        LogoError::DimensionsExceeded => AppError::UnprocessableEntity(
            ErrorCode::LogoDimensionsExceeded,
            "Image is too large. Max 2048×2048 pixels.".into(),
        ),
        LogoError::Malformed => AppError::UnprocessableEntity(
            ErrorCode::LogoMalformed,
            "File unreadable. Try another image.".into(),
        ),
    })?;

    let new_ext = validated.format.to_string();
    let production_dir = state.data_dir.join("production");

    // 5. Read current extension before overwriting (for orphan sweep)
    let old_ext = mokumo_db::get_logo_info(&state.production_db)
        .await
        .map_err(|e| {
            tracing::error!("failed to read current logo info: {e}");
            AppError::InternalError("Failed to read current logo info".into())
        })?
        .map(|(ext, _)| ext);

    // 6. Write to .tmp then atomic rename
    let tmp_path = production_dir.join(format!("logo.{new_ext}.tmp"));
    let final_path = production_dir.join(format!("logo.{new_ext}"));

    if let Err(e) = fs::create_dir_all(&production_dir).await {
        tracing::error!("failed to create production dir: {e}");
        return Err(AppError::InternalError(
            "Failed to create logo directory".into(),
        ));
    }

    fs::write(&tmp_path, &validated.bytes).await.map_err(|e| {
        tracing::error!("failed to write logo tmp: {e}");
        AppError::InternalError("Failed to write logo file".into())
    })?;

    fs::rename(&tmp_path, &final_path).await.map_err(|e| {
        tracing::error!("failed to rename logo: {e}");
        AppError::InternalError("Failed to persist logo file".into())
    })?;

    // 7. Persist metadata to DB (before orphan sweep so file stays if DB write fails)
    let updated_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    mokumo_db::shop::upsert_logo(&state.production_db, &new_ext, updated_at, &actor_id)
        .await
        .map_err(|e| {
            tracing::error!("failed to upsert logo metadata: {e}");
            AppError::InternalError("Failed to save logo metadata".into())
        })?;

    // 8. Orphan sweep: delete old file if extension changed (after DB commit)
    if let Some(ref old) = old_ext
        && old != &new_ext
    {
        let old_path = production_dir.join(format!("logo.{old}"));
        if let Err(e) = fs::remove_file(&old_path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("failed to remove old logo file {:?}: {e}", old_path);
        }
    }

    Ok(StatusCode::NO_CONTENT)
}
