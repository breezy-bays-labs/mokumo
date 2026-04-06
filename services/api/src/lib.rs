pub mod activity;
pub mod auth;
pub mod customer;
pub mod demo;
pub mod discovery;
pub mod error;
pub mod pagination;
pub mod profile_db;
pub mod profile_switch;
pub mod rate_limit;
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
use mokumo_core::setup::SetupMode;
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
    /// Demo profile database connection.
    pub demo_db: DatabaseConnection,
    /// Production profile database connection.
    pub production_db: DatabaseConnection,
    /// The currently active profile. Controls the unauthenticated fallback in
    /// `ProfileDbMiddleware` and demo auto-login detection.
    ///
    /// Wrapped in `RwLock` so the profile-switch handler (Session 2) can update
    /// it in-process without a restart. Reads are always `read().unwrap()`;
    /// writes happen only in the profile-switch handler after persisting to disk.
    pub active_profile: std::sync::RwLock<SetupMode>,
    pub ws: Arc<ws::manager::ConnectionManager>,
    pub shutdown: CancellationToken,
    pub started_at: std::time::Instant,
    pub mdns_status: discovery::SharedMdnsStatus,
    pub local_ip: tokio::sync::watch::Receiver<Option<std::net::IpAddr>>,
    pub setup_completed: Arc<AtomicBool>,
    pub setup_in_progress: Arc<AtomicBool>,
    pub setup_token: Option<String>,
    pub data_dir: PathBuf,
    /// In-memory store for file-drop password reset PINs. Maps email → PendingReset.
    pub reset_pins: Arc<dashmap::DashMap<String, PendingReset>>,
    /// Directory where recovery files are placed for file-drop password reset.
    pub recovery_dir: PathBuf,
    /// Rate limiter for recovery code verification attempts (5 per 15 min per email).
    pub recovery_limiter: rate_limit::RateLimiter,
    /// Rate limiter for recovery code regeneration attempts (3 per hour per user).
    pub regen_limiter: rate_limit::RateLimiter,
    /// Rate limiter for profile switch attempts (3 per 15 min per user).
    pub switch_limiter: rate_limit::RateLimiter,
    /// True until the first profile switch completes (set false after active_profile is written).
    /// Initialized at startup from whether the active_profile file is absent.
    pub is_first_launch: Arc<AtomicBool>,
}

impl AppState {
    /// Return the database connection for the given profile.
    pub fn db_for(&self, mode: SetupMode) -> &DatabaseConnection {
        match mode {
            SetupMode::Demo => &self.demo_db,
            SetupMode::Production => &self.production_db,
        }
    }
}

pub type SharedState = Arc<AppState>;

#[derive(Embed)]
#[folder = "../../apps/web/build"]
struct SpaAssets;

