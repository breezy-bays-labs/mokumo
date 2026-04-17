use axum::extract::State;
use axum::http::StatusCode;
use axum_login::AuthSession;
use kikan::SetupMode;
use kikan_types::error::ErrorCode;
use tokio::fs;

use crate::SharedState;
use crate::error::AppError;
use kikan::auth::Backend;

/// DELETE /api/shop/logo — remove the current shop logo.
///
/// Requires production profile. Commits the NULL to DB first, then
/// removes the file (a lingering orphan file is benign).
pub async fn delete_logo(
    auth_session: AuthSession<Backend>,
    State(state): State<SharedState>,
) -> Result<StatusCode, AppError> {
    // 1. Require production profile
    if *state.active_profile.read() != SetupMode::Production {
        return Err(AppError::Forbidden(
            ErrorCode::ShopLogoRequiresProductionProfile,
            "Logo management requires the production profile".into(),
        ));
    }

    // 2. Extract actor ID
    let actor_id = auth_session
        .user
        .as_ref()
        .ok_or_else(|| AppError::Unauthorized(ErrorCode::Unauthorized, "Not authenticated".into()))?
        .user
        .id
        .to_string();

    // 3. Check logo exists
    let (ext, _) = mokumo_db::get_logo_info(&state.production_db)
        .await
        .map_err(|e| {
            tracing::error!("delete_logo: failed to read logo info: {e}");
            AppError::InternalError("Failed to read logo info".into())
        })?
        .ok_or_else(|| {
            AppError::Domain(mokumo_core::error::DomainError::NotFound {
                entity: "shop_logo",
                id: "1".into(),
            })
        })?;

    // 4. Commit NULL to DB first (delete_logo → DB NULL committed)
    mokumo_db::shop::delete_logo(&state.production_db, &actor_id)
        .await
        .map_err(|e| {
            tracing::error!("delete_logo: failed to update db: {e}");
            AppError::InternalError("Failed to remove logo metadata".into())
        })?;

    // 5. Remove file — lingering dead file is benign
    let path = state
        .data_dir
        .join("production")
        .join(format!("logo.{ext}"));

    if let Err(e) = fs::remove_file(&path).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("delete_logo: failed to remove logo file {:?}: {e}", path);
    }

    Ok(StatusCode::NO_CONTENT)
}
