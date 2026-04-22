use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::Router;
use axum::routing::{get, post};
use axum_login::AuthManagerLayerBuilder;
use dashmap::DashMap;
use parking_lot::RwLock;
use sea_orm::DatabaseConnection;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;
use tower_sessions_sqlx_store::SqliteStore;

use crate::activity::{ActivityWriter, SqliteActivityWriter};
use crate::boot::BootConfig;
use crate::control_plane::state::ControlPlaneState;
use crate::error::EngineError;
use crate::graft::{Graft, SelfGraft, SubGraft};
use crate::middleware::host_allowlist::HostHeaderAllowList;
use crate::middleware::security_headers;
use crate::middleware::session_layer;
use crate::migrations;
use crate::migrations::Migration;
use crate::platform;
use crate::platform_state::{MdnsStatus, PlatformState, SharedProfileDbInitializer};
use crate::rate_limit::RateLimiter;
use crate::tenancy::{ProfileDirName, Tenancy};

/// Runtime context shared across all requests. All fields have O(1) `Clone`:
/// `DatabaseConnection` is internally Arc-wrapped; every other field is an
/// `Arc<T>`. This matters because `FromRef` fires on every request.
///
/// `EngineContext` is the Graft-facing seam per design-doc M3: verticals see
/// this, not the underlying pool/store types.
#[derive(Clone)]
pub struct EngineContext {
    pub pool: DatabaseConnection,
    pub tenancy: Arc<Tenancy>,
    pub activity_writer: Arc<dyn ActivityWriter>,
    pub sessions: Sessions,
}

/// Opaque newtype around the concrete session store. Verticals never name
/// the inner type — swapping stores stays kikan-internal.
#[derive(Clone)]
pub struct Sessions(Arc<SqliteStore>);

impl Sessions {
    pub fn new(store: SqliteStore) -> Self {
        Self(Arc::new(store))
    }

    /// Clone the underlying store (`SqliteStore` is cheap to clone internally).
    pub(crate) fn store(&self) -> SqliteStore {
        (*self.0).clone()
    }
}

pub struct Engine<G: Graft> {
    config: BootConfig,
    ctx: EngineContext,
    all_migrations: Vec<Arc<dyn Migration>>,
    _graft: PhantomData<G>,
}

impl<G: Graft> Engine<G> {
    /// Construct the engine.
    ///
    /// Callers open the main pool and session store separately; the
    /// vertical-aware `initialize_database` wrapper in `mokumo_shop::db`
    /// composes `kikan::db::initialize_database` with the vertical's migrator.
    /// The default activity writer is [`SqliteActivityWriter`]; callers that
    /// need a different writer should use [`Engine::new_with`].
    pub fn new(
        config: BootConfig,
        graft: &G,
        pool: DatabaseConnection,
        session_store: SqliteStore,
    ) -> Result<Self, EngineError> {
        Self::new_with(
            config,
            graft,
            pool,
            session_store,
            Arc::new(SqliteActivityWriter::new()),
        )
    }

    /// Construct the engine with a custom [`ActivityWriter`].
    pub fn new_with(
        config: BootConfig,
        graft: &G,
        pool: DatabaseConnection,
        session_store: SqliteStore,
        activity_writer: Arc<dyn ActivityWriter>,
    ) -> Result<Self, EngineError> {
        let tenancy = Arc::new(Tenancy::new(config.data_dir.clone()));

        let subgraft_migrations: Vec<Vec<Box<dyn Migration>>> =
            std::iter::once(SelfGraft.migrations())
                .chain(config.subgrafts.iter().map(|sg| sg.migrations()))
                .collect();

        let all_migrations =
            migrations::collect_migrations(graft.migrations(), subgraft_migrations);

        let ctx = EngineContext {
            pool,
            tenancy,
            activity_writer,
            sessions: Sessions::new(session_store),
        };

        Ok(Self {
            config,
            ctx,
            all_migrations,
            _graft: PhantomData,
        })
    }

    pub async fn run_migrations(&self, pool: &DatabaseConnection) -> Result<(), EngineError> {
        migrations::runner::run_migrations_with_backfill(pool, &self.all_migrations, Some(G::id()))
            .await
    }

