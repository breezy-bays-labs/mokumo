use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::Router;
use axum_login::AuthManagerLayerBuilder;
use parking_lot::RwLock;
use sea_orm::DatabaseConnection;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;
use tower_sessions_sqlx_store::SqliteStore;

use crate::activity::{ActivityWriter, SqliteActivityWriter};
use crate::boot::BootConfig;
use crate::control_plane::SetupTokenSource;
use crate::control_plane::state::ControlPlaneState;
use crate::data_plane::csrf_layer::CsrfLayer;
use crate::data_plane::forwarded_layer::ForwardedLayer;
use crate::data_plane::rate_limiter_layer::{PerIpRateLimit, PerIpRateLimiterLayer};
use crate::data_plane::session_layer::session_layer_for_mode;
use crate::error::EngineError;
use crate::graft::{Graft, SelfGraft, SubGraft};
use crate::middleware::host_allowlist::HostHeaderAllowList;
use crate::middleware::security_headers;
use crate::migrations;
use crate::migrations::Migration;
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
    /// setup-token resolution via [`Graft::setup_token_source`].
    ///
    /// The vertical's file-drop recovery directory and reset-PIN store
    /// are no longer boot parameters. The vertical owns those pieces on
    /// its own state slice and exposes the recovery-dir path through
    /// [`Graft::recovery_dir`] for any kikan-side caller that needs it.
    #[allow(clippy::too_many_arguments)]
    pub async fn boot(
        config: BootConfig,
        graft: &G,
        pools: HashMap<ProfileDirName, DatabaseConnection>,
        active_profile: ProfileDirName,
        session_store: SqliteStore,
        profile_db_initializer: SharedProfileDbInitializer,
        setup_completed: Arc<AtomicBool>,
        demo_install_ok: Arc<AtomicBool>,
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
        // as opaque data from here on. `kind.to_string()` (via Display) is
        // the single source of truth for on-disk directory names — see
        // `Graft::ProfileKind` invariant docs.
        //
        // Fail-fast invariant check: every declared kind must produce a
        // path-safe ProfileDirName AND round-trip through FromStr back to
        // the same kind. A graft whose Display/FromStr are not inverses
        // would otherwise silently drop profiles from the auth pool or
        // route to the wrong dir.
        let profile_dir_names: Arc<[ProfileDirName]> = graft
            .all_profile_kinds()
            .iter()
            .map(|k| validate_profile_kind::<G>(k))
            .collect::<Result<Vec<_>, _>>()?
            .into();
        let requires_setup_by_dir: HashMap<ProfileDirName, bool> = graft
            .all_profile_kinds()
            .iter()
            .map(|k| validate_profile_kind::<G>(k).map(|dir| (dir, graft.requires_setup_wizard(k))))
            .collect::<Result<_, _>>()?;

        let auth_kind = graft.auth_profile_kind();
        let auth_profile_kind_dir = validate_profile_kind::<G>(&auth_kind)?;

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

        // ── Resolve setup_token via Graft hook ───────────────────────
        //
        // The vertical declares its token source; kikan reads it once at
        // boot and stashes the value for the setup_admin pure-fn to
        // compare against. I/O errors on `File` surface as
        // `EngineError::Boot` — the engine refuses to start rather than
        // run with an indeterminate token. (Fail-fast at boot per ADR
        // amendment 2026-04-22 (a).)
        let setup_token: Option<Arc<str>> = resolve_setup_token(graft.setup_token_source())?;

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

    /// Wrap `G::data_plane_routes(&state)` with the full middleware stack
    /// and bind `state`.
    ///
    /// Axum applies the last `.layer()` as the outermost wrap. Layer order
    /// (outermost → innermost) — see `data_plane::mod` for per-mode
    /// behavior:
    ///
    /// 1. **HostHeaderAllowList** — reject disallowed Host headers first.
    /// 2. **ForwardedLayer** — trust or strip `X-Forwarded-*`.
    /// 3. **PerIpRateLimiterLayer** — per-IP global limit (non-Lan).
    /// 4. **SecurityHeaders** — CSP, X-Frame-Options, etc.
    /// 5. **TraceLayer** — request/response tracing.
    /// 6. **AuthManagerLayer** — session + auth backend (axum-login).
    /// 7. **CsrfLayer** — double-submit cookie + Origin check (non-Lan).
    /// 8. **ProfileDbMiddleware** — inject per-request `ProfileDb` based on
    ///    the authenticated session's profile. Uses `from_fn_with_state` to
    ///    bind `PlatformState` independently of `G::AppState`.
    pub fn build_router(&self, state: G::AppState) -> Router {
        use std::str::FromStr;

        let platform = G::platform_state(&state);
        let dp = &self.config.data_plane;

        // Auth backend dispatches by compound user ID across every profile
        // pool the graft declared. Every dir name in `profile_dir_names`
        // round-trips through `K::from_str` by construction —
        // `Engine::boot` verified that invariant for every kind, so an
        // `Err` here signals bookkeeping drift (not a runtime surprise).
        let mut pool_map: HashMap<G::ProfileKind, DatabaseConnection> = HashMap::new();
        for dir in platform.profile_dir_names.iter() {
            let Some(pool) = platform.db_for(dir.as_str()) else {
                continue;
            };
            let kind = G::ProfileKind::from_str(dir.as_str())
                .expect("boot invariant: profile dir round-trips through K::from_str");
            pool_map.insert(kind, pool.clone());
        }
        let auth_kind = G::ProfileKind::from_str(platform.auth_profile_kind_dir.as_str())
            .expect("boot invariant: auth profile kind dir round-trips through K::from_str");
        let backend = crate::auth::Backend::<G::ProfileKind>::new(Arc::new(pool_map), auth_kind);
        let auth_layer = AuthManagerLayerBuilder::new(
            backend,
            session_layer_for_mode(&self.ctx.sessions, dp.deployment_mode),
        )
        .build();

        let csrf_layer = CsrfLayer::for_mode(dp.deployment_mode, dp.allowed_origins.clone());
        let rate_limit_layer =
            PerIpRateLimiterLayer::for_mode(dp.deployment_mode, PerIpRateLimit::default());
        let forwarded_layer = ForwardedLayer::for_mode(dp.deployment_mode);
        let host_allowlist = HostHeaderAllowList::from_config(dp);

        G::data_plane_routes(&state)
            .layer(axum::middleware::from_fn_with_state(
                platform.clone(),
                crate::profile_db::profile_db_middleware::<G::ProfileKind>,
            ))
            .layer(csrf_layer)
            .layer(auth_layer)
            .layer(TraceLayer::new_for_http())
            .layer(axum::middleware::from_fn(security_headers::middleware))
            .layer(rate_limit_layer)
            .layer(forwarded_layer)
            .layer(host_allowlist)
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
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await?;
        Ok(())
    }
}

