//! `MokumoApp: Graft` — the Mokumo application fused to the kikan engine.
//!
//! Owns the per-profile migration set and lifecycle hooks. `build_domain_state`
//! constructs `MokumoShopState`; `compose_state` assembles the full
//! `MokumoState` from platform + control-plane + domain slices.
//!
//! `data_plane_routes` returns the full domain route tree via
//! `crate::routes::data_plane_routes`. `spawn_background_tasks` runs
//! the PIN sweep (60s) and PRAGMA optimize (2h + shutdown) tasks.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use kikan::control_plane::SetupTokenSource;
use kikan::data_plane::spa::SpaSource;
use kikan::migrations::conn::MigrationConn;
use kikan::rate_limit::RateLimiter;
use kikan::{EngineContext, EngineError, Graft, GraftId, Migration, MigrationRef, MigrationTarget};
use kikan_types::SetupMode;
use sea_orm_migration::MigratorTrait;
use sea_orm_migration::sea_orm::DbErr;

use crate::migrations::Migrator;
use crate::state::{MokumoShopState, MokumoState, SharedMokumoState};
use crate::ws::ConnectionManager;

const MOKUMO_GRAFT_ID: GraftId = GraftId::new("mokumo");

static MOKUMO_PROFILE_KINDS: &[SetupMode] = &[SetupMode::Demo, SetupMode::Production];

/// The Mokumo application grafted onto the kikan engine.
///
/// Carries the first-admin setup token (resolved once by the caller at
/// startup from [`crate::startup::init_session_and_setup`]) so that
/// [`Graft::setup_token_source`] can hand it to the engine at boot.
/// Once setup completes the token is `None` and the setup-wizard gate
/// rejects every caller.
///
/// Optionally carries an explicit recovery-dir path — callers that
/// want a deterministic location (tests, managed deployments) pass one
/// through [`MokumoApp::with_recovery_dir`]. When unset, the graft's
/// `recovery_dir` hook and `build_domain_state` both fall back to
/// [`crate::startup::resolve_recovery_dir`] (env var → Desktop → cwd).
/// Factory for SPA sources. `Graft::spa_source` returns a fresh
/// `Box<dyn SpaSource>` on every call (the trait signature), so the
/// graft holds a factory closure rather than a single boxed instance.
///
/// The factory is `Arc`-wrapped so `MokumoApp` stays `'static`-friendly
/// — multiple clones of the graft (in tests, or across background task
/// spawns) share the same factory without a generic parameter.
type SpaSourceFactory = Arc<dyn Fn() -> Box<dyn SpaSource> + Send + Sync + 'static>;

pub struct MokumoApp {
    setup_token: Option<Arc<str>>,
    recovery_dir_override: Option<Arc<std::path::PathBuf>>,
    spa_source_factory: Option<SpaSourceFactory>,
}

impl MokumoApp {
    /// Construct a `MokumoApp` with a resolved setup-token.
    ///
    /// Pass `None` when setup has already completed (the wizard gate is
    /// permanently closed) or in contexts that never reach the wizard
    /// (CLI reset-db / restore / tests).
    pub fn new(setup_token: Option<Arc<str>>) -> Self {
        Self {
            setup_token,
            recovery_dir_override: None,
            spa_source_factory: None,
        }
    }

    /// Override the default recovery-dir resolution. Useful for tests
    /// that want a per-test-case tempdir, and for deployments that want
    /// a deterministic path without relying on `MOKUMO_RECOVERY_DIR`.
    pub fn with_recovery_dir(mut self, path: std::path::PathBuf) -> Self {
        self.recovery_dir_override = Some(Arc::new(path));
        self
    }

    /// Inject an SPA fallback factory. The desktop binary supplies an
    /// embedded (`rust-embed`) source; the headless server supplies a
    /// disk-backed source when `--spa-dir` is set. Tests and API-only
    /// deployments leave this unset, in which case non-API paths return
    /// Axum's default 404.
    ///
    /// The factory is invoked once at engine construction
    /// ([`kikan::Engine::new_with`]); the produced `Box<dyn SpaSource>`
    /// is cached for the lifetime of the router.
    pub fn with_spa_source<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> Box<dyn SpaSource> + Send + Sync + 'static,
    {
        self.spa_source_factory = Some(Arc::new(factory));
        self
    }

    /// Resolve the effective recovery directory — the explicit override
    /// if set, otherwise [`crate::startup::resolve_recovery_dir`].
    ///
    /// Public so the binary can capture the dir at boot time when
    /// constructing the kikan `RecoveryArtifactWriter` closure (the
    /// closure cannot reach into `&self` once boxed).
    pub fn effective_recovery_dir(&self) -> std::path::PathBuf {
        match &self.recovery_dir_override {
            Some(p) => (**p).clone(),
            None => crate::startup::resolve_recovery_dir(),
        }
    }
}

impl Default for MokumoApp {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Graft for MokumoApp {
    type AppState = SharedMokumoState;
    type DomainState = MokumoShopState;
    type ProfileKind = SetupMode;

