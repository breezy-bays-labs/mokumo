pub mod admin_uds;
pub mod error;
pub mod graft;

// Compatibility re-exports — `demo` and `discovery` are still referenced via
// the historic `mokumo_api::*` paths from the desktop shell / BDD world after
// the S4.1 platform lift (#507). `backup_status`, `diagnostics`, and
// `diagnostics_bundle` are reached through `kikan::platform::*` directly.
/// Re-export — moved to `kikan::logging` in PR 4b (#512).
pub use kikan::logging;
pub use kikan::platform::demo;
pub use kikan::platform::discovery;
pub mod pagination;
/// Re-export from mokumo-shop (moved in PR 3, Session 3.2).
pub use mokumo_shop::profile_switch;
pub mod restore;
/// Re-export from kikan (moved in PR 3, Session 3.1).
pub use kikan::middleware::security_headers;
pub mod server_info;
/// Re-export from mokumo-shop (moved in PR 3, Session 3.2).
pub use mokumo_shop::settings;
/// Re-export from mokumo-shop (moved in PR 3, Session 3.2).
pub use mokumo_shop::setup;
pub mod ws;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_login::AuthManagerLayerBuilder;
use kikan::SetupMode;
#[cfg(feature = "spa")]
use rust_embed::Embed;
use sea_orm::DatabaseConnection;
use time::Duration;
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;
use tower_sessions::Expiry;
use tower_sessions::SessionManagerLayer;
use tower_sessions::session_store::ExpiredDeletion;
use tower_sessions_sqlx_store::SqliteStore;

use kikan::auth::Backend;
use kikan::rate_limit;
use kikan_types::HealthResponse;

/// Path of the demo-reset endpoint. Re-exported from
/// `kikan::platform::auth::DEMO_RESET_PATH` for downstream callers that
/// still reference the historic `mokumo_api::DEMO_RESET_PATH` path.
pub use kikan::platform::auth::DEMO_RESET_PATH;

/// Re-export from `mokumo_shop::startup` — lifted in PR 4b (#512).
pub use mokumo_shop::startup::ProfileDbError;

pub use kikan::platform::auth::PendingReset;

/// Configuration for the Mokumo server.
///
/// Clone is required because Tauri's `setup()` moves it into an async task.
#[derive(Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub data_dir: PathBuf,
    pub recovery_dir: PathBuf,
    /// Hidden debug-only flag: WebSocket heartbeat interval in milliseconds.
    /// Only present in debug builds; absent in release to prevent leaking
    /// test-only behaviour into production.
    #[cfg(debug_assertions)]
    pub ws_ping_ms: Option<u64>,
}

/// Shared application state — handlers extract `State<SharedState>`.
///
/// After PR 2, `SharedState` is the composed `MokumoState` produced by
/// the Graft trait's `compose_state`. `MokumoAppState` has been removed;
/// handlers access fields via accessor methods on `MokumoState`.
pub type SharedState = mokumo_shop::state::SharedMokumoState;

pub use mokumo_shop::profile_db_init::MokumoProfileDbInitializer;

#[derive(Embed)]
#[folder = "../../apps/web/build"]
#[cfg(feature = "spa")]
pub struct SpaAssets;

/// Re-exports from `mokumo_shop::startup` — lifted in PR 4b (#512).
pub use mokumo_shop::startup::{
    ensure_data_dirs, generate_setup_token, init_session_and_setup, migrate_flat_layout,
    prepare_database, resolve_active_profile, resolve_demo_install_ok, try_bind,
};

/// Test-only convenience wrapper. Does NOT spawn the background IP refresh
/// task — the local IP is computed once and never updated. Use
/// `build_app_with_shutdown` in production for graceful lifecycle control.
///
/// Requires `feature = "spa"` because it attaches `.fallback(serve_spa)`.
#[cfg(feature = "spa")]
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
pub async fn build_app(
    config: &ServerConfig,
    demo_db: DatabaseConnection,
    production_db: DatabaseConnection,
    active_profile: SetupMode,
) -> Result<(Router, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
    let local_ip = Arc::new(parking_lot::RwLock::new(local_ip_address::local_ip().ok()));

    let session_db_path = config.data_dir.join("sessions.db");
    let (session_store, setup_completed, setup_token) =
        init_session_and_setup(&production_db, &session_db_path).await?;

    let demo_install_ok = resolve_demo_install_ok(&demo_db, active_profile).await;

    let (router, _ws, _state) = build_app_inner(
        config,
        demo_db,
        production_db,
        active_profile,
        CancellationToken::new(),
        kikan::MdnsStatus::shared(),
        local_ip,
        session_store,
        setup_completed,
        setup_token.clone(),
        demo_install_ok,
    );
    Ok((router.fallback(serve_spa), setup_token))
}