    /// Boot the engine: construct the Engine, then assemble the full
    /// application state from platform + control-plane + domain slices.
    ///
    /// Callers prepare database connections and session store beforehand;
    /// `boot` handles migration execution, state composition, and
    /// setup-token generation.
    #[allow(clippy::too_many_arguments)]
    pub async fn boot(
        config: BootConfig,
        graft: &G,
        pools: HashMap<ProfileDirName, DatabaseConnection>,
        active_profile: ProfileDirName,
        session_store: SqliteStore,
        profile_db_initializer: SharedProfileDbInitializer,
        setup_completed: Arc<AtomicBool>,
        setup_token: Option<String>,
        demo_install_ok: Arc<AtomicBool>,
        recovery_dir: PathBuf,
        shutdown: CancellationToken,
    ) -> Result<(Self, G::AppState), EngineError> {
        let activity_writer: Arc<dyn ActivityWriter> = Arc::new(SqliteActivityWriter::new());

        // Use the active-profile pool as the engine's "main" pool (drives
        // activity writer + session store schema). Individual handlers
        // resolve per-request pools via `PlatformState::db_for`.
        let main_pool = pools.get(&active_profile).cloned().ok_or_else(|| {
            EngineError::Boot(format!(
                "active profile {active_profile:?} has no pool entry in PlatformState pools map"
            ))
        })?;

        let engine = Self::new_with(
            config,
            graft,
            main_pool,
            session_store,
            activity_writer.clone(),
        )?;

        // Run migrations on every profile database.
        for pool in pools.values() {
            engine.run_migrations(pool).await?;
        }

        let first_launch = !engine.config.data_dir.join("active_profile").exists();

        // Snapshot graft vocabulary answers at boot — kikan consumes these
        // as opaque data from here on.
        let profile_dir_names: Arc<[ProfileDirName]> = graft
            .all_profile_kinds()
            .iter()
            .map(|k| ProfileDirName::from(graft.profile_dir_name(k)))
            .collect::<Vec<_>>()
            .into();
        let requires_setup_by_dir: HashMap<ProfileDirName, bool> = graft
            .all_profile_kinds()
            .iter()
            .map(|k| {
                (
                    ProfileDirName::from(graft.profile_dir_name(k)),
                    graft.requires_setup_wizard(k),
                )
            })
            .collect();

        // ── PlatformState ────────────────────────────────────────────
        let platform = PlatformState {
            data_dir: engine.config.data_dir.clone(),
            db_filename: graft.db_filename(),
            pools: Arc::new(pools),
            active_profile: Arc::new(RwLock::new(active_profile)),
            profile_dir_names,
            requires_setup_by_dir: Arc::new(requires_setup_by_dir),
            shutdown,
            started_at: std::time::Instant::now(),
            mdns_status: MdnsStatus::shared(),
            demo_install_ok,
            is_first_launch: Arc::new(AtomicBool::new(first_launch)),
            setup_completed,
            profile_db_initializer,
        };

        // ── ControlPlaneState ────────────────────────────────────────
        let rlc = &engine.config.rate_limit_config;
        let control_plane = ControlPlaneState {
            platform: platform.clone(),
            login_limiter: Arc::new(RateLimiter::new(rlc.login.max_attempts, rlc.login.window)),
            recovery_limiter: Arc::new(RateLimiter::new(
                rlc.recovery.max_attempts,
                rlc.recovery.window,
            )),
            regen_limiter: Arc::new(RateLimiter::new(rlc.regen.max_attempts, rlc.regen.window)),
            switch_limiter: Arc::new(RateLimiter::new(
                rlc.profile_switch.max_attempts,
                rlc.profile_switch.window,
            )),
            reset_pins: Arc::new(DashMap::new()),
            recovery_dir,
            setup_token,
            setup_in_progress: Arc::new(AtomicBool::new(false)),
            activity_writer,
        };

        // ── DomainState ──────────────────────────────────────────────
        let domain = graft.build_domain_state(&engine.ctx).await?;

        // ── Compose ──────────────────────────────────────────────────
        let app_state = G::compose_state(control_plane, domain);

        Ok((engine, app_state))
    }

    pub fn tenancy(&self) -> &Tenancy {
        &self.ctx.tenancy
    }

    pub fn config(&self) -> &BootConfig {
        &self.config
    }

    pub fn context(&self) -> EngineContext {
        self.ctx.clone()
    }