    fn id() -> GraftId {
        MOKUMO_GRAFT_ID
    }

    fn db_filename(&self) -> &'static str {
        "mokumo.db"
    }

    fn all_profile_kinds(&self) -> &'static [SetupMode] {
        MOKUMO_PROFILE_KINDS
    }

    fn default_profile_kind(&self) -> SetupMode {
        SetupMode::Demo
    }

    fn requires_setup_wizard(&self, kind: &SetupMode) -> bool {
        matches!(kind, SetupMode::Production)
    }

    fn auth_profile_kind(&self) -> SetupMode {
        SetupMode::Production
    }

    fn recover_profile_sidecar(
        &self,
        kind: &SetupMode,
        data_dir: &std::path::Path,
        db_filename: &'static str,
    ) -> Result<kikan::SidecarRecovery, kikan::SidecarRecoveryError> {
        // Mokumo bundles a sidecar only for the demo profile — the
        // primary profile starts blank and is filled by the setup wizard.
        if !matches!(kind, SetupMode::Demo) {
            return Err(kikan::SidecarRecoveryError::NotSupported);
        }

        let dest = data_dir.join(kind.as_str()).join(db_filename);

        // Skip recovery if the destination database is present and passes
        // the kikan application_id guard (guard 1 from
        // adr-database-startup-guard-chain.md). Guards 2 + 3 (auto_vacuum,
        // schema compatibility) run later in setup_profile_db on the
        // re-opened pool — those failures are surfaced as boot errors,
        // not silently repaired here. Sidecar recovery exists for the
        // missing-file and corrupt-application_id cases.
        if dest.try_exists().unwrap_or(false) && kikan::db::check_application_id(&dest).is_ok() {
            return Ok(kikan::SidecarRecovery::NotNeeded);
        }

        // `force_copy_sidecar` resolves the source via MOKUMO_DEMO_SIDECAR or
        // the binary-adjacent `demo.db`, copies it over the destination, and
        // returns the *resolved source path* — so the diagnostic below
        // describes the exact file the copy consumed (no double-resolution,
        // no TOCTOU window). Returning `Failed` here preserves the
        // best-effort contract: a missing sidecar means we couldn't
        // self-repair, but boot continues and the operator sees the
        // existing-but-corrupt file (which the per-profile setup will
        // surface separately).
        let source = crate::demo_reset::force_copy_sidecar(data_dir, db_filename).map_err(|e| {
            kikan::SidecarRecoveryError::Failed {
                source: Box::new(e),
            }
        })?;

        let bytes = std::fs::metadata(&dest).map(|m| m.len()).map_err(|e| {
            kikan::SidecarRecoveryError::Failed {
                source: Box::new(e),
            }
        })?;

        Ok(kikan::SidecarRecovery::Recreated {
            source,
            dest,
            bytes,
        })
    }

    fn migrations(&self) -> Vec<Box<dyn Migration>> {
        let seaorm_migrations = Migrator::migrations();
        let names: Vec<&'static str> = vec![
            "m20260321_000000_init",
            "m20260322_000000_settings",
            "m20260324_000000_number_sequences",
            "m20260324_000001_customers_and_activity",
            "m20260326_000000_customers_deleted_at_index",
            "m20260404_000000_set_pragmas",
            "m20260416_000000_login_lockout",
            "m20260418_000000_activity_log_composite_index",
        ];

        const KIKAN_ENGINE_GRAFT_ID: GraftId = GraftId::new("kikan::engine");
        let login_lockout_cross_graft_dep = MigrationRef {
            graft: KIKAN_ENGINE_GRAFT_ID,
            name: "m20260327_000000_users_and_roles",
        };

        seaorm_migrations
            .into_iter()
            .enumerate()
            .map(|(i, m)| {
                let mut deps: Vec<MigrationRef> = Vec::new();

                if i > 0 {
                    deps.push(MigrationRef {
                        graft: MOKUMO_GRAFT_ID,
                        name: names[i - 1],
                    });
                }

                if names[i] == "m20260416_000000_login_lockout" {
                    deps.push(login_lockout_cross_graft_dep.clone());
                }

                Box::new(BridgedSeaOrmMigration {
                    inner: m,
                    name: names[i],
                    deps,
                }) as Box<dyn Migration>
            })
            .collect()
    }

    async fn build_domain_state(
        &self,
        _ctx: &EngineContext,
    ) -> Result<Self::DomainState, EngineError> {
        Ok(MokumoShopState {
            ws: Arc::new(ConnectionManager::new(64)),
            local_ip: Arc::new(parking_lot::RwLock::new(local_ip_address::local_ip().ok())),
            restore_in_progress: Arc::new(AtomicBool::new(false)),
            restore_limiter: Arc::new(RateLimiter::new(5, std::time::Duration::from_hours(1))),
            recovery_dir: Arc::new(self.effective_recovery_dir()),
            #[cfg(debug_assertions)]
            ws_ping_ms: None,
        })
    }

    fn compose_state(
        control_plane: kikan::ControlPlaneState,
        domain: Self::DomainState,
    ) -> Self::AppState {
        Arc::new(MokumoState {
            control_plane,
            domain,
        })
    }

    fn platform_state(state: &Self::AppState) -> &kikan::PlatformState {
        &state.control_plane.platform
    }

    fn control_plane_state(state: &Self::AppState) -> &kikan::ControlPlaneState {
        &state.control_plane
    }

    fn data_plane_routes(state: &Self::AppState) -> axum::Router<Self::AppState> {
        crate::routes::data_plane_routes(state)
    }

    fn spawn_background_tasks(&self, state: &Self::AppState) {
        // Background task: refresh local IP every 30s.
        {
            let local_ip = state.local_ip().clone();
            let token = state.shutdown().clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                interval.tick().await; // skip immediate first tick (set at boot)
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let current = match local_ip_address::local_ip() {
                                Ok(ip) => Some(ip),
                                Err(err) => {
                                    tracing::debug!(error = %err, "local_ip lookup failed; keeping last known value");
                                    continue;
                                }
                            };
                            let mut guard = local_ip.write();
                            if *guard != current {
                                *guard = current;
                            }
                        }
                        () = token.cancelled() => break,
                    }
                }
            });
        }

        // Background task: run PRAGMA optimize every 2 hours and once on graceful shutdown.
        {
            let demo_pool = state.demo_db().get_sqlite_connection_pool().clone();
            let prod_pool = state.production_db().get_sqlite_connection_pool().clone();
            let token = state.shutdown().clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_hours(2));
                interval.tick().await; // skip immediate first tick
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            for pool in [&demo_pool, &prod_pool] {
                                if let Err(e) = sqlx::query("PRAGMA optimize(0xfffe)")
                                    .execute(pool)
                                    .await
                                {
                                    tracing::warn!("periodic PRAGMA optimize failed: {e}");
                                }
                            }
                        }
                        () = token.cancelled() => {
                            for pool in [&demo_pool, &prod_pool] {
                                if let Err(e) = sqlx::query("PRAGMA optimize(0xfffe)")
                                    .execute(pool)
                                    .await
                                {
                                    tracing::warn!("shutdown PRAGMA optimize failed: {e}");
                                }
                            }
                            break;
                        }
                    }
                }
            });
        }
    }

    fn on_backup_created(
        &self,
        db_path: &std::path::Path,
        backup_path: &std::path::Path,
    ) -> Result<(), String> {
        crate::lifecycle::copy_logo_to_backup(db_path, backup_path);
        Ok(())
    }

    fn on_post_restore(
        &self,
        db_path: &std::path::Path,
        backup_path: &std::path::Path,
    ) -> Result<(), String> {
        crate::lifecycle::restore_logo_from_backup(db_path, backup_path);
        Ok(())
    }

    fn on_post_reset_db(&self, profile_dir: &std::path::Path) -> Result<(), String> {
        crate::lifecycle::cleanup_domain_artifacts(profile_dir);
        // Mokumo owns the recovery-file layout (`mokumo-recovery-*.html`
        // under `recovery_dir`); the cleanup moved out of `kikan-cli` in
        // Session 3. The hook resolves its own recovery dir via the
        // same path the engine hook uses.
        let recovery_dir = self.effective_recovery_dir();
        if let Err(e) = crate::lifecycle::cleanup_recovery_files(&recovery_dir) {
            return Err(e.to_string());
        }
        Ok(())
    }

    fn recovery_dir(
        &self,
        _profile_id: &kikan::ProfileId<SetupMode>,
    ) -> Option<std::path::PathBuf> {
        // Mokumo's recovery-file layout is profile-agnostic in M0: a
        // single directory shared across Demo and Production. The
        // parameter is accepted for future per-profile layouts without
        // widening the seam later.
        Some(self.effective_recovery_dir())
    }

    fn setup_token_source(&self) -> SetupTokenSource {
        match &self.setup_token {
            Some(t) => SetupTokenSource::Inline(t.clone()),
            None => SetupTokenSource::Disabled,
        }
    }

    fn spa_source(&self) -> Option<Box<dyn SpaSource>> {
        self.spa_source_factory.as_ref().map(|f| f())
    }

    // `valid_reset_pin_ids` keeps the default empty slice — mokumo's
    // reset flow uses email as the PIN lookup key and has no concept of
    // "valid PIN ids" today. The hook stays available for future
    // verticals that want kikan-side PIN-id gating.
}

struct BridgedSeaOrmMigration {
    inner: Box<dyn sea_orm_migration::MigrationTrait + Send + Sync>,
    name: &'static str,
    deps: Vec<MigrationRef>,
}

#[async_trait::async_trait]
impl Migration for BridgedSeaOrmMigration {
    fn name(&self) -> &'static str {
        self.name
    }

    fn graft_id(&self) -> GraftId {
        MOKUMO_GRAFT_ID
    }

    fn target(&self) -> MigrationTarget {
        MigrationTarget::PerProfile
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        self.deps.clone()
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), DbErr> {
        let manager = conn.schema_manager();
        self.inner.up(&manager).await
    }
}
