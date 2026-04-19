//! Admin UDS router — control-plane endpoints served over the Unix socket.
//!
//! ## Security model
//!
//! No session middleware, no auth layer. The Unix socket's fs-permissions
//! (mode 0600) are the sole access-control gate. Only the owning user
//! can connect.
//!
//! ## Endpoint subset
//!
//! The admin router surfaces control-plane operations that CLI
//! subcommands (`mokumo-server diagnose`, `backup`, etc.) dispatch to
//! when the daemon is running:
//!
//! - `GET  /health`              — liveness probe
//! - `GET  /diagnostics`         — structured diagnostics snapshot
//! - `GET  /diagnostics/bundle`  — zip export
//!
//! Additional admin endpoints (bootstrap, user management) will land
//! as the CLI subcommands that consume them are implemented.

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};

use kikan::PlatformState;

/// Build the admin router for the Unix domain socket surface.
///
/// Takes `PlatformState` — the narrowest slice that covers all
/// control-plane pure fns needed by the admin CLI. No session layer,
/// no auth layer, no SPA fallback.
pub fn build_admin_uds_router(state: PlatformState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/diagnostics", get(diagnostics))
        .route("/diagnostics/bundle", get(diagnostics_bundle))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn diagnostics(
    State(state): State<PlatformState>,
) -> Result<Json<kikan_types::diagnostics::DiagnosticsResponse>, StatusCode> {
    kikan::control_plane::diagnostics::collect(&state)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("admin UDS diagnostics failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn diagnostics_bundle(
    State(state): State<PlatformState>,
) -> Result<impl IntoResponse, StatusCode> {
    let (bytes, filename) = kikan::control_plane::diagnostics::build_bundle(&state)
        .await
        .map_err(|e| {
            tracing::error!("admin UDS diagnostics bundle failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let headers = [
        (header::CONTENT_TYPE, "application/zip".to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        ),
    ];
    Ok((headers, bytes))
}