    /// Wrap `G::data_plane_routes(&state)` with the full 5-layer middleware
    /// stack and bind `state`.
    ///
    /// Axum applies the last `.layer()` as the outermost wrap. Layer order
    /// (outermost → innermost):
    ///
    /// 1. **HostHeaderAllowList** — reject disallowed Host headers before
    ///    any other work.
    /// 2. **SecurityHeaders** — CSP, X-Frame-Options, etc. on every response.
    /// 3. **TraceLayer** — request/response tracing.
    /// 4. **AuthManagerLayer** — session + auth backend (axum-login).
    /// 5. **ProfileDbMiddleware** — inject per-request `ProfileDb` based on
    ///    the authenticated session's profile. Uses `from_fn_with_state` to
    ///    bind `PlatformState` independently of `G::AppState`.
    pub fn build_router(&self, state: G::AppState) -> Router {
        let platform = G::platform_state(&state);

        // Auth backend dispatches by compound user ID across both profile DBs.
        // Session 2b bridge: dir-name lookup until Backend goes generic<K>
        // in the follow-up commit. The "demo" / "production" literals leak
        // mokumo vocabulary into kikan by design for one commit only — the
        // Graft generic cascade replaces them with `&kind` map lookups.
        let demo_db = platform
            .db_for("demo")
            .cloned()
            .expect("demo profile pool present at router build time");
        let production_db = platform
            .db_for("production")
            .cloned()
            .expect("production profile pool present at router build time");
        let backend = crate::auth::Backend::new(demo_db, production_db);
        let auth_layer =
            AuthManagerLayerBuilder::new(backend, session_layer(&self.ctx.sessions)).build();

        G::data_plane_routes(&state)
            .layer(axum::middleware::from_fn_with_state(
                platform.clone(),
                crate::profile_db::profile_db_middleware,
            ))
            .layer(auth_layer)
            .layer(TraceLayer::new_for_http())
            .layer(axum::middleware::from_fn(security_headers::middleware))
            .layer(HostHeaderAllowList::loopback_only())
            .with_state(state)
    }

    /// Build the admin router for the Unix domain socket surface.
    ///
    /// No session middleware, no auth layer. The Unix socket's fs-permissions
    /// (mode 0600) are the sole access-control gate.
    ///
    /// Endpoints:
    /// - `GET  /health`              — liveness probe
    /// - `GET  /diagnostics`         — structured diagnostics snapshot
    /// - `GET  /diagnostics/bundle`  — zip export
    /// - `GET  /profiles`            — list profiles with status
    /// - `POST /profiles/switch`     — switch active profile
    /// - `GET  /migrate/status`      — applied migration list per profile
    /// - `GET  /backups`             — list pre-migration backup files
    /// - `POST /backups/create`      — create a database backup
    pub fn admin_router(&self, state: &G::AppState) -> Router {
        let platform = G::platform_state(state).clone();
        admin::build_admin_router(platform)
    }

    /// No-shutdown convenience. Binaries needing graceful shutdown use
    /// [`Engine::build_router`] directly and pass the router to
    /// `axum::serve` with their own shutdown token.
    pub async fn serve(
        &self,
        state: G::AppState,
        listener: TcpListener,
    ) -> Result<(), EngineError> {
        let app = self.build_router(state);
        axum::serve(listener, app).await?;
        Ok(())
    }
}

/// Admin UDS router — control-plane endpoints served over the Unix socket.
pub mod admin {
    use axum::extract::State;
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;
    use axum::routing::{get, post};
    use axum::{Json, Router};

    use crate::platform_state::PlatformState;
    use kikan_types::admin::{
        BackupCreateRequest, BackupCreatedResponse, ProfileSwitchAdminRequest,
    };

    pub fn build_admin_router(state: PlatformState) -> Router {
        Router::new()
            .route("/health", get(health))
            .route("/diagnostics", get(diagnostics))
            .route("/diagnostics/bundle", get(diagnostics_bundle))
            .route("/profiles", get(profiles_list))
            .route("/profiles/switch", post(profiles_switch))
            .route("/migrate/status", get(migrate_status))
            .route("/backups", get(backups_list))
            .route("/backups/create", post(backups_create))
            .with_state(state)
    }

