use std::path::{Path, PathBuf};

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use rust_embed::Embed;
use sqlx::SqlitePool;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

use mokumo_types::HealthResponse;

/// Configuration for the Mokumo server.
///
/// Clone is required because Tauri's `setup()` moves it into an async task.
#[derive(Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub data_dir: PathBuf,
}

pub struct AppState {
    pub db: SqlitePool,
}

pub type SharedState = Arc<AppState>;

#[derive(Embed)]
#[folder = "../../apps/web/build"]
struct SpaAssets;

/// Create the required data directories: data_dir, data_dir/logs/, data_dir/backups/.
///
/// Returns an error with the path included in the message on failure.
pub fn ensure_data_dirs(data_dir: &Path) -> Result<(), std::io::Error> {
    for dir in [
        data_dir.to_path_buf(),
        data_dir.join("logs"),
        data_dir.join("backups"),
    ] {
        std::fs::create_dir_all(&dir).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("Failed to create directory {}: {}", dir.display(), e),
            )
        })?;
    }
    Ok(())
}

/// Attempt to bind a TCP listener, trying ports from `port` through `port + 10`.
///
/// Returns the listener and the actual port that was bound. Logs at INFO when
/// a port is successfully bound. Returns an error if all 11 ports are exhausted.
pub async fn try_bind(
    host: &str,
    port: u16,
) -> Result<(tokio::net::TcpListener, u16), std::io::Error> {
    let end_port = port.saturating_add(10);
    for p in port..=end_port {
        let addr = format!("{host}:{p}");
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("Listening on {addr}");
                return Ok((listener, p));
            }
            Err(e) => {
                tracing::debug!("Port {p} unavailable: {e}");
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        format!("Could not bind to any port in range {port}..={end_port} on host {host}"),
    ))
}

/// Build the Axum router with health check, SPA fallback, and tracing.
pub fn build_app(_config: &ServerConfig, pool: SqlitePool) -> Router {
    let state: SharedState = Arc::new(AppState { db: pool });

    Router::new()
        .route("/api/health", get(health))
        .fallback(serve_spa)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health(State(state): State<SharedState>) -> Result<Json<HealthResponse>, StatusCode> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("Health check DB query failed: {e}");
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    Ok(Json(HealthResponse {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }))
}

async fn serve_spa(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // Return a proper JSON 404 for unmatched API paths instead of serving the SPA shell
    if path.starts_with("api/") {
        return (
            StatusCode::NOT_FOUND,
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "application/json".to_owned(),
                ),
                (axum::http::header::CACHE_CONTROL, "no-store".to_owned()),
            ],
            r#"{"error":"not_found","message":"No API route matches this path"}"#
                .as_bytes()
                .to_vec(),
        );
    }

    if let Some(file) = SpaAssets::get(path) {
        let cache = if path.contains("/_app/immutable/") {
            "public, max-age=31536000, immutable"
        } else {
            "public, max-age=3600"
        };
        (
            StatusCode::OK,
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    file.metadata.mimetype().to_owned(),
                ),
                (axum::http::header::CACHE_CONTROL, cache.to_owned()),
            ],
            file.data.to_vec(),
        )
    } else if let Some(index) = SpaAssets::get("index.html") {
        (
            StatusCode::OK,
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    index.metadata.mimetype().to_owned(),
                ),
                (axum::http::header::CACHE_CONTROL, "no-cache".to_owned()),
            ],
            index.data.to_vec(),
        )
    } else {
        tracing::warn!("SPA assets not found — run: moon run web:build");
        (
            StatusCode::NOT_FOUND,
            [
                (axum::http::header::CONTENT_TYPE, "text/plain".to_owned()),
                (axum::http::header::CACHE_CONTROL, "no-store".to_owned()),
            ],
            b"SPA not built. Run: moon run web:build".to_vec(),
        )
    }
}
