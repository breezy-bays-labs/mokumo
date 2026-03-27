pub mod activity;
pub mod customer;
pub mod discovery;
pub mod error;
pub mod pagination;
pub mod server_info;
pub mod ws;

use std::path::{Path, PathBuf};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use mokumo_db::DatabaseConnection;
use rust_embed::Embed;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
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
    pub db: DatabaseConnection,
    pub ws: Arc<ws::manager::ConnectionManager>,
    pub shutdown: CancellationToken,
    pub started_at: std::time::Instant,
    pub mdns_status: discovery::SharedMdnsStatus,
    pub local_ip: tokio::sync::watch::Receiver<Option<std::net::IpAddr>>,
}

pub type SharedState = Arc<AppState>;

#[derive(Embed)]
#[folder = "../../apps/web/build"]
struct SpaAssets;

/// Create the required data directories: data_dir and data_dir/logs/.
///
/// Returns an error with the path included in the message on failure.
pub fn ensure_data_dirs(data_dir: &Path) -> Result<(), std::io::Error> {
    for dir in [data_dir.to_path_buf(), data_dir.join("logs")] {
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
                let actual_port = listener.local_addr()?.port();
                tracing::info!("Listening on {host}:{actual_port}");
                return Ok((listener, actual_port));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                tracing::debug!("Port {p} in use, trying next");
            }
            Err(e) => {
                // Permission denied, address not available, etc. — these won't
                // resolve by trying the next port.
                return Err(std::io::Error::new(
                    e.kind(),
                    format!("Cannot bind to {host}:{p}: {e}"),
                ));
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        format!("Could not bind to any port in range {port}..={end_port} on host {host}"),
    ))
}

/// Build the Axum router with health check, SPA fallback, and tracing.
///
/// Test-only convenience wrapper. Does NOT spawn the background IP refresh
/// task — the local IP is computed once and never updated. Use
/// `build_app_with_shutdown` in production for graceful lifecycle control.
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
pub fn build_app(config: &ServerConfig, db: DatabaseConnection) -> Router {
    let local_ip = local_ip_address::local_ip().ok();
    let (_, local_ip_rx) = tokio::sync::watch::channel(local_ip);

    build_app_inner(
        config,
        db,
        CancellationToken::new(),
        discovery::MdnsStatus::shared(),
        local_ip_rx,
    )
}

/// Build the Axum router with an explicit shutdown token.
///
/// The token is stored in `AppState` so handlers (e.g. WebSocket) can observe
/// shutdown and drain gracefully. Spawns a background task that refreshes the
/// cached local IP every 30s, stopped by the shutdown token.
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
pub fn build_app_with_shutdown(
    config: &ServerConfig,
    db: DatabaseConnection,
    shutdown: CancellationToken,
    mdns_status: discovery::SharedMdnsStatus,
) -> Router {
    let initial_ip = local_ip_address::local_ip().ok();
    let (local_ip_tx, local_ip_rx) = tokio::sync::watch::channel(initial_ip);

    // Background task: re-check local IP every 30s so the settings UI
    // reflects network changes (Wi-Fi reconnect, VPN toggle, DHCP lease).
    let shutdown_token = shutdown.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await; // skip immediate first tick
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let current = local_ip_address::local_ip().ok();
                    local_ip_tx.send_if_modified(|prev| {
                        if *prev != current {
                            *prev = current;
                            true
                        } else {
                            false
                        }
                    });
                }
                _ = shutdown_token.cancelled() => break,
            }
        }
    });

    build_app_inner(config, db, shutdown, mdns_status, local_ip_rx)
}

#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
fn build_app_inner(
    config: &ServerConfig,
    db: DatabaseConnection,
    shutdown: CancellationToken,
    mdns_status: discovery::SharedMdnsStatus,
    local_ip: tokio::sync::watch::Receiver<Option<std::net::IpAddr>>,
) -> Router {
    let state: SharedState = Arc::new(AppState {
        db,
        ws: Arc::new(ws::manager::ConnectionManager::new(64)),
        shutdown,
        started_at: std::time::Instant::now(),
        mdns_status,
        local_ip,
    });

    let mut router = Router::new()
        .route("/api/health", get(health))
        .route("/api/server-info", get(server_info::handler))
        .nest("/api/customers", customer::router())
        .nest("/api/activity", activity::router())
        .route("/ws", get(ws::ws_handler));

    #[cfg(debug_assertions)]
    {
        router = router
            .route("/api/debug/connections", get(ws::debug_connections))
            .route("/api/debug/broadcast", post(ws::debug_broadcast));
    }

    router
        .fallback(serve_spa)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health(
    State(state): State<SharedState>,
) -> Result<
    (
        [(axum::http::HeaderName, &'static str); 1],
        Json<HealthResponse>,
    ),
    error::AppError,
> {
    mokumo_db::health_check(&state.db).await?;

    let uptime_seconds = state.started_at.elapsed().as_secs();

    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(HealthResponse {
            status: "ok".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            uptime_seconds,
            database: "ok".into(),
        }),
    ))
}

type SpaResponse = (StatusCode, [(axum::http::HeaderName, String); 2], Vec<u8>);

fn spa_response(status: StatusCode, content_type: &str, cache: &str, body: Vec<u8>) -> SpaResponse {
    (
        status,
        [
            (axum::http::header::CONTENT_TYPE, content_type.to_owned()),
            (axum::http::header::CACHE_CONTROL, cache.to_owned()),
        ],
        body,
    )
}

async fn serve_spa(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // Return a proper JSON 404 for unmatched API paths instead of serving the SPA shell
    if path.starts_with("api/") {
        let body = mokumo_types::error::ErrorBody {
            code: "not_found".into(),
            message: "No API route matches this path".into(),
            details: None,
        };
        let json = serde_json::to_vec(&body).unwrap_or_else(|e| {
            tracing::error!("Failed to serialize ErrorBody: {e}");
            br#"{"code":"internal_error","message":"An internal error occurred","details":null}"#
                .to_vec()
        });
        return spa_response(StatusCode::NOT_FOUND, "application/json", "no-store", json);
    }

    if let Some(file) = SpaAssets::get(path) {
        let cache = if path.contains("/_app/immutable/") {
            "public, max-age=31536000, immutable"
        } else {
            "public, max-age=3600"
        };
        spa_response(
            StatusCode::OK,
            file.metadata.mimetype(),
            cache,
            file.data.to_vec(),
        )
    } else if let Some(index) = SpaAssets::get("index.html") {
        spa_response(
            StatusCode::OK,
            index.metadata.mimetype(),
            "no-cache",
            index.data.to_vec(),
        )
    } else {
        tracing::warn!("SPA assets not found — run: moon run web:build");
        spa_response(
            StatusCode::NOT_FOUND,
            "text/plain",
            "no-store",
            b"SPA not built. Run: moon run web:build".to_vec(),
        )
    }
}