/// Create the required data directories: data_dir, demo/, production/, and logs/.
///
/// Returns an error with the path included in the message on failure.
pub fn ensure_data_dirs(data_dir: &Path) -> Result<(), std::io::Error> {
    for dir in [
        data_dir.to_path_buf(),
        data_dir.join("demo"),
        data_dir.join("production"),
        data_dir.join("logs"),
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

/// Read the `active_profile` file from the data directory.
///
/// Returns `SetupMode::Demo` if the file does not exist, is empty, or contains
/// an unrecognised value (first launch defaults to demo).
pub fn resolve_active_profile(data_dir: &Path) -> mokumo_core::setup::SetupMode {
    use mokumo_core::setup::SetupMode;

    let profile_path = data_dir.join("active_profile");
    match std::fs::read_to_string(&profile_path) {
        Ok(contents) => contents.trim().parse().unwrap_or(SetupMode::Demo),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SetupMode::Demo,
        Err(e) => {
            tracing::error!(path = %profile_path.display(), "Failed to read active_profile file: {e}; defaulting to demo");
            SetupMode::Demo
        }
    }
}

/// Migrate a flat data directory layout to the dual-profile structure.
///
/// Idempotent: safe to call on every startup.
///
/// Steps:
/// 1. If `production/mokumo.db` does NOT exist AND flat `mokumo.db` DOES exist:
///    copy flat -> production/mokumo.db
/// 2. If `active_profile` does NOT exist: write "production"
///    (existing users who had a flat layout are production users)
/// 3. If BOTH `production/mokumo.db` AND flat `mokumo.db` exist: remove flat
pub fn migrate_flat_layout(data_dir: &Path) -> Result<(), std::io::Error> {
    let flat_db = data_dir.join("mokumo.db");
    let production_db = data_dir.join("production").join("mokumo.db");
    let profile_path = data_dir.join("active_profile");

    let flat_exists = flat_db.try_exists()?;
    let production_exists = production_db.try_exists()?;

    // Step 1: Copy flat DB to production/ if production doesn't have one yet
    if !production_exists && flat_exists {
        std::fs::create_dir_all(data_dir.join("production"))?;
        // Best-effort WAL checkpoint before copying: ensures committed but
        // un-checkpointed transactions are included in the destination file.
        // Logs a warning and continues if the file isn't in WAL mode or isn't
        // a valid SQLite database (e.g., legacy installs that never used WAL).
        if let Ok(conn) = rusqlite::Connection::open(&flat_db)
            && let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        {
            tracing::warn!(
                "WAL checkpoint failed during flat DB migration (proceeding with copy): {e}"
            );
        }
        std::fs::copy(&flat_db, &production_db)?;
        tracing::info!("Migrated flat database to {}", production_db.display());
    }

    // Step 2: Write active_profile = "production" for existing users
    if !profile_path.try_exists()? && flat_exists {
        std::fs::write(&profile_path, "production")?;
        tracing::info!("Set active profile to 'production' (migrated from flat layout)");
    }

    // Step 3: Remove flat DB — at this point production either already existed
    // (crash recovery) or was just created by Step 1. Either way, flat is redundant.
    if flat_exists {
        std::fs::remove_file(&flat_db)?;
        tracing::info!("Removed flat database after migration");
        // WAL and SHM files may not exist — silently ignore NotFound
        let _ = std::fs::remove_file(data_dir.join("mokumo.db-wal"));
        let _ = std::fs::remove_file(data_dir.join("mokumo.db-shm"));
    }

    Ok(())
}

/// Shared startup sequence: create directories, migrate layout, copy sidecar,
/// resolve profile, run guard chain and initialize both databases.
///
/// Guard chain per profile (active first, then non-active):
///   1. `check_application_id` — reject non-Mokumo SQLite files
///   2. `pre_migration_backup` — WAL-safe backup before any migration runs
///   3. `check_schema_compatibility` — reject databases from newer Mokumo versions
///      (demo: silent recreate from sidecar; production: hard abort)
///   4. `initialize_database` — pool + PRAGMAs + `Migrator::up()`
///
/// Used by both the CLI server (`main.rs`) and the desktop app (`lib.rs`).
/// Returns `(demo_db, production_db, profile)` on success.
pub async fn prepare_database(
    data_dir: &Path,
) -> Result<
    (
        DatabaseConnection,
        DatabaseConnection,
        mokumo_core::setup::SetupMode,
    ),
    Box<dyn std::error::Error>,
> {
    use mokumo_core::setup::SetupMode;

    ensure_data_dirs(data_dir)?;
    migrate_flat_layout(data_dir)?;

    if let Err(e) = demo::copy_sidecar_if_needed(data_dir) {
        tracing::warn!(
            "Failed to copy demo sidecar: {e}; \
             demo will start with empty database (no pre-seeded data)"
        );
    }

    let profile = resolve_active_profile(data_dir);
    let other_profile = match profile {
        SetupMode::Demo => SetupMode::Production,
        SetupMode::Production => SetupMode::Demo,
    };

    let active_db_path = data_dir.join(profile.as_str()).join("mokumo.db");
    let other_db_path = data_dir.join(other_profile.as_str()).join("mokumo.db");

    let active_db = setup_profile_db(&active_db_path, profile == SetupMode::Production, data_dir)
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    tracing::info!(
        "Active database ({profile}) ready at {}",
        active_db_path.display()
    );

    let other_db = setup_profile_db(
        &other_db_path,
        other_profile == SetupMode::Production,
        data_dir,
    )
    .await
    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    tracing::info!(
        "Non-active database ({other_profile}) ready at {}",
        other_db_path.display()
    );

    let (demo_db, production_db) = match profile {
        SetupMode::Demo => (active_db, other_db),
        SetupMode::Production => (other_db, active_db),
    };

    Ok((demo_db, production_db, profile))
}

/// Run the full guard chain for one profile database and return an initialized connection.
///
/// Guards run in order:
///   1. `check_application_id` (pre-pool; only if DB file exists)
///   2. `pre_migration_backup` (only if DB file exists)
///   3. `check_schema_compatibility` (pre-pool; only if DB file exists)
///      - If demo profile is incompatible: silently recreate from sidecar and continue.
///      - If production profile is incompatible: hard abort with actionable message.
///   4. `initialize_database` (pool + migrations)
///
/// Returns a human-readable error string on failure (technical detail sent to tracing).
async fn setup_profile_db(
    db_path: &Path,
    is_production: bool,
    data_dir: &Path,
) -> Result<DatabaseConnection, String> {
    use mokumo_db::DatabaseSetupError;

    // Pre-migration backup only runs when the DB file already exists.
    // Track this so format_db_setup_error can omit the backup claim for fresh installs.
    let backup_taken = db_path.exists();

    if backup_taken {
        // Guard 1: confirm this file belongs to Mokumo
        mokumo_db::check_application_id(db_path).map_err(|e| match e {
            DatabaseSetupError::NotMokumoDatabase { ref path } => format!(
                "The database at {} is not a Mokumo database. \
                 Check your --data-dir setting.",
                path.display()
            ),
            _ => format!("application_id check failed for {}: {e}", db_path.display()),
        })?;

        // Guard 2: backup before any migration runs
        mokumo_db::pre_migration_backup(db_path)
            .await
            .map_err(|e| {
                format!(
                    "Pre-migration backup failed for {}: {e}. \
                     Refusing to run migrations without a backup. \
                     Check disk space and permissions.",
                    db_path.display()
                )
            })?;

        // Guard 3: reject databases from newer Mokumo versions
        match mokumo_db::check_schema_compatibility(db_path) {
            Ok(()) => {}
            Err(DatabaseSetupError::SchemaIncompatible {
                ref path,
                ref unknown_migrations,
            }) => {
                if is_production {
                    return Err(format!(
                        "The production database at {} was created by a newer version of Mokumo. \
                         Please upgrade Mokumo to the latest version, or restore from a backup. \
                         Do not delete the database — your production data is there.",
                        path.display()
                    ));
                }
                // Demo profile: silent recreate from sidecar (ephemeral data)
                tracing::warn!(
                    ?unknown_migrations,
                    "Demo database has unknown migrations from newer Mokumo version; \
                     resetting to fresh demo data."
                );
                demo::force_copy_sidecar(data_dir)
                    .map_err(|e| format!("Failed to reset demo database: {e}"))?;
                // Re-run guards on the fresh sidecar before initializing.
                // The bundled sidecar could theoretically be malformed or from a future version.
                if db_path.exists() {
                    mokumo_db::check_application_id(db_path).map_err(|e| match e {
                        DatabaseSetupError::NotMokumoDatabase { ref path } => format!(
                            "The bundled demo database is not a valid Mokumo database: {}. \
                             Please reinstall Mokumo.",
                            path.display()
                        ),
                        _ => format!(
                            "application_id check failed for demo database after reset: {e}"
                        ),
                    })?;
                    mokumo_db::pre_migration_backup(db_path)
                        .await
                        .map_err(|e| {
                            format!(
                                "Pre-migration backup failed for demo database after reset: {e}"
                            )
                        })?;
                    if let Err(e) = mokumo_db::check_schema_compatibility(db_path) {
                        return Err(format!(
                            "Demo database failed compatibility check after reset: {e}"
                        ));
                    }
                }
                let url = format!("sqlite:{}?mode=rwc", db_path.display());
                // Backup was taken on the fresh sidecar (re-run guards ran pre_migration_backup above).
                return mokumo_db::initialize_database(&url)
                    .await
                    .map_err(|e| format_db_setup_error(e, db_path, true));
            }
            Err(e) => {
                return Err(format!(
                    "Schema compatibility check failed for {}: {e}",
                    db_path.display()
                ));
            }
        }
    }

    // Guard 4: initialize pool + run migrations
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    mokumo_db::initialize_database(&url)
        .await
        .map_err(|e| format_db_setup_error(e, db_path, backup_taken))
}

/// Format a `DatabaseSetupError` into a human-readable message for the operator.
///
/// `backup_taken` indicates whether `pre_migration_backup` ran before the error occurred.
/// When `false` (fresh install), the backup claim is omitted to avoid a false assertion.
///
/// Technical details (DbErr internals) are sent to `tracing::error!` only.
fn format_db_setup_error(
    e: mokumo_db::DatabaseSetupError,
    db_path: &Path,
    backup_taken: bool,
) -> String {
    use mokumo_db::DatabaseSetupError;
    tracing::error!("Database setup error for {}: {:?}", db_path.display(), e);
    match e {
        DatabaseSetupError::Migration(_) => {
            let backup_note = if backup_taken {
                " Your data was backed up before the attempt."
            } else {
                ""
            };
            format!(
                "Mokumo could not apply a database migration to {}.{backup_note} \
                 Contact support if this persists.",
                db_path.display()
            )
        }
        DatabaseSetupError::SchemaIncompatible { ref path, .. } => format!(
            "The database at {} was created by a newer version of Mokumo. \
             Please upgrade Mokumo to the latest version, or restore from a backup.",
            path.display()
        ),
        DatabaseSetupError::NotMokumoDatabase { ref path } => format!(
            "The database at {} is not a Mokumo database. \
             Check your --data-dir setting.",
            path.display()
        ),
        DatabaseSetupError::Pool(_) => format!(
            "Failed to open database connection pool at {}. \
             Check disk space and file permissions.",
            db_path.display()
        ),
        DatabaseSetupError::Query(_) => format!(
            "A database query failed during initialization of {}. \
             Your data was not modified.",
            db_path.display()
        ),
        DatabaseSetupError::Rusqlite(_) => format!(
            "A low-level database error occurred while checking {}. \
             Check disk space and file permissions.",
            db_path.display()
        ),
    }
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
///
/// The session store is opened from a SEPARATE SQLite database at `session_db_path`,
/// keeping session data independent of the active profile database.
///
/// `production_db` is used to check setup completion status (production setup
/// is what the wizard populates).
pub async fn init_session_and_setup(
    production_db: &DatabaseConnection,
    session_db_path: &Path,
) -> Result<(SqliteStore, Arc<AtomicBool>, Option<String>), Box<dyn std::error::Error + Send + Sync>>
{
    let is_complete = mokumo_db::is_setup_complete(production_db).await?;
    let setup_completed = Arc::new(AtomicBool::new(is_complete));
    let setup_token = if is_complete {
        None
    } else {
        Some(generate_setup_token())
    };

    // Open a separate SQLite pool for sessions
    let session_url = format!("sqlite:{}?mode=rwc", session_db_path.display());
    let session_pool = mokumo_db::open_raw_sqlite_pool(&session_url)
        .await
        .map_err(|e| {
            format!(
                "Failed to open session database at {}: {e}",
                session_db_path.display()
            )
        })?;
    let session_store = SqliteStore::new(session_pool);
    session_store
        .migrate()
        .await
        .map_err(|e| format!("Session store migration failed: {e}"))?;

    Ok((session_store, setup_completed, setup_token))
}

/// Build the Axum router with health check, SPA fallback, and tracing.
///
/// Test-only convenience wrapper. Does NOT spawn the background IP refresh
/// task — the local IP is computed once and never updated. Use
/// `build_app_with_shutdown` in production for graceful lifecycle control.
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
pub async fn build_app(
    config: &ServerConfig,
    demo_db: DatabaseConnection,
    production_db: DatabaseConnection,
    active_profile: SetupMode,
) -> Result<(Router, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
    let local_ip = local_ip_address::local_ip().ok();
    let (_, local_ip_rx) = tokio::sync::watch::channel(local_ip);

    let session_db_path = config.data_dir.join("sessions.db");
    let (session_store, setup_completed, setup_token) =
        init_session_and_setup(&production_db, &session_db_path).await?;

    let router = build_app_inner(
        config,
        demo_db,
        production_db,
        active_profile,
        CancellationToken::new(),
        discovery::MdnsStatus::shared(),
        local_ip_rx,
        session_store,
        setup_completed,
        setup_token.clone(),
    );
    Ok((router, setup_token))
}

/// Build the Axum router with an explicit shutdown token.
///
/// The token is stored in `AppState` so handlers (e.g. WebSocket) can observe
/// shutdown and drain gracefully. Spawns background tasks for IP refresh and
/// expired session cleanup, both stopped by the shutdown token.
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
pub async fn build_app_with_shutdown(
    config: &ServerConfig,
    demo_db: DatabaseConnection,
    production_db: DatabaseConnection,
    active_profile: SetupMode,
    shutdown: CancellationToken,
    mdns_status: discovery::SharedMdnsStatus,
) -> Result<(Router, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
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

    let router = build_app_inner(
        config,
        demo_db,
        production_db,
        active_profile,
        shutdown,
        mdns_status,
        local_ip_rx,
        session_store,
        setup_completed,
        setup_token.clone(),
    );
    Ok((router, setup_token))
}

#[allow(clippy::too_many_arguments)]
#[allow(unused_variables)] // config will be used by future CORS/rate-limit settings
fn build_app_inner(
    config: &ServerConfig,
    demo_db: DatabaseConnection,
    production_db: DatabaseConnection,
    active_profile: SetupMode,
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

    // Auth backend holds both databases; dispatches by compound user ID.
    let backend = Backend::new(demo_db.clone(), production_db.clone());
    let auth_layer = AuthManagerLayerBuilder::new(backend, session_layer).build();

    // Fresh install: active_profile file absent → first launch. Checked here (after
    // prepare_database has run migrate_flat_layout) so upgrades from flat layout are
    // not mistakenly treated as first launches.
    let first_launch = !config.data_dir.join("active_profile").exists();

    let state: SharedState = Arc::new(AppState {
        demo_db,
        production_db,
        active_profile: std::sync::RwLock::new(active_profile),
        ws: Arc::new(ws::manager::ConnectionManager::new(64)),
        shutdown,
        started_at: std::time::Instant::now(),
        mdns_status,
        local_ip,
        setup_completed,
        setup_in_progress: Arc::new(AtomicBool::new(false)),
        setup_token,
        data_dir: config.data_dir.clone(),
        reset_pins: Arc::new(dashmap::DashMap::new()),
        recovery_dir: config.recovery_dir.clone(),
        recovery_limiter: rate_limit::RateLimiter::new(
            rate_limit::DEFAULT_MAX_ATTEMPTS,
            rate_limit::DEFAULT_WINDOW,
        ),
        regen_limiter: rate_limit::RateLimiter::new(3, std::time::Duration::from_secs(3600)),
        switch_limiter: rate_limit::RateLimiter::new(3, rate_limit::DEFAULT_WINDOW),
        is_first_launch: Arc::new(AtomicBool::new(first_launch)),
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

    // Protected routes: require login (with demo auto-login support)
    //
    // Uses a combined middleware that handles both demo auto-login and auth checking
    // in a single layer. This is necessary because login_required! checks the user
    // from the incoming request, which doesn't reflect a session created by a
    // preceding middleware in the same request cycle.
    let protected_routes = Router::new()
        .nest("/api/customers", customer::router())
        .nest("/api/activity", activity::router())
        .nest("/api/auth", auth::auth_me_router())
        .route(
            "/api/account/recovery-codes/regenerate",
            post(auth::regenerate_recovery_codes),
        )
        .route("/api/demo/reset", post(demo::demo_reset))
        .route("/api/profile/switch", post(profile_switch::profile_switch))
        .route("/ws", get(ws::ws_handler))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth_with_demo_auto_login,
        ));

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
        // ProfileDbMiddleware: innermost — runs after auth session is populated.
        // Injects ProfileDb into request extensions for all routes.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            profile_db::profile_db_middleware,
        ))
        .layer(auth_layer)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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

/// Resolve the directory for password-reset recovery files.
///
/// Priority: MOKUMO_RECOVERY_DIR env var > user's Desktop (macOS/Windows) > cwd.
/// On Linux, Desktop may not be available (XDG Desktop is optional), so the
/// effective priority is env var > cwd.
pub fn resolve_recovery_dir() -> PathBuf {
    std::env::var("MOKUMO_RECOVERY_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(dirs::desktop_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

// ---------------------------------------------------------------------------
// Process-level lock (prevents concurrent server + reset-db)
// ---------------------------------------------------------------------------

/// Path to the process-level lock file within the data directory.
pub fn lock_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join("mokumo.lock")
}

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
    // Check both profile databases — either being unhealthy makes the whole instance unhealthy
    mokumo_db::health_check(state.db_for(SetupMode::Demo)).await?;
    mokumo_db::health_check(state.db_for(SetupMode::Production)).await?;

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

async fn setup_status(
    State(state): State<SharedState>,
) -> Result<Json<mokumo_types::setup::SetupStatusResponse>, crate::error::AppError> {
    let active = *state.active_profile.read().unwrap();
    // Demo is always pre-seeded — never needs the setup wizard.
    // Production uses the AtomicBool that is set when the wizard completes.
    let setup_complete = match active {
        mokumo_core::setup::SetupMode::Demo => true,
        mokumo_core::setup::SetupMode::Production => state
            .setup_completed
            .load(std::sync::atomic::Ordering::Acquire),
    };
    let is_first_launch = state
        .is_first_launch
        .load(std::sync::atomic::Ordering::Acquire);

    let shop_name = mokumo_db::get_shop_name(&state.production_db)
        .await
        .map_err(|e| {
            tracing::error!("setup_status: failed to fetch shop_name: {e}");
            crate::error::AppError::InternalError("Failed to read shop configuration".into())
        })?;

    // Query production_db directly so this reflects the production setup state regardless of
    // which profile is currently active. Mirrors the shop_name pattern above.
    let production_setup_complete = mokumo_db::is_setup_complete(&state.production_db)
        .await
        .map_err(|e| {
            tracing::error!("setup_status: failed to fetch production_setup_complete: {e}");
            crate::error::AppError::InternalError("Failed to read production setup status".into())
        })?;

    Ok(Json(mokumo_types::setup::SetupStatusResponse {
        setup_complete,
        setup_mode: setup_complete.then_some(active),
        is_first_launch,
        production_setup_complete,
        shop_name,
    }))
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

/// Last-resort JSON returned when serializing an `ErrorBody` to JSON fails
/// in `serve_spa`. Kept as a byte-string constant so the sync-guard test can
/// verify it stays in sync with the canonical serde output.
pub(crate) const INTERNAL_ERROR_FALLBACK_JSON: &[u8] =
    br#"{"code":"internal_error","message":"An internal error occurred","details":null}"#;

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
            INTERNAL_ERROR_FALLBACK_JSON.to_vec()
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

#[cfg(test)]
mod tests {
    /// The content of `INTERNAL_ERROR_FALLBACK_JSON` must match what serde
    /// produces for `redacted_internal()`. If an `ErrorCode` variant is renamed
    /// or serde attributes change, this test catches the divergence.
    #[test]
    fn fallback_json_matches_serde_output() {
        let expected =
            serde_json::to_vec(&crate::error::redacted_internal()).expect("must serialize");

        assert_eq!(
            expected.as_slice(),
            super::INTERNAL_ERROR_FALLBACK_JSON,
            "INTERNAL_ERROR_FALLBACK_JSON diverged from serde output. \
             Update the constant to: {}",
            String::from_utf8_lossy(&expected),
        );
    }
}