/// Build the Axum router with an explicit shutdown token.
///
/// The token is stored in the application state so handlers (e.g. WebSocket) can observe
/// shutdown and drain gracefully. Spawns background tasks for IP refresh and
/// expired session cleanup, both stopped by the shutdown token.
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
pub async fn build_app_with_shutdown(
    config: &ServerConfig,
    demo_db: DatabaseConnection,
    production_db: DatabaseConnection,
    active_profile: SetupMode,
    shutdown: CancellationToken,
    mdns_status: kikan::SharedMdnsStatus,
) -> Result<
    (
        Router,
        Option<String>,
        Arc<ws::manager::ConnectionManager>,
        SharedState,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let local_ip = Arc::new(parking_lot::RwLock::new(local_ip_address::local_ip().ok()));

    // Background task: re-check local IP every 30s
    let shutdown_token = shutdown.clone();
    let local_ip_task = local_ip.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await; // skip immediate first tick
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let current = local_ip_address::local_ip().ok();
                    let mut guard = local_ip_task.write();
                    if *guard != current {
                        *guard = current;
                    }
                }
                _ = shutdown_token.cancelled() => break,
            }
        }
    });

    let session_db_path = config.data_dir.join("sessions.db");
    let (session_store, setup_completed, setup_token) =
        init_session_and_setup(&production_db, &session_db_path).await?;

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

    let demo_install_ok = resolve_demo_install_ok(&demo_db, active_profile).await;

    let (router, ws, state) = build_app_inner(
        config,
        demo_db,
        production_db,
        active_profile,
        shutdown,
        mdns_status,
        local_ip,
        session_store,
        setup_completed,
        setup_token.clone(),
        demo_install_ok,
    );
    Ok((router, setup_token, ws, Arc::clone(&state)))
}

