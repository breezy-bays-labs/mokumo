use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::Router;
use axum::routing::get;
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

        let auth_profile_kind_dir =
            ProfileDirName::from(graft.profile_dir_name(&graft.auth_profile_kind()));

        // ── PlatformState ────────────────────────────────────────────
        let platform = PlatformState {
            data_dir: engine.config.data_dir.clone(),
            db_filename: graft.db_filename(),
            pools: Arc::new(pools),
            active_profile: Arc::new(RwLock::new(active_profile)),
            profile_dir_names,
            requires_setup_by_dir: Arc::new(requires_setup_by_dir),
            auth_profile_kind_dir,
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
        use std::str::FromStr;

        let platform = G::platform_state(&state);

        // Auth backend dispatches by compound user ID across every profile
        // pool the graft declared. Keys come from `profile_dir_names` — each
        // dir name parses back to the vertical's `ProfileKind` via
        // `K::from_str` (enforced by the `Graft::ProfileKind` bounds).
        let mut pool_map: HashMap<G::ProfileKind, DatabaseConnection> = HashMap::new();
        for dir in platform.profile_dir_names.iter() {
            let Some(pool) = platform.db_for(dir.as_str()) else {
                continue;
            };
            match G::ProfileKind::from_str(dir.as_str()) {
                Ok(kind) => {
                    pool_map.insert(kind, pool.clone());
                }
                Err(e) => {
                    tracing::warn!(
                        dir = dir.as_str(),
                        "profile dir does not parse to Graft::ProfileKind: {e}; skipping from auth backend"
                    );
                }
            }
        }
        let auth_kind = G::ProfileKind::from_str(platform.auth_profile_kind_dir.as_str())
            .expect("auth profile kind dir was captured at boot and must parse back");
        let backend = crate::auth::Backend::<G::ProfileKind>::new(Arc::new(pool_map), auth_kind);
        let auth_layer =
            AuthManagerLayerBuilder::new(backend, session_layer(&self.ctx.sessions)).build();

        G::data_plane_routes(&state)
            .layer(axum::middleware::from_fn_with_state(
                platform.clone(),
                crate::profile_db::profile_db_middleware::<G::ProfileKind>,
            ))
            .layer(auth_layer)
            .layer(TraceLayer::new_for_http())
            .layer(axum::middleware::from_fn(security_headers::middleware))
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
