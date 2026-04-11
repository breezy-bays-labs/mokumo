use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use mokumo_types::error::ErrorCode;
use tokio::fs;

use crate::SharedState;
use crate::error::AppError;

/// GET /api/shop/logo — serve the current shop logo (public, no auth required).
///
/// Returns the raw image with an appropriate Content-Type header. The URL
/// includes a `?v={updated_at}` cache-buster which the `setup-status` handler
/// appends; the server returns `Cache-Control: no-cache` so browsers revalidate
/// on each page load while still caching across requests within a session.
pub async fn get_logo(State(state): State<SharedState>) -> Result<impl IntoResponse, AppError> {
    // 1. Read logo metadata from DB
    let (ext, updated_at) = mokumo_db::get_logo_info(&state.production_db)
        .await
        .map_err(|e| {
            tracing::error!("get_logo: failed to read logo info: {e}");
            AppError::InternalError("Failed to read logo info".into())
        })?
        .ok_or_else(|| {
            AppError::Domain(mokumo_core::error::DomainError::NotFound {
                entity: "shop_logo",
                id: "1".into(),
            })
        })?;

    // 2. Read file
    let path = state
        .data_dir
        .join("production")
        .join(format!("logo.{ext}"));

    let data = fs::read(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::Domain(mokumo_core::error::DomainError::NotFound {
                entity: "shop_logo",
                id: "1".into(),
            })
        } else {
            tracing::error!("get_logo: failed to read logo file {:?}: {e}", path);
            AppError::InternalError("Failed to read logo file".into())
        }
    })?;

    // 3. Content-Type from extension
    let content_type = match ext.as_str() {
        "png" => "image/png",
        "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        other => {
            tracing::error!("get_logo: unknown extension stored: {other}");
            return Err(AppError::UnprocessableEntity(
                ErrorCode::LogoMalformed,
                "Stored logo has an unknown format".into(),
            ));
        }
    };

    // 4. Build response headers
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static(content_type),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
    headers.insert(
        axum::http::header::ETAG,
        HeaderValue::from_str(&format!("\"{updated_at}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("\"\"")),
    );

    Ok((StatusCode::OK, headers, Bytes::from(data)).into_response())
}
