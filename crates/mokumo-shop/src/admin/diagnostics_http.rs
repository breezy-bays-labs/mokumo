//! HTTP delegation for `GET /api/diagnostics`. The business logic lives
//! in [`crate::admin::diagnostics::collect`]; this module is the
//! Axum-flavored thin adapter.

use std::path::Path;

use axum::{Json, extract::State};
use kikan::{AppError, PlatformState};
use kikan_types::diagnostics::DiagnosticsResponse;

pub async fn handler(
    State(state): State<PlatformState>,
) -> Result<Json<DiagnosticsResponse>, AppError> {
    let diag = crate::admin::diagnostics::collect(&state).await?;
    Ok(Json(diag))
}

/// Re-export of the pure-fn disk warning helper so HTTP callers (e.g. the
/// vertical health-check handler) keep a stable import path.
pub fn compute_disk_warning(data_dir: &Path) -> bool {
    crate::admin::diagnostics::compute_disk_warning(data_dir)
}