#[allow(clippy::too_many_arguments)]
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
fn build_app_inner(
    config: &ServerConfig,
    demo_db: DatabaseConnection,
    production_db: DatabaseConnection,
    active_profile: SetupMode,
    shutdown: CancellationToken,
    mdns_status: kikan::SharedMdnsStatus,
    local_ip: Arc<parking_lot::RwLock<Option<std::net::IpAddr>>>,
    session_store: SqliteStore,
    setup_completed: Arc<AtomicBool>,
    setup_token: Option<String>,
    demo_install_ok: Arc<AtomicBool>,
) -> (Router, Arc<ws::manager::ConnectionManager>, SharedState) {
    // Session layer: SameSite=Lax, HttpOnly, no Secure for M0 (LAN HTTP)
    // Lax (not Strict) so bookmarks and mDNS links preserve the session.
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_http_only(true)
        .with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(Duration::hours(24)));

    // Auth backend holds both databases; dispatches by compound user ID.
    let backend = Backend::new(demo_db.clone(), production_db.clone());
    let auth_layer = AuthManagerLayerBuilder::new(backend, session_layer).build();

    // Fresh install: active_profile file absent → first launch. Checked here (after
    // prepare_database has run migrate_flat_layout) so upgrades from flat layout are
    // not mistakenly treated as first launches.
    let first_launch = !config.data_dir.join("active_profile").exists();

    let ws_handle = Arc::new(ws::manager::ConnectionManager::new(64));

    let platform = kikan::PlatformState {
        data_dir: config.data_dir.clone(),
        demo_db,
        production_db,
        active_profile: Arc::new(parking_lot::RwLock::new(active_profile)),
        shutdown,
        started_at: std::time::Instant::now(),
        mdns_status,
        demo_install_ok,
        is_first_launch: Arc::new(AtomicBool::new(first_launch)),
        setup_completed,
        profile_db_initializer: Arc::new(MokumoProfileDbInitializer),
    };

    let control_plane = kikan::ControlPlaneState {
        platform,
        login_limiter: Arc::new(rate_limit::RateLimiter::new(10, rate_limit::DEFAULT_WINDOW)),
        recovery_limiter: Arc::new(rate_limit::RateLimiter::new(
            rate_limit::DEFAULT_MAX_ATTEMPTS,
            rate_limit::DEFAULT_WINDOW,
        )),
        regen_limiter: Arc::new(rate_limit::RateLimiter::new(
            3,
            std::time::Duration::from_secs(3600),
        )),
        switch_limiter: Arc::new(rate_limit::RateLimiter::new(3, rate_limit::DEFAULT_WINDOW)),
        reset_pins: Arc::new(dashmap::DashMap::new()),
        recovery_dir: config.recovery_dir.clone(),
        setup_token,
        setup_in_progress: Arc::new(AtomicBool::new(false)),
        activity_writer: Arc::new(kikan::SqliteActivityWriter::new()),
    };

    let domain = mokumo_shop::state::MokumoShopState {
        ws: ws_handle.clone(),
        local_ip,
        restore_in_progress: Arc::new(AtomicBool::new(false)),
        restore_limiter: Arc::new(rate_limit::RateLimiter::new(
            5,
            std::time::Duration::from_secs(3600),
        )),
        #[cfg(debug_assertions)]
        ws_ping_ms: config.ws_ping_ms,
    };

    let state: SharedState = Arc::new(mokumo_shop::state::MokumoState {
        control_plane,
        domain,
    });

    // Background task: sweep expired reset PINs every 60s
    {
        let pins = state.reset_pins().clone();
        let token = state.shutdown().clone();
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

    // Background task: run PRAGMA optimize every 2 hours and once on graceful shutdown.
    // Keeps SQLite's query-planner statistics fresh without blocking requests.
    {
        let demo_pool = state.demo_db().get_sqlite_connection_pool().clone();
        let prod_pool = state.production_db().get_sqlite_connection_pool().clone();
        let token = state.shutdown().clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2 * 3600));
            interval.tick().await; // skip immediate first tick (already ran at startup)
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        for pool in [&demo_pool, &prod_pool] {
                            if let Err(e) = sqlx::query("PRAGMA optimize(0xfffe)").execute(pool).await {
                                tracing::warn!("periodic PRAGMA optimize failed: {e}");
                            }
                        }
                    }
                    _ = token.cancelled() => {
                        for pool in [&demo_pool, &prod_pool] {
                            if let Err(e) = sqlx::query("PRAGMA optimize(0xfffe)").execute(pool).await {
                                tracing::warn!("shutdown PRAGMA optimize failed: {e}");
                            }
                        }
                        break;
                    }
                }
            }
        });
    }

    // Protected routes: require login (with demo auto-login support)
    //
    // Uses a combined middleware that handles both demo auto-login and auth checking
    // in a single layer. This is necessary because login_required! checks the user
    // from the incoming request, which doesn't reflect a session created by a
    // preceding middleware in the same request cycle.

    // Shop-logo router lives in the mokumo-shop vertical; the 3 MiB body
    // limit (1 MiB above the 2 MiB LogoValidator cap to absorb multipart
    // framing) is applied inside `shop_logo_protected_router()`.
    let shop_logo_deps = mokumo_shop::ShopLogoRouterDeps {
        activity_writer: state.activity_writer().clone(),
        production_db: state.production_db().clone(),
        data_dir: state.data_dir().clone(),
        logo_upload_limiter: Arc::new(kikan::rate_limit::RateLimiter::new(
            10,
            std::time::Duration::from_secs(60),
        )),
    };
    let shop_upload_router = Router::new().nest(
        "/api/shop",
        mokumo_shop::shop_logo_protected_router().with_state(shop_logo_deps.clone()),
    );

    let control_plane_state = state.control_plane_state();

    // Control-plane-stated sub-router for protected auth endpoints.
    // Handlers under `kikan::platform::auth` are thin delegations over
    // `kikan::control_plane::users::*`; both share `ControlPlaneState`.
    let protected_auth_routes = Router::new()
        .nest("/api/auth", kikan::platform::auth::auth_me_router())
        .route(
            "/api/account/recovery-codes/regenerate",
            post(kikan::platform::auth::regenerate_recovery_codes),
        )
        .with_state(control_plane_state.clone());

    let protected_routes = Router::new()
        .nest(
            "/api/customers",
            mokumo_shop::customer_router().with_state(mokumo_shop::CustomerRouterDeps {
                activity_writer: state.activity_writer().clone(),
            }),
        )
        .nest(
            "/api/users",
            kikan::platform::users::user_admin_router().with_state(control_plane_state.clone()),
        )
        .nest(
            "/api/activity",
            kikan::platform::activity_http::activity_router(),
        )
        .nest("/api/settings", settings::router())
        .route("/api/profile/switch", post(profile_switch::profile_switch))
        .route("/ws", get(ws::ws_handler))
        .merge(protected_auth_routes)
        .merge(shop_upload_router)
        // Lifted from `services/api` in S4.1 — protected platform routes
        // (`/api/demo/reset`, `/api/diagnostics`, `/api/diagnostics/bundle`)
        // are owned by `kikan::platform` and bound to `PlatformState` here.
        .merge(kikan::platform_protected_routes().with_state(state.platform_state()))
        .route_layer(axum::middleware::from_fn_with_state(
            state.platform_state(),
            kikan::platform::auth::require_auth_with_demo_auto_login,
        ));

    // Restore routes: unauthenticated, 500 MB body limit for file uploads.
    let restore_routes = Router::new()
        .route(
            "/api/shop/restore/validate",
            post(restore::validate_handler),
        )
        .route("/api/shop/restore", post(restore::restore_handler))
        .layer(axum::extract::DefaultBodyLimit::max(500 * 1024 * 1024));

    let mut router = Router::new()
        .route("/api/health", get(health))
        .route("/api/server-info", get(server_info::handler))
        .route("/api/setup-status", get(setup_status))
        // Lifted in S4.1 — `GET /api/backup-status` lives in
        // `kikan::platform::backup_status`.
        .merge(kikan::platform_public_routes().with_state(state.platform_state()))
        .nest(
            "/api/shop",
            mokumo_shop::shop_logo_public_router().with_state(shop_logo_deps),
        )
        .nest(
            "/api/auth",
            kikan::platform::auth::auth_router().with_state(control_plane_state.clone()),
        )
        .nest(
            "/api/setup",
            crate::setup::vertical_setup_router().with_state(control_plane_state.clone()),
        )
        .merge(restore_routes)
        .merge(protected_routes);

    #[cfg(debug_assertions)]
    {
        router = router
            .route("/api/debug/connections", get(ws::debug_connections))
            .route("/api/debug/broadcast", post(ws::debug_broadcast))
            .route("/api/debug/expire-pin", post(debug_expire_pin))
            .route("/api/debug/recovery-dir", get(debug_recovery_dir));
    }

    let app = router
        .method_not_allowed_fallback(handle_method_not_allowed)
        // ProfileDbMiddleware: innermost — runs after auth session is populated.
        // Injects ProfileDb into request extensions for all routes. Lives in
        // kikan; binds to the `PlatformState` slice, not the full `AppState`.
        .layer(axum::middleware::from_fn_with_state(
            state.platform_state(),
            kikan::profile_db::profile_db_middleware,
        ))
        .layer(auth_layer)
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(security_headers::middleware))
        .layer(kikan::middleware::host_allowlist::HostHeaderAllowList::loopback_only())
        .with_state(state.clone());
    (app, ws_handle, state)
}