    async fn health() -> &'static str {
        "ok"
    }

    async fn diagnostics(
        State(state): State<PlatformState>,
    ) -> Result<Json<kikan_types::diagnostics::DiagnosticsResponse>, StatusCode> {
        crate::control_plane::diagnostics::collect(&state)
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
        let (bytes, filename) = crate::control_plane::diagnostics::build_bundle(&state)
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

    async fn profiles_list(
        State(state): State<PlatformState>,
    ) -> Result<Json<kikan_types::admin::ProfileListResponse>, StatusCode> {
        crate::control_plane::profile_list::list_profiles(&state)
            .await
            .map(Json)
            .map_err(|e| {
                tracing::error!("admin UDS profiles list failed: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })
    }

    async fn profiles_switch(
        State(state): State<PlatformState>,
        Json(req): Json<ProfileSwitchAdminRequest>,
    ) -> Result<Json<kikan_types::admin::ProfileSwitchAdminResponse>, StatusCode> {
        crate::control_plane::profiles::switch_profile_admin(&state, req.profile)
            .await
            .map(Json)
            .map_err(|e| {
                tracing::error!("admin UDS profile switch failed: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })
    }

    async fn migrate_status(
        State(state): State<PlatformState>,
    ) -> Result<Json<kikan_types::admin::MigrationStatusResponse>, StatusCode> {
        crate::control_plane::migration_status::collect_migration_status(&state)
            .await
            .map(Json)
            .map_err(|e| {
                tracing::error!("admin UDS migration status failed: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })
    }

    async fn backups_list(
        State(state): State<PlatformState>,
    ) -> Json<kikan_types::BackupStatusResponse> {
        // UDS response DTO still names `production` / `demo` — kikan-types
        // wire shape, not a kikan-src vocabulary leak. Collect per
        // dir_name; the DTO field each entry lands in is the corresponding
        // wire name derived by string match.
        let mut production = kikan_types::ProfileBackups { backups: vec![] };
        let mut demo = kikan_types::ProfileBackups { backups: vec![] };
        for dir in state.profile_dir_names.iter() {
            let path = state.data_dir.join(dir.as_str()).join(state.db_filename);
            let entries = collect_profile_backups(&path).await;
            match dir.as_str() {
                "production" => production = entries,
                "demo" => demo = entries,
                _ => tracing::debug!(
                    dir = dir.as_str(),
                    "UDS backups_list: dir not represented in BackupStatusResponse DTO"
                ),
            }
        }
        Json(kikan_types::BackupStatusResponse { production, demo })
    }

    async fn backups_create(
        State(state): State<PlatformState>,
        Json(req): Json<BackupCreateRequest>,
    ) -> Result<Json<BackupCreatedResponse>, StatusCode> {
        let profile = req
            .profile
            .unwrap_or_else(|| default_profile_from_active(&state));
        let dir = profile.as_dir_name();
        let db_path = state.data_dir.join(dir).join(state.db_filename);

        let output_dir = state.data_dir.join(dir);
        let output_name = crate::backup::build_timestamped_name();
        let output_path = output_dir.join(&output_name);

        let db_path_clone = db_path.clone();
        let output_path_clone = output_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            crate::backup::create_backup(&db_path_clone, &output_path_clone)
        })
        .await
        .map_err(|e| {
            tracing::error!("backup task panicked: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map_err(|e| {
            tracing::error!("admin UDS backup create failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        Ok(Json(BackupCreatedResponse {
            path: result.path.display().to_string(),
            size: result.size,
            profile,
        }))
    }

    /// UDS wire-shape bridge: translate the active profile dir-name into
    /// the `SetupMode` variant the admin DTO expects. Uses `FromStr` on
    /// `SetupMode` — mokumo's kikan-types export. Removed alongside the
    /// admin DTO generic cascade in the follow-up commit.
    fn default_profile_from_active(state: &PlatformState) -> kikan_types::SetupMode {
        use std::str::FromStr;
        let active = state.active_profile.read();
        kikan_types::SetupMode::from_str(active.as_str()).unwrap_or(kikan_types::SetupMode::Demo)
    }

    async fn collect_profile_backups(db_path: &std::path::Path) -> kikan_types::ProfileBackups {
        let backups = match crate::backup::collect_existing_backups(db_path).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(path = %db_path.display(), "backup scan failed: {e}");
                return kikan_types::ProfileBackups { backups: vec![] };
            }
        };

        let entries: Vec<kikan_types::BackupEntry> = backups
            .into_iter()
            .rev()
            .map(|(path, mtime)| {
                let version = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.rsplit_once(".backup-v"))
                    .map(|(_, v)| v.to_owned())
                    .unwrap_or_default();
                let backed_up_at = {
                    use chrono::{DateTime, Utc};
                    DateTime::<Utc>::from(mtime).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                };
                kikan_types::BackupEntry {
                    path: path.display().to_string(),
                    version,
                    backed_up_at,
                }
            })
            .collect();

        kikan_types::ProfileBackups { backups: entries }
    }
}

/// Public (unauthenticated) platform routes that consume
/// [`PlatformState`]. Currently:
/// - `GET /api/backup-status`
///
/// The host crate is responsible for binding the inner state with
/// `.with_state(...)` (or merging into the outer router when
/// `PlatformState: FromRef<OuterState>` holds).
pub fn platform_public_routes() -> Router<PlatformState> {
    Router::new().route("/api/backup-status", get(platform::backup_status::handler))
}

/// Protected platform routes (require the host's auth layer). The
/// caller wraps these with whatever `route_layer` enforces login —
/// kikan does not own the auth middleware. Currently:
/// - `POST /api/demo/reset`
/// - `GET  /api/diagnostics`
/// - `GET  /api/diagnostics/bundle`
pub fn platform_protected_routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/demo/reset", post(platform::demo::demo_reset))
        .route("/api/diagnostics", get(platform::diagnostics::handler))
        .route(
            "/api/diagnostics/bundle",
            get(platform::diagnostics_bundle::handler),
        )
}
