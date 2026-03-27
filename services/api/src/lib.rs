pub mod activity;
pub mod auth;
pub mod customer;
pub mod discovery;
pub mod error;
pub mod pagination;
pub mod server_info;
pub mod ws;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use axum_login::AuthManagerLayerBuilder;
use axum_login::login_required;
use mokumo_db::DatabaseConnection;
use rust_embed::Embed;
use time::Duration;
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;
use tower_sessions::Expiry;
use tower_sessions::SessionManagerLayer;
use tower_sessions::session_store::ExpiredDeletion;
use tower_sessions_sqlx_store::SqliteStore;

use auth::backend::Backend;
use mokumo_types::HealthResponse;

/// A pending file-drop password reset entry.
pub struct PendingReset {
    pub pin_hash: String,
    pub created_at: std::time::SystemTime,
}

/// Configuration for the Mokumo server.
///
/// Clone is required because Tauri's `setup()` moves it into an async task.
#[derive(Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub data_dir: PathBuf,
    pub recovery_dir: PathBuf,
}

pub struct AppState {
    pub db: DatabaseConnection,
    pub ws: Arc<ws::manager::ConnectionManager>,
    pub shutdown: CancellationToken,
    pub started_at: std::time::Instant,
    pub mdns_status: discovery::SharedMdnsStatus,
    pub local_ip: tokio::sync::watch::Receiver<Option<std::net::IpAddr>>,
    pub setup_completed: Arc<AtomicBool>,
    pub setup_token: Option<String>,
    /// In-memory store for file-drop password reset PINs. Maps email → PendingReset.
    pub reset_pins: Arc<dashmap::DashMap<String, PendingReset>>,
    /// Directory where recovery files are placed for file-drop password reset.
    pub recovery_dir: PathBuf,
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

/// Generate a random setup token (UUID v4).
pub fn generate_setup_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Shared init for session store + setup state. Used by both `build_app` and
/// `build_app_with_shutdown` to avoid duplicating the setup/session bootstrap.
async fn init_session_and_setup(
    db: &DatabaseConnection,
) -> (SqliteStore, Arc<AtomicBool>, Option<String>) {
    let is_complete = mokumo_db::is_setup_complete(db).await.unwrap_or(false);
    let setup_completed = Arc::new(AtomicBool::new(is_complete));
    let setup_token = if is_complete {
        None
    } else {
        Some(generate_setup_token())
    };

    let pool = db.get_sqlite_connection_pool().clone();
    let session_store = SqliteStore::new(pool);
    session_store
        .migrate()
        .await
        .expect("session store migration failed");

    (session_store, setup_completed, setup_token)
}

/// Build the Axum router with health check, SPA fallback, and tracing.
///
/// Test-only convenience wrapper. Does NOT spawn the background IP refresh
/// task — the local IP is computed once and never updated. Use
/// `build_app_with_shutdown` in production for graceful lifecycle control.
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
pub async fn build_app(config: &ServerConfig, db: DatabaseConnection) -> (Router, Option<String>) {
    let local_ip = local_ip_address::local_ip().ok();
    let (_, local_ip_rx) = tokio::sync::watch::channel(local_ip);

    let (session_store, setup_completed, setup_token) = init_session_and_setup(&db).await;

    let router = build_app_inner(
        config,
        db,
        CancellationToken::new(),
        discovery::MdnsStatus::shared(),
        local_ip_rx,
        session_store,
        setup_completed,
        setup_token.clone(),
    );
    (router, setup_token)
}

/// Build the Axum router with an explicit shutdown token.
///
/// The token is stored in `AppState` so handlers (e.g. WebSocket) can observe
/// shutdown and drain gracefully. Spawns background tasks for IP refresh and
/// expired session cleanup, both stopped by the shutdown token.
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
pub async fn build_app_with_shutdown(
    config: &ServerConfig,
    db: DatabaseConnection,
    shutdown: CancellationToken,
    mdns_status: discovery::SharedMdnsStatus,
) -> (Router, Option<String>) {
    let initial_ip = local_ip_address::local_ip().ok();
    let (local_ip_tx, local_ip_rx) = tokio::sync::watch::channel(initial_ip);

    // Background task: re-check local IP every 30s
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

    let (session_store, setup_completed, setup_token) = init_session_and_setup(&db).await;

    // Background task: delete expired sessions every 60s
    let deletion_store = session_store.clone();
    let deletion_token = shutdown.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = deletion_store.continuously_delete_expired(std::time::Duration::from_secs(60)) => {}
            _ = deletion_token.cancelled() => {}
        }
    });

    if let Some(token) = &setup_token {
        tracing::info!("Setup required — token: {token}");
    }

    let router = build_app_inner(
        config,
        db,
        shutdown,
        mdns_status,
        local_ip_rx,
        session_store,
        setup_completed,
        setup_token.clone(),
    );
    (router, setup_token)
}