/// Reset a user's password directly via SQLite (no server required).
///
/// This is the CLI support fallback — opens the database file directly,
/// hashes the new password with Argon2id, and updates the row.
/// Returns an error message on failure.
pub fn cli_reset_password(db_path: &Path, email: &str, new_password: &str) -> Result<(), String> {
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| format!("Cannot open database at {}: {e}", db_path.display()))?;

    let hash = password_auth::generate_hash(new_password);

    let rows = conn
        .execute(
            "UPDATE users SET password_hash = ?1 WHERE email = ?2 AND deleted_at IS NULL",
            rusqlite::params![hash, email],
        )
        .map_err(|e| format!("Failed to update password: {e}"))?;

    if rows == 0 {
        return Err(format!("No active user found with email '{email}'"));
    }

    Ok(())
}

// Binary-shell helpers (process lock, recovery dir, drain timeout) moved to
// `mokumo_shop::startup` in PR 4b (#512). Re-exports preserved for BDD tests
// until B3-bulk migrates them.
pub use mokumo_shop::startup::{
    DRAIN_TIMEOUT_SECS, format_lock_conflict_message, format_reset_db_conflict_message,
    lock_file_path, read_lock_info, resolve_recovery_dir, write_lock_info,
};

/// SQLite sidecar suffixes deleted alongside the main database file.
pub const DB_SIDECAR_SUFFIXES: &[&str] = &["", "-wal", "-shm", "-journal"];