/// Verify a `ProfileKind` satisfies the two invariants kikan relies on at
/// every request:
///
/// 1. `kind.to_string()` produces a path-safe [`ProfileDirName`] (non-empty,
///    no path separators, no `.`/`..`/leading-dot, no NUL).
/// 2. The string round-trips through `K::from_str(kind.to_string())` back
///    to an equal `K`.
///
/// Both are required for the vocabulary-neutral design: dir names are the
/// primary key for per-profile state, and kikan reconstructs `K` from
/// those strings at request time. Failure = Graft invariant violation;
/// bubble it up as `EngineError::Boot` so the app refuses to start.
/// Resolve a [`SetupTokenSource`] into the effective token value.
///
/// Empty or whitespace-only resolutions collapse to `None` (equivalent to
/// `Disabled`). A zero-length or whitespace-only setup token would otherwise
/// match a zero-length or whitespace request body in `setup_admin` and
/// silently permit unauthenticated bootstrap. Both `Inline(Arc<str>)` and
/// `File` variants are trimmed; the empty-after-trim case normalizes to
/// Disabled. I/O errors on `File` surface as [`EngineError::Boot`].
fn resolve_setup_token(source: SetupTokenSource) -> Result<Option<Arc<str>>, EngineError> {
    match source {
        SetupTokenSource::Disabled => Ok(None),
        SetupTokenSource::Inline(t) => {
            let trimmed = t.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else if trimmed.len() == t.len() {
                Ok(Some(t))
            } else {
                Ok(Some(Arc::from(trimmed)))
            }
        }
        SetupTokenSource::File(path) => {
            let raw = std::fs::read_to_string(&path).map_err(|e| {
                EngineError::Boot(format!(
                    "Graft::setup_token_source file {} could not be read: {e}",
                    path.display()
                ))
            })?;
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(Arc::from(trimmed)))
            }
        }
    }
}