#[allow(clippy::too_many_arguments)]
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
fn build_app_inner(
    config: &ServerConfig,
    db: DatabaseConnection,
    shutdown: CancellationToken,
    mdns_status: discovery::SharedMdnsStatus,
    local_ip: tokio::sync::watch::Receiver<Option<std::net::IpAddr>>,
    session_store: SqliteStore,
    setup_completed: Arc<AtomicBool>,
    setup_token: Option<String>,
) -> Router {
    // Session layer: SameSite=Lax, HttpOnly, no Secure for M0 (LAN HTTP)
    // Lax (not Strict) so bookmarks and mDNS links preserve the session.
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_http_only(true)
        .with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(Duration::hours(24)));

    // Auth layer
    let backend = Backend::new(db.clone());
    let auth_layer = AuthManagerLayerBuilder::new(backend, session_layer).build();

    let state: SharedState = Arc::new(AppState {
        db,
        ws: Arc::new(ws::manager::ConnectionManager::new(64)),
        shutdown,
        started_at: std::time::Instant::now(),
        mdns_status,
        local_ip,
        setup_completed,
        setup_token,
        reset_pins: Arc::new(dashmap::DashMap::new()),
        recovery_dir: config.recovery_dir.clone(),
    });

    // Background task: sweep expired reset PINs every 60s
    {
        let pins = state.reset_pins.clone();
        let token = state.shutdown.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                        let now = std::time::SystemTime::now();
                        pins.retain(|_, v| {
                            now.duration_since(v.created_at)
                                .unwrap_or(std::time::Duration::ZERO)
                                < std::time::Duration::from_secs(15 * 60)
                        });
                    }
                    _ = token.cancelled() => break,
                }
            }
        });
    }

    // Protected routes: require login
    let protected_routes = Router::new()
        .nest("/api/customers", customer::router())
        .nest("/api/activity", activity::router())
        .route("/ws", get(ws::ws_handler))
        .route_layer(login_required!(Backend));

    let mut router = Router::new()
        .route("/api/health", get(health))
        .route("/api/server-info", get(server_info::handler))
        .route("/api/setup-status", get(setup_status))
        .nest("/api/auth", auth::auth_router())
        .nest("/api/setup", auth::setup_router())
        .merge(protected_routes);

    #[cfg(debug_assertions)]
    {
        router = router
            .route("/api/debug/connections", get(ws::debug_connections))
            .route("/api/debug/broadcast", post(ws::debug_broadcast))
            .route("/api/debug/expire-pin", post(debug_expire_pin))
            .route("/api/debug/recovery-dir", get(debug_recovery_dir));
    }

    router
        .fallback(serve_spa)
        .layer(auth_layer)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Resolve the directory for password-reset recovery files.
///
/// Priority: MOKUMO_RECOVERY_DIR env var > user's Desktop > cwd.
pub fn resolve_recovery_dir() -> PathBuf {
    std::env::var("MOKUMO_RECOVERY_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(dirs::desktop_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(debug_assertions)]
async fn debug_recovery_dir(State(state): State<SharedState>) -> impl IntoResponse {
    Json(serde_json::json!({"path": state.recovery_dir.to_string_lossy()}))
}

#[cfg(debug_assertions)]
async fn debug_expire_pin(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let email = body["email"].as_str().unwrap_or_default();
    if let Some(mut entry) = state.reset_pins.get_mut(email) {
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(20 * 60);
        entry.created_at = past;
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
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

async fn setup_status(State(state): State<SharedState>) -> impl IntoResponse {
    let setup_complete = state
        .setup_completed
        .load(std::sync::atomic::Ordering::Relaxed);
    Json(serde_json::json!({ "setup_complete": setup_complete }))
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
            code: mokumo_types::error::ErrorCode::NotFound,
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