/// Report from a database reset operation.
#[derive(Debug, Default)]
pub struct ResetReport {
    pub deleted: Vec<PathBuf>,
    pub not_found: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, std::io::Error)>,
    pub recovery_dir_error: Option<(PathBuf, std::io::Error)>,
    /// Non-fatal: backup directory could not be scanned (only set when `include_backups` is true).
    pub backup_dir_error: Option<(PathBuf, std::io::Error)>,
}

/// Fatal errors during database reset (not partial file failures).
#[derive(Debug, thiserror::Error)]
pub enum ResetError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Delete database files, sidecars, and optionally backups + recovery files.
///
/// `profile_dir` is the directory containing `mokumo.db` for the target profile
/// (e.g. `data_dir/demo` or `data_dir/production`). The caller resolves this
/// from the `--production` flag before calling.
///
/// If `profile_dir` does not exist, all database and backup entries will appear
/// in `report.not_found`; the function does not return `Err` in this case.
///
/// This is a pure filesystem function with no stdin/stdout interaction.
/// The caller (main.rs) handles confirmation prompts and result display.
pub fn cli_reset_db(
    profile_dir: &Path,
    recovery_dir: &Path,
    include_backups: bool,
) -> Result<ResetReport, ResetError> {
    let mut report = ResetReport::default();

    // 1. Database file + sidecars
    for suffix in DB_SIDECAR_SUFFIXES {
        let path = profile_dir.join(format!("mokumo.db{suffix}"));
        delete_file(&path, &mut report);
    }

    // 2. Backup files (opt-in)
    if include_backups {
        match std::fs::read_dir(profile_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if let Some(name_str) = name.to_str()
                        && name_str.starts_with("mokumo.db.backup-v")
                    {
                        delete_file(&entry.path(), &mut report);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // profile_dir doesn't exist — nothing to scan
            }
            Err(e) => {
                report.backup_dir_error = Some((profile_dir.to_path_buf(), e));
            }
        }
    }

    // 3. Recovery directory contents (only mokumo-recovery-*.html files)
    match std::fs::read_dir(recovery_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if let Some(name_str) = name.to_str()
                    && name_str.starts_with("mokumo-recovery-")
                    && name_str.ends_with(".html")
                {
                    delete_file(&entry.path(), &mut report);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            report.recovery_dir_error = Some((recovery_dir.to_path_buf(), e));
        }
    }

    Ok(report)
}

/// Create a manual backup of the database using the SQLite Online Backup API.
///
/// Resolves the output path: if `output` is provided, uses it directly; otherwise
/// generates a timestamped filename in the database's directory.
///
/// This is safe to run while the server is running — the Online Backup API
/// handles WAL mode and concurrent access correctly.
pub fn cli_backup(
    db_path: &Path,
    output: Option<&Path>,
) -> Result<kikan::backup::BackupResult, String> {
    let output_path = match output {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = db_path.parent().unwrap_or(Path::new("."));
            dir.join(kikan::backup::build_timestamped_name())
        }
    };

    let result = kikan::backup::create_backup(db_path, &output_path).map_err(|e| format!("{e}"))?;

    kikan::backup::verify_integrity(&output_path)
        .map_err(|e| format!("Backup created but integrity check failed: {e}"))?;

    // Bundle the shop logo as a sibling file alongside the backup DB.
    // Read from the backup file (output_path) to match the state we just captured.
    // Failure is non-fatal — log a warning and continue.
    let production_dir = db_path.parent().unwrap_or(Path::new("."));
    if let Ok(conn) = rusqlite::Connection::open(&output_path)
        && let Ok(ext) = conn.query_row(
            "SELECT logo_extension FROM shop_settings WHERE id = 1 AND logo_extension IS NOT NULL",
            [],
            |row| row.get::<_, String>(0),
        )
    {
        let logo_src = production_dir.join(format!("logo.{ext}"));
        let logo_dst = output_path.with_extension(format!("logo.{ext}"));
        if let Err(e) = std::fs::copy(&logo_src, &logo_dst) {
            tracing::warn!(
                "cli_backup: could not copy logo file {:?} → {:?}: {e}",
                logo_src,
                logo_dst
            );
        }
    }

    Ok(result)
}