fn validate_profile_kind<G: Graft>(kind: &G::ProfileKind) -> Result<ProfileDirName, EngineError> {
    use std::str::FromStr;
    let dir_string = kind.to_string();
    let dir = ProfileDirName::new(dir_string.clone()).map_err(|e| {
        EngineError::Boot(format!(
            "Graft::ProfileKind `{kind:?}` serializes to invalid profile dir {dir_string:?}: {e}"
        ))
    })?;
    let parsed = G::ProfileKind::from_str(dir.as_str()).map_err(|e| {
        EngineError::Boot(format!(
            "Graft::ProfileKind Display/FromStr are not inverses: {kind:?} serializes to {dir_string:?} but FromStr rejects it: {e}"
        ))
    })?;
    if &parsed != kind {
        return Err(EngineError::Boot(format!(
            "Graft::ProfileKind Display/FromStr round-trip mismatch: {kind:?} → {dir_string:?} → {parsed:?}"
        )));
    }
    Ok(dir)
}

#[cfg(test)]
mod resolve_setup_token_tests {
    use super::{EngineError, SetupTokenSource, resolve_setup_token};
    use std::sync::Arc;

    #[test]
    fn disabled_resolves_to_none() {
        let got = resolve_setup_token(SetupTokenSource::Disabled).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn inline_empty_normalizes_to_none() {
        let got = resolve_setup_token(SetupTokenSource::Inline(Arc::from(""))).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn inline_whitespace_only_normalizes_to_none() {
        let got = resolve_setup_token(SetupTokenSource::Inline(Arc::from(" \t\n"))).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn inline_clean_passes_through_without_realloc() {
        let original: Arc<str> = Arc::from("tok-abc");
        let got = resolve_setup_token(SetupTokenSource::Inline(original.clone())).unwrap();
        let Some(resolved) = got else {
            panic!("expected Some");
        };
        assert_eq!(&*resolved, "tok-abc");
        assert!(Arc::ptr_eq(&original, &resolved));
    }

    #[test]
    fn inline_with_surrounding_whitespace_is_trimmed() {
        let got = resolve_setup_token(SetupTokenSource::Inline(Arc::from("  tok-abc\n"))).unwrap();
        assert_eq!(&*got.unwrap(), "tok-abc");
    }

    #[test]
    fn file_missing_surfaces_boot_error() {
        let err = resolve_setup_token(SetupTokenSource::File(
            "/nonexistent/path/for/setup-token".into(),
        ))
        .unwrap_err();
        let EngineError::Boot(msg) = err else {
            panic!("expected Boot error");
        };
        assert!(msg.contains("setup_token_source file"));
    }

    #[test]
    fn file_with_content_trims_and_returns_token() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "  file-tok  \n").unwrap();
        let got = resolve_setup_token(SetupTokenSource::File(tmp.path().to_path_buf())).unwrap();
        assert_eq!(&*got.unwrap(), "file-tok");
    }

    #[test]
    fn file_empty_normalizes_to_none() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "").unwrap();
        let got = resolve_setup_token(SetupTokenSource::File(tmp.path().to_path_buf())).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn file_whitespace_only_normalizes_to_none() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), " \t\n\n").unwrap();
        let got = resolve_setup_token(SetupTokenSource::File(tmp.path().to_path_buf())).unwrap();
        assert!(got.is_none());
    }
}
