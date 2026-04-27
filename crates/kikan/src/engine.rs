use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::Router;
use chrono::Utc;
use parking_lot::RwLock;
use sea_orm::DatabaseConnection;
use serde_json::json;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_sessions_sqlx_store::SqliteStore;

use crate::activity::{ActivityWriter, SqliteActivityWriter};
use crate::boot::BootConfig;
use crate::control_plane::SetupTokenSource;
use crate::control_plane::state::ControlPlaneState;
use crate::data_plane::spa::SpaSource;
use crate::error::EngineError;
use crate::graft::{Graft, SelfGraft, SubGraft};
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
    /// Cached once from [`Graft::spa_source`] at construction time.
    /// [`Engine<G>`] holds `PhantomData<G>`, so `build_router` cannot
    /// call back into the graft — the capability has to be captured
    /// here while `&G` is still in scope.
    spa_source: Option<Box<dyn SpaSource>>,
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

        let spa_source = graft.spa_source();

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
            spa_source,
            _graft: PhantomData,
        })
    }

    /// Run every migration in `all_migrations` against `pool`, ignoring
    /// `Migration::target()`. Used by single-pool callers (tests, CLI
    /// init paths). Production boot routes through
    /// [`Engine::run_meta_migrations`] + [`Engine::run_per_profile_migrations`]
    /// so each migration lands on the pool whose role matches its target —
    /// see `adr-kikan-upgrade-migration-strategy.md` §"Per-database
    /// `kikan_migrations` history".
    pub async fn run_migrations(&self, pool: &DatabaseConnection) -> Result<(), EngineError> {
        migrations::runner::run_migrations_with_backfill(pool, &self.all_migrations, Some(G::id()))
            .await
    }

    /// Run only `MigrationTarget::Meta` migrations against the meta-DB pool.
    pub async fn run_meta_migrations(
        &self,
        meta_pool: &DatabaseConnection,
    ) -> Result<(), EngineError> {
        migrations::runner::run_migrations_for_target(
            meta_pool,
            &self.all_migrations,
            crate::migrations::MigrationTarget::Meta,
        )
        .await
    }

    /// Run only `MigrationTarget::PerProfile` migrations against the given
    /// per-profile pool. Callers loop over each per-profile pool. The
    /// vertical's graft id is used to backfill any pre-existing
    /// `seaql_migrations` rows into `kikan_migrations` so per-profile DBs
    /// initialised through SeaORM's `Migrator::up` (e.g. via
    /// `mokumo_shop::db::initialize_database`) aren't re-applied.
    pub async fn run_per_profile_migrations(
        &self,
        profile_pool: &DatabaseConnection,
    ) -> Result<(), EngineError> {
        migrations::runner::run_migrations_for_target_with_backfill(
            profile_pool,
            &self.all_migrations,
            crate::migrations::MigrationTarget::PerProfile,
            Some(G::id()),
        )
        .await
    }

    /// Boot the engine: construct the Engine, then assemble the full
    /// application state from platform + control-plane + domain slices.
    ///
    /// Callers prepare database connections and session store beforehand;
    /// `boot` handles migration execution, state composition, and
    /// setup-token resolution via [`Graft::setup_token_source`].
    ///
    /// # Concurrent safety
    ///
    /// Callers must guarantee a single Engine instance per data
    /// directory. kikan's pool-level PRAGMAs (WAL + `busy_timeout`,
    /// configured in [`crate::db::pragmas`]) make each pool safe for
    /// concurrent in-process reads and writes, and migrations are
    /// serialized by [`crate::migrations::runner`] using
    /// `SqliteTransactionMode::Immediate`. None of that coordinates
    /// two Engines racing on the same data directory: sidecar swaps
    /// manipulate files outside SQLite's locking protocol; backup
    /// destination filenames are app-chosen and race at the
    /// filesystem layer; concurrent migrations serialize through the
    /// write lock but the loser fails to boot rather than cooperating.
    /// See the crate-root docs for the full contract.
    ///
    /// The [`Graft::recovery_dir`] file-drop directory and reset-PIN
    /// store are owned by the vertical on its own state slice; `boot`
    /// reaches them through [`Graft::recovery_dir`] for any kikan-side
    /// caller that needs the path.
    #[allow(clippy::too_many_arguments)]
    pub async fn boot(
        config: BootConfig,
        graft: &G,
        meta_db: DatabaseConnection,
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

        // Target-aware migration dispatch per
        // `adr-kikan-upgrade-migration-strategy.md`: meta migrations land
        // on meta.db (once per install), per-profile migrations land on
        // each per-profile pool. Cross-target deps (e.g. PerProfile→Meta)
        // are valid; the DAG resolver returns a single topological order
        // and each function filters by target.
        engine.run_meta_migrations(&meta_db).await?;

        // Boot-state detection runs *after* meta migrations (so
        // `meta.profiles` exists to be queried) and *before* per-profile
        // migrations (so a defensive-empty legacy install fails fast
        // without mutating per-profile state). See M00 shape doc §Seam 1.
        let boot_state =
            crate::meta::detect_boot_state(&engine.config.data_dir, &meta_db, graft.db_filename())
                .await?;
        dispatch_boot_state(&boot_state, &meta_db, graft, &pools).await?;

        for pool in pools.values() {
            engine.run_per_profile_migrations(pool).await?;
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

        // ── Sidecar recovery (best-effort, never blocks boot) ────────
        //
        // Offer each non-setup-wizard profile kind a chance to self-repair
        // from its bundled sidecar via [`Graft::recover_profile_sidecar`].
        // Successful recoveries are surfaced on PlatformState; failures
        // are logged and ignored — a corrupt sidecar must not gate boot.
        let sidecar_recoveries =
            maybe_repair_profile_sidecars(graft, &engine.config.data_dir, &meta_db).await;

        // ── PlatformState ────────────────────────────────────────────
        let platform = PlatformState {
            data_dir: engine.config.data_dir.clone(),
            db_filename: graft.db_filename(),
            meta_db,
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
            sidecar_recoveries: Arc::new(RwLock::new(sidecar_recoveries)),
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

    /// Build the data-plane HTTP router for `state`.
    ///
    /// Thin orchestrator: collects the graft-extracted routes, the
    /// platform-state slice, the cached SPA source, and the kikan-side
    /// session/config handles, then delegates to
    /// [`crate::data_plane::router::compose_router`] for the eight-layer
    /// middleware stack (which also owns the API 404 + SPA fallback
    /// composition).
    pub fn build_router(&self, state: G::AppState) -> Router {
        let platform = G::platform_state(&state).clone();
        let routes = G::data_plane_routes(&state);
        let inputs = crate::data_plane::router::ComposeInputs::<G::AppState, G::ProfileKind> {
            routes,
            state,
            platform,
            sessions: &self.ctx.sessions,
            config: &self.config.data_plane,
            spa_source: self.spa_source.as_deref(),
            _profile_kind: PhantomData,
        };
        crate::data_plane::router::compose_router(inputs)
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

/// Dispatch on a detected [`BootState`]: log every variant, run the legacy
/// upgrade for `LegacyCompleted`, and refuse to boot for
/// `LegacyDefensiveEmpty`. Extracted from [`Engine::boot`] so the
/// boot-orchestrator stays under the CRAP complexity threshold; the
/// per-variant logic also reads more clearly here than inside the
/// already-long boot pipeline.
///
/// `LegacyCompleted` runs the meta-only upgrade per
/// `adr-kikan-upgrade-migration-strategy.md` §Seam 2: INSERT into
/// `meta.profiles` + `meta.activity_log` inside one meta-DB transaction,
/// no on-disk rename, no `active_profile` pointer change. PR B refactors
/// the binary's `prepare_database` to consult `meta.profiles` and adds
/// the physical-truth migration (folder rename + `active_profile`
/// update); A1.2 is intentionally meta-only to avoid a "bad state on
/// main" window between PRs.
///
/// [`BootState`]: crate::meta::BootState
async fn dispatch_boot_state<G: Graft>(
    boot_state: &crate::meta::BootState,
    meta_db: &DatabaseConnection,
    graft: &G,
    pools: &HashMap<ProfileDirName, DatabaseConnection>,
) -> Result<(), EngineError> {
    match boot_state {
        crate::meta::BootState::FreshInstall => {
            tracing::info!(
                "boot-state: fresh install (no meta.profiles, no legacy profile folder)"
            );
            Ok(())
        }
        crate::meta::BootState::PostUpgradeOrSetup { profile_count } => {
            tracing::info!(
                profile_count = *profile_count,
                "boot-state: post-upgrade-or-setup (normal boot)"
            );
            Ok(())
        }
        crate::meta::BootState::LegacyAbandoned { reason } => {
            tracing::warn!(
                reason = ?reason,
                "boot-state: legacy profile folder detected but setup incomplete; \
                 treating as fresh install"
            );
            Ok(())
        }
        crate::meta::BootState::LegacyCompleted {
            vertical_db_path,
            shop_name,
        } => {
            // Resolve the auth-profile pool — this is the per-profile DB
            // that holds the pre-PR-A `users` + `roles` tables we need to
            // migrate into meta.users / meta.roles.
            let auth_kind = graft.auth_profile_kind();
            let auth_dir = validate_profile_kind::<G>(&auth_kind)?;
            let auth_pool = pools.get(&auth_dir).ok_or_else(|| {
                EngineError::Boot(format!(
                    "auth profile {auth_dir:?} has no pool entry in PlatformState pools map"
                ))
            })?;

            let kind = auth_kind.to_string();
            let outcome = crate::meta::run_legacy_upgrade(
                meta_db,
                auth_pool,
                shop_name,
                vertical_db_path,
                &kind,
            )
            .await?;
            tracing::info!(
                derived_slug = %outcome.slug,
                vertical_db_path = %vertical_db_path.display(),
                shop_name = %shop_name,
                "boot-state: legacy completed install upgraded to meta.profiles"
            );
            Ok(())
        }
        crate::meta::BootState::LegacyDefensiveEmpty { vertical_db_path } => {
            tracing::error!(
                vertical_db_path = %vertical_db_path.display(),
                "boot-state: refusing to boot — admin user(s) present but shop_name blank"
            );
            Err(EngineError::DefensiveEmptyShopName {
                path: vertical_db_path.clone(),
            })
        }
    }
}

/// Offer each non-setup-wizard profile kind a chance to self-repair from
/// its bundled sidecar via [`Graft::recover_profile_sidecar`] and return
/// the per-kind diagnostic map for [`PlatformState::sidecar_recoveries`].
///
/// Per-kind work — hook invocation, activity-log write, diagnostic
/// construction — is in [`record_sidecar_recovery`]. This loop is the
/// iteration shell.
async fn maybe_repair_profile_sidecars<G: Graft>(
    graft: &G,
    data_dir: &std::path::Path,
    meta_db: &DatabaseConnection,
) -> HashMap<ProfileDirName, crate::meta::SidecarRecoveryDiagnostic> {
    let mut diagnostics = HashMap::new();
    for kind in graft.all_profile_kinds() {
        if graft.requires_setup_wizard(kind) {
            continue;
        }
        if let Some((dir_name, diagnostic)) =
            record_sidecar_recovery::<G>(graft, kind, data_dir, meta_db).await
        {
            diagnostics.insert(dir_name, diagnostic);
        }
    }
    diagnostics
}

/// Validate the profile kind into an opaque [`ProfileDirName`], or log
/// and return `None` (skipping this kind from recovery).
///
/// Generic over `G` because [`validate_profile_kind`] consults the
/// `Display`/`FromStr` invariants on [`Graft::ProfileKind`].
fn validate_kind_for_recovery<G: Graft>(kind: &G::ProfileKind) -> Option<ProfileDirName> {
    match validate_profile_kind::<G>(kind) {
        Ok(name) => Some(name),
        Err(err) => {
            tracing::warn!(error = %err, "skip sidecar recovery: profile kind validation failed");
            None
        }
    }
}

/// Invoke [`Graft::recover_profile_sidecar`] for one kind, then dispatch
/// the outcome via [`handle_sidecar_outcome`].
///
/// Thin wrapper that exists so [`maybe_repair_profile_sidecars`] doesn't
/// have to thread the kind validation through every iteration.
async fn record_sidecar_recovery<G: Graft>(
    graft: &G,
    kind: &G::ProfileKind,
    data_dir: &std::path::Path,
    meta_db: &DatabaseConnection,
) -> Option<(ProfileDirName, crate::meta::SidecarRecoveryDiagnostic)> {
    let dir_name = validate_kind_for_recovery::<G>(kind)?;
    let outcome = graft.recover_profile_sidecar(kind, data_dir, graft.db_filename());
    let diagnostic = handle_sidecar_outcome(&dir_name, outcome, meta_db).await?;
    Some((dir_name, diagnostic))
}

/// Pure (non-generic) dispatch on a sidecar recovery outcome:
/// - `NotNeeded` → log debug, return `None`.
/// - `Recreated` → log info, write `meta.activity_log` audit row,
///   return the diagnostic.
/// - `Err(NotSupported)` → log debug, return `None`.
/// - `Err(Failed)` → log warn, return `None`.
///
/// Audit-log write failure is logged but does not suppress the
/// diagnostic — recoveries are independent and a corrupt audit pool
/// must not mask a real recovery from the operator UI.
///
/// Non-generic so unit tests can drive every arm without setting up a
/// `Graft` stub.
async fn handle_sidecar_outcome(
    dir_name: &ProfileDirName,
    outcome: Result<crate::graft::SidecarRecovery, crate::graft::SidecarRecoveryError>,
    meta_db: &DatabaseConnection,
) -> Option<crate::meta::SidecarRecoveryDiagnostic> {
    use crate::graft::{SidecarRecovery, SidecarRecoveryError};

    match outcome {
        Ok(SidecarRecovery::NotNeeded) => {
            tracing::debug!(profile_dir = %dir_name, "sidecar recovery: healthy, no copy");
            None
        }
        Ok(SidecarRecovery::Recreated {
            source,
            dest,
            bytes,
        }) => {
            let diagnostic = crate::meta::SidecarRecoveryDiagnostic {
                source: source.clone(),
                dest: dest.clone(),
                bytes,
                recovered_at: Utc::now(),
            };
            write_sidecar_audit_row(meta_db, dir_name, &source, &dest, bytes).await;
            tracing::info!(
                profile_dir = %dir_name,
                bytes,
                "sidecar recovery: recreated profile database from bundled sidecar",
            );
            Some(diagnostic)
        }
        Err(SidecarRecoveryError::NotSupported) => {
            tracing::debug!(
                profile_dir = %dir_name,
                "sidecar recovery: vertical does not bundle a sidecar for this kind",
            );
            None
        }
        Err(SidecarRecoveryError::Failed { source }) => {
            tracing::warn!(
                profile_dir = %dir_name,
                error = %source,
                "sidecar recovery: hook reported failure; continuing boot",
            );
            None
        }
    }
}

/// Write one `meta.activity_log` audit row for a successful recovery.
/// Failure is logged at warn level and swallowed — the diagnostic still
/// surfaces (see [`record_sidecar_recovery`] doc-comment).
async fn write_sidecar_audit_row(
    meta_db: &DatabaseConnection,
    dir_name: &ProfileDirName,
    source: &std::path::Path,
    dest: &std::path::Path,
    bytes: u64,
) {
    let payload = json!({
        "source": source.display().to_string(),
        "dest": dest.display().to_string(),
        "bytes": bytes,
    });
    if let Err(err) = crate::activity::insert_activity_log_raw(
        meta_db,
        "profile",
        dir_name.as_str(),
        kikan_types::activity::ActivityAction::ProfileSidecarRecovered,
        "system",
        "system",
        &payload,
    )
    .await
    {
        tracing::warn!(
            profile_dir = %dir_name,
            error = %err,
            "sidecar recovery: failed to write meta.activity_log entry; \
             diagnostic still surfaced",
        );
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

#[cfg(test)]
mod sidecar_recovery_tests {
    //! Drive every arm of [`handle_sidecar_outcome`] directly. The fn is
    //! intentionally non-generic so tests don't need a Graft stub.

    use super::*;
    use crate::graft::{SidecarRecovery, SidecarRecoveryError};
    use sea_orm::ConnectionTrait;

    async fn empty_meta_db() -> sea_orm::DatabaseConnection {
        sea_orm::Database::connect("sqlite::memory:").await.unwrap()
    }

    /// In-memory meta DB with the `activity_log` table created so
    /// [`write_sidecar_audit_row`] can succeed. Mirrors the schema kikan
    /// applies via `m_0003_create_meta_activity_log`.
    async fn meta_db_with_activity_log() -> sea_orm::DatabaseConnection {
        let conn = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        conn.execute_unprepared(
            "CREATE TABLE activity_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                action TEXT NOT NULL,
                actor_id TEXT NOT NULL DEFAULT 'system',
                actor_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .await
        .unwrap();
        conn
    }

    fn dir() -> ProfileDirName {
        ProfileDirName::from("demo")
    }

    #[tokio::test]
    async fn not_needed_returns_none() {
        let meta = empty_meta_db().await;
        let got = handle_sidecar_outcome(&dir(), Ok(SidecarRecovery::NotNeeded), &meta).await;
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn not_supported_returns_none() {
        let meta = empty_meta_db().await;
        let got =
            handle_sidecar_outcome(&dir(), Err(SidecarRecoveryError::NotSupported), &meta).await;
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn failed_returns_none_and_continues() {
        let meta = empty_meta_db().await;
        let got = handle_sidecar_outcome(
            &dir(),
            Err(SidecarRecoveryError::Failed {
                source: "synthetic".into(),
            }),
            &meta,
        )
        .await;
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn recreated_writes_audit_and_returns_diagnostic() {
        let meta = meta_db_with_activity_log().await;
        let outcome = Ok(SidecarRecovery::Recreated {
            source: std::path::PathBuf::from("/tmp/src"),
            dest: std::path::PathBuf::from("/tmp/dest"),
            bytes: 42,
        });
        let got = handle_sidecar_outcome(&dir(), outcome, &meta).await;
        let diag = got.expect("Recreated yields a diagnostic");
        assert_eq!(diag.bytes, 42);
        assert_eq!(diag.dest.as_os_str(), "/tmp/dest");
        assert_eq!(diag.source.as_os_str(), "/tmp/src");

        use sea_orm::{DbBackend, Statement};
        let row = meta
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT entity_type, entity_id, action FROM activity_log \
                 WHERE entity_id = ? AND action = 'profile_sidecar_recovered'",
                ["demo".into()],
            ))
            .await
            .unwrap()
            .expect("audit row present");
        assert_eq!(row.try_get_by_index::<String>(0).unwrap(), "profile");
        assert_eq!(row.try_get_by_index::<String>(1).unwrap(), "demo");
        assert_eq!(
            row.try_get_by_index::<String>(2).unwrap(),
            "profile_sidecar_recovered"
        );
    }

    #[tokio::test]
    async fn recreated_audit_failure_still_yields_diagnostic() {
        // Empty meta DB → activity_log table missing → write fails → swallowed.
        let meta = empty_meta_db().await;
        let outcome = Ok(SidecarRecovery::Recreated {
            source: std::path::PathBuf::from("/tmp/src"),
            dest: std::path::PathBuf::from("/tmp/dest"),
            bytes: 7,
        });
        let got = handle_sidecar_outcome(&dir(), outcome, &meta).await;
        let diag = got.expect("Recreated yields a diagnostic even when audit insert fails");
        assert_eq!(diag.bytes, 7);
    }
}