/// Restore the database from a backup file.
///
/// Verifies the backup's integrity, creates a safety backup of the current
/// database, then overwrites it with the backup contents.
///
/// The caller must hold the process lock (server must not be running).
pub fn cli_restore(
    db_path: &Path,
    backup_path: &Path,
) -> Result<kikan::backup::RestoreResult, String> {
    let result = kikan::backup::restore_from_backup(db_path, backup_path, DB_SIDECAR_SUFFIXES)
        .map_err(|e| format!("{e}"))?;

    // Restore the shop logo from its sibling file, if present.
    // First sweep any stale logo.* files so a changed extension doesn't leave orphans.
    // Failure is non-fatal — log a warning and continue.
    let production_dir = db_path.parent().unwrap_or(Path::new("."));
    for candidate_ext in &["png", "jpeg", "webp"] {
        let stale = production_dir.join(format!("logo.{candidate_ext}"));
        if stale.exists()
            && let Err(e) = std::fs::remove_file(&stale)
        {
            tracing::warn!("cli_restore: could not remove stale logo {:?}: {e}", stale);
        }
    }
    if let Ok(conn) = rusqlite::Connection::open(backup_path)
        && let Ok(ext) = conn.query_row(
            "SELECT logo_extension FROM shop_settings WHERE id = 1 AND logo_extension IS NOT NULL",
            [],
            |row| row.get::<_, String>(0),
        )
    {
        let sibling = backup_path.with_extension(format!("logo.{ext}"));
        if sibling.exists() {
            let logo_dst = production_dir.join(format!("logo.{ext}"));
            if let Err(e) = std::fs::copy(&sibling, &logo_dst) {
                tracing::warn!(
                    "cli_restore: could not restore logo file {:?} → {:?}: {e}",
                    sibling,
                    logo_dst
                );
            }
        }
    }

    Ok(result)
}

/// A single migration record from `seaql_migrations`, with computed status.
#[derive(Debug)]
pub struct MigrationRecord {
    pub name: String,
    pub applied_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Output of `mokumo migrate status`.
#[derive(Debug)]
pub struct MigrateStatusReport {
    pub current_version: Option<String>,
    pub applied: Vec<MigrationRecord>,
    pub pending: Vec<String>,
    /// Migrations recorded in the DB but not known to this binary.
    /// Non-empty only on binary downgrade — the schema is ahead of the binary.
    pub unknown: Vec<String>,
}

/// Query the migration state of a database file.
///
/// Opens the database with a raw rusqlite connection (no pool, no migrations).
/// Returns the set of applied migrations (with timestamps) and pending migrations
/// (known to the binary but not recorded in `seaql_migrations`).
///
/// Returns an error string on any database or query failure.
pub fn cli_migrate_status(db_path: &Path) -> Result<MigrateStatusReport, String> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| format!("Cannot open database at {}: {e}", db_path.display()))?;

    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='seaql_migrations'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to query sqlite_master: {e}"))?;

    if !table_exists {
        let known = mokumo_shop::db::known_migration_names();
        return Ok(MigrateStatusReport {
            current_version: None,
            applied: vec![],
            pending: known,
            unknown: vec![],
        });
    }

    let mut stmt = conn
        .prepare("SELECT version, applied_at FROM seaql_migrations ORDER BY version")
        .map_err(|e| format!("Failed to prepare migration query: {e}"))?;

    let applied: Vec<MigrationRecord> = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let ts: i64 = row.get(1)?;
            Ok((name, ts))
        })
        .map_err(|e| format!("Failed to query seaql_migrations: {e}"))?
        .map(|r| {
            r.map(|(name, ts)| MigrationRecord {
                applied_at: chrono::DateTime::from_timestamp(ts, 0),
                name,
            })
        })
        .collect::<Result<_, _>>()
        .map_err(|e: rusqlite::Error| format!("Failed to read migration row: {e}"))?;

    let known = mokumo_shop::db::known_migration_names();
    let known_set: std::collections::HashSet<&str> = known.iter().map(|n| n.as_str()).collect();

    let unknown: Vec<String> = applied
        .iter()
        .filter(|r| !known_set.contains(r.name.as_str()))
        .map(|r| r.name.clone())
        .collect();

    let applied_names: std::collections::HashSet<&str> =
        applied.iter().map(|r| r.name.as_str()).collect();

    let pending: Vec<String> = known
        .into_iter()
        .filter(|n| !applied_names.contains(n.as_str()))
        .collect();

    let current_version = applied.last().map(|r| r.name.clone());

    Ok(MigrateStatusReport {
        current_version,
        applied,
        pending,
        unknown,
    })
}

