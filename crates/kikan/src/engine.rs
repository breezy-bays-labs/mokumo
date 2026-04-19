use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::Router;
use axum::routing::{get, post};
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
use crate::middleware::session_layer;
use crate::migrations;
use crate::migrations::Migration;
use crate::platform;
use crate::platform_state::{MdnsStatus, PlatformState, SharedProfileDbInitializer};
use crate::rate_limit::RateLimiter;
use crate::tenancy::{SetupMode, Tenancy};

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
    /// Callers open the main pool and session store separately; kikan does
    /// not own pool creation in Stage 3 (the `initialize_database` helper
    /// lives in `mokumo_db` until S1.1 lifts it into `kikan::db`). The
    /// default activity writer is [`SqliteActivityWriter`]; callers that
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
        demo_db: DatabaseConnection,
        production_db: DatabaseConnection,
        active_profile: SetupMode,
        session_store: SqliteStore,
        profile_db_initializer: SharedProfileDbInitializer,
        setup_completed: Arc<AtomicBool>,
        setup_token: Option<String>,
        demo_install_ok: Arc<AtomicBool>,
        recovery_dir: PathBuf,
    ) -> Result<(Self, G::AppState), EngineError> {
        let activity_writer: Arc<dyn ActivityWriter> = Arc::new(SqliteActivityWriter::new());

        let engine = Self::new_with(
            config,
            graft,
            production_db.clone(),
            session_store,
            activity_writer.clone(),
        )?;

        // Run migrations on both profile databases.
        engine.run_migrations(&demo_db).await?;
        engine.run_migrations(&production_db).await?;

        let first_launch = !engine.config.data_dir.join("active_profile").exists();

        // ── PlatformState ────────────────────────────────────────────
        let platform = PlatformState {
            data_dir: engine.config.data_dir.clone(),
            demo_db,
            production_db,
            active_profile: Arc::new(RwLock::new(active_profile)),
            shutdown: CancellationToken::new(),
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

    /// Wrap `G::data_plane_routes(&state)` with platform tower layers
    /// (tracing, host allowlist, session layer) and bind `state`.
    ///
    /// Axum applies the last `.layer()` as the outermost wrap. The
    /// pre-Stage-3 composition in `services/api::build_app_inner` has
    /// `HostHeaderAllowList` as the outermost layer (reject bad hosts
    /// before any other work), then `TraceLayer`, then the session
    /// layer innermost. This matches that order. The `platform_routes()`
    /// merge seam is introduced in S3.1 once `MokumoAppState` exists.
    pub fn build_router(&self, state: G::AppState) -> Router {
        G::data_plane_routes(&state)
            .layer(session_layer(&self.ctx.sessions))
            .layer(TraceLayer::new_for_http())
            .layer(HostHeaderAllowList::loopback_only())
            .with_state(state)
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
