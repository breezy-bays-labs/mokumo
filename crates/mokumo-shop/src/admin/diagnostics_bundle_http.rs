//! HTTP delegation for `GET /api/diagnostics/bundle`. The zip-build +
//! log-redaction logic lives in [`crate::admin::diagnostics::build_bundle`];
//! this module only adds the HTTP response headers.

use axum::{extract::State, http::header, response::IntoResponse};
use kikan::{AppError, PlatformState};

pub async fn handler(State(state): State<PlatformState>) -> Result<impl IntoResponse, AppError> {
    let (zip_bytes, filename) = crate::admin::diagnostics::build_bundle(&state).await?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        zip_bytes,
    ))
}