fn delete_file(path: &Path, report: &mut ResetReport) {
    match std::fs::remove_file(path) {
        Ok(()) => report.deleted.push(path.to_path_buf()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            report.not_found.push(path.to_path_buf());
        }
        Err(e) => report.failed.push((path.to_path_buf(), e)),
    }
}

#[cfg(debug_assertions)]
async fn debug_recovery_dir(State(state): State<SharedState>) -> impl IntoResponse {
    Json(serde_json::json!({"path": state.recovery_dir().to_string_lossy()}))
}

#[cfg(debug_assertions)]
async fn debug_expire_pin(
    State(state): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let email = body["email"].as_str().unwrap_or_default();
    if let Some(mut entry) = state.reset_pins().get_mut(email) {
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
    // Check both profile databases — either being unhealthy makes the whole instance unhealthy
    kikan::db::health_check(state.db_for(SetupMode::Demo)).await?;
    kikan::db::health_check(state.db_for(SetupMode::Production)).await?;

    // Read the active profile once — both install_ok and db_path must agree on the
    // same profile snapshot to avoid a TOCTOU race with a concurrent profile switch.
    let active = *state.active_profile().read();

    // install_ok is only meaningful in Demo profile. In Production the flag is
    // permanently true (set at boot by resolve_demo_install_ok), but we re-derive
    // it from the active profile here so that a cold-start server which later runs
    // setup (switching from Demo→Production) reports install_ok=true immediately.
    let install_ok = if active == SetupMode::Production {
        true
    } else {
        state
            .demo_install_ok()
            .load(std::sync::atomic::Ordering::Acquire)
    };

    // storage_ok: disk pressure on the data directory's filesystem volume +
    // fragmentation check on the active profile database file.
    let db_path = state
        .data_dir()
        .join(active.as_dir_name())
        .join("mokumo.db");
    let disk_warning = kikan::platform::diagnostics::compute_disk_warning(state.data_dir());
    let diag_result =
        tokio::task::spawn_blocking(move || kikan::db::diagnose_database(&db_path)).await;
    let storage_ok = match diag_result {
        Ok(Ok(diag)) => !disk_warning && !diag.vacuum_needed(),
        Ok(Err(e)) => {
            tracing::warn!("diagnose_database failed in health handler: {e}");
            false
        }
        Err(e) => {
            tracing::warn!("spawn_blocking panicked in health handler: {e}");
            false
        }
    };

    let uptime_seconds = state.started_at().elapsed().as_secs();
    let status = if install_ok && storage_ok {
        "ok"
    } else {
        "degraded"
    };

    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(HealthResponse {
            status: status.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            uptime_seconds,
            database: "ok".into(),
            install_ok,
            storage_ok,
        }),
    ))
}

async fn setup_status(
    State(state): State<SharedState>,
) -> Result<Json<kikan_types::setup::SetupStatusResponse>, crate::error::AppError> {
    let active = *state.active_profile().read();
    let setup_complete = state.is_setup_complete();
    let is_first_launch = state
        .is_first_launch()
        .load(std::sync::atomic::Ordering::Acquire);

    let shop_name = mokumo_shop::db::get_shop_name(state.production_db())
        .await
        .map_err(|e| {
            tracing::error!("setup_status: failed to fetch shop_name: {e}");
            crate::error::AppError::InternalError("Failed to read shop configuration".into())
        })?;

    // Query production_db directly so this reflects the production setup state regardless of
    // which profile is currently active. Mirrors the shop_name pattern above.
    let production_setup_complete = mokumo_shop::db::is_setup_complete(state.production_db())
        .await
        .map_err(|e| {
            tracing::error!("setup_status: failed to fetch production_setup_complete: {e}");
            crate::error::AppError::InternalError("Failed to read production setup status".into())
        })?;

    let logo_info: Option<(Option<String>, Option<i64>)> =
        sqlx::query_as("SELECT logo_extension, logo_epoch FROM shop_settings WHERE id = 1")
            .fetch_optional(state.production_db().get_sqlite_connection_pool())
            .await
            .map_err(|e| {
                tracing::error!("setup_status: failed to fetch logo_info: {e}");
                crate::error::AppError::InternalError("Failed to read shop logo".into())
            })?;

    let logo_url = logo_info.and_then(|(ext, ts)| match (ext, ts) {
        (Some(_), Some(updated_at)) => Some(format!("/api/shop/logo?v={updated_at}")),
        _ => None,
    });

    Ok(Json(kikan_types::setup::SetupStatusResponse {
        setup_complete,
        setup_mode: setup_complete.then_some(active),
        is_first_launch,
        production_setup_complete,
        shop_name,
        logo_url,
    }))
}

#[cfg(feature = "spa")]
fn spa_response(status: StatusCode, content_type: &str, cache: &str, body: Vec<u8>) -> Response {
    (
        status,
        [
            (axum::http::header::CONTENT_TYPE, content_type.to_owned()),
            (axum::http::header::CACHE_CONTROL, cache.to_owned()),
        ],
        body,
    )
        .into_response()
}

async fn handle_method_not_allowed() -> Response {
    let body = kikan_types::error::ErrorBody {
        code: kikan_types::error::ErrorCode::MethodNotAllowed,
        message: "Method not allowed".into(),
        details: None,
    };
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(body),
    )
        .into_response()
}

/// SPA fallback: serve embedded static assets or index.html for client-side routing.
///
/// Public so binary crates (`services/api/src/main.rs`, `mokumo-desktop`) can
/// mount it as an Axum fallback. Headless binaries (`mokumo-server`) omit it.
///
/// Gated behind `feature = "spa"` so headless consumers that use
/// `default-features = false` don't pull in `rust-embed` or require
/// `apps/web/build/` to exist at compile time.
#[cfg(feature = "spa")]
pub async fn serve_spa(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Return a proper JSON 404 for unmatched API paths instead of serving the SPA shell
    if path == "api" || path.starts_with("api/") {
        let body = kikan_types::error::ErrorBody {
            code: kikan_types::error::ErrorCode::NotFound,
            message: "No API route matches this path".into(),
            details: None,
        };
        return (
            StatusCode::NOT_FOUND,
            [(axum::http::header::CACHE_CONTROL, "no-store")],
            Json(body),
        )
            .into_response();
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

#[cfg(test)]
mod tests {
    use super::*;
    use kikan_types::error::{ErrorBody, ErrorCode};

    #[cfg(feature = "spa")]
    #[tokio::test]
    async fn serve_spa_api_path_returns_not_found_code() {
        // All /api* paths that should return JSON 404 — including boundary cases
        for path in [
            "/api/nonexistent",
            "/api",
            "/api/",           // trailing slash
            "/api/v2/foo/bar", // deeply nested
        ] {
            let uri: axum::http::Uri = path.parse().unwrap();
            let response = serve_spa(uri).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "path: {path}");
            let ct = response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap();
            assert!(
                ct.to_str().unwrap().contains("application/json"),
                "path: {path} should return JSON, got: {ct:?}"
            );
            let cc = response
                .headers()
                .get(axum::http::header::CACHE_CONTROL)
                .unwrap();
            assert_eq!(cc.to_str().unwrap(), "no-store", "path: {path}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
            assert_eq!(error_body.code, ErrorCode::NotFound, "path: {path}");
        }
    }

    #[cfg(feature = "spa")]
    #[tokio::test]
    async fn serve_spa_prefix_collision_not_caught_by_api_guard() {
        // Paths that look like /api but are not — must NOT match the API prefix guard.
        // Without SPA assets embedded these fall through to "SPA not built" (text/plain),
        // not the JSON 404 returned for actual /api/* paths.
        for path in ["/api-docs", "/apiary", "/application"] {
            let uri: axum::http::Uri = path.parse().unwrap();
            let response = serve_spa(uri).await;
            let ct = response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap();
            assert!(
                !ct.to_str().unwrap().contains("application/json"),
                "path: {path} should not return JSON — it should bypass the API prefix guard"
            );
        }
    }

    #[tokio::test]
    async fn handle_method_not_allowed_returns_json_405() {
        let response = handle_method_not_allowed().await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let ct = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap();
        assert!(
            ct.to_str().unwrap().contains("application/json"),
            "405 response should be JSON, got: {ct:?}"
        );
        let cc = response
            .headers()
            .get(axum::http::header::CACHE_CONTROL)
            .unwrap();
        assert_eq!(cc.to_str().unwrap(), "no-store");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error_body: ErrorBody = serde_json::from_slice(&body).unwrap();
        assert_eq!(error_body.code, ErrorCode::MethodNotAllowed);
    }
}
