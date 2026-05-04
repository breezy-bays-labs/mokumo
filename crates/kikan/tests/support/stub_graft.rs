use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use kikan::migrations::conn::MigrationConn;
use kikan::{
    BootConfig, Engine, EngineContext, EngineError, Graft, GraftId, Migration, MigrationRef,
    MigrationTarget, Tenancy,
};
use kikan_types::SetupMode;
use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

static STUB_PROFILE_KINDS: &[SetupMode] = &[SetupMode::Demo, SetupMode::Production];

/// Minimal composed state for StubGraft, mirroring the real
/// MokumoState structure but without domain fields.
#[derive(Clone)]
pub struct StubAppState {
    pub control_plane: kikan::ControlPlaneState,
}

pub struct StubGraft {
    migrations: Vec<Box<dyn Migration>>,
}

impl StubGraft {
    pub fn new(migrations: Vec<Box<dyn Migration>>) -> Self {
        Self { migrations }
    }

    pub fn diamond() -> Self {
        Self::new(vec![
            make_migration("A", vec![], MigrationTarget::PerProfile),
            make_migration("B", vec!["A"], MigrationTarget::PerProfile),
            make_migration("C", vec!["A"], MigrationTarget::PerProfile),
            make_migration("D", vec!["B", "C"], MigrationTarget::PerProfile),
        ])
    }
}

impl Graft for StubGraft {
    type AppState = StubAppState;
    type DomainState = ();
    type ProfileKind = SetupMode;

    fn id() -> GraftId {
        GraftId::new("stub")
    }

    fn db_filename(&self) -> &'static str {
        "mokumo.db"
    }

    fn all_profile_kinds(&self) -> &'static [SetupMode] {
        STUB_PROFILE_KINDS
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

    fn migrations(&self) -> Vec<Box<dyn Migration>> {
        self.migrations
            .iter()
            .map(|m| -> Box<dyn Migration> {
                make_migration(
                    m.name(),
                    m.dependencies().iter().map(|d| d.name).collect(),
                    m.target(),
                )
            })
            .collect()
    }

    async fn build_domain_state(
        &self,
        _ctx: &EngineContext,
    ) -> Result<Self::DomainState, EngineError> {
        Ok(())
    }

    fn compose_state(
        control_plane: kikan::ControlPlaneState,
        _domain: Self::DomainState,
    ) -> Self::AppState {
        StubAppState { control_plane }
    }

    fn platform_state(state: &Self::AppState) -> &kikan::PlatformState {
        &state.control_plane.platform
    }

    fn control_plane_state(state: &Self::AppState) -> &kikan::ControlPlaneState {
        &state.control_plane
    }

    fn data_plane_routes(_state: &Self::AppState) -> axum::Router<Self::AppState> {
        axum::Router::new()
    }
}

/// Build a minimal `StubAppState` for tests that need a real state
/// (e.g. `build_router`).
///
/// Callers open the meta-DB pool themselves (typically `:memory:`) and
/// pass it in — keeps this function synchronous so the existing tests
/// don't need an async wrapper.
pub fn stub_app_state(
    meta_db: sea_orm::DatabaseConnection,
    demo_db: sea_orm::DatabaseConnection,
    production_db: sea_orm::DatabaseConnection,
    data_dir: std::path::PathBuf,
) -> StubAppState {
    let demo_dir = kikan::tenancy::ProfileDirName::from(SetupMode::Demo.as_dir_name());
    let production_dir = kikan::tenancy::ProfileDirName::from(SetupMode::Production.as_dir_name());
    let mut pools = std::collections::HashMap::with_capacity(2);
    pools.insert(demo_dir.clone(), demo_db);
    pools.insert(production_dir.clone(), production_db);
    let profile_dir_names: Arc<[kikan::tenancy::ProfileDirName]> =
        vec![production_dir.clone(), demo_dir.clone()].into();
    let mut requires_setup_by_dir = std::collections::HashMap::with_capacity(2);
    requires_setup_by_dir.insert(production_dir, true);
    requires_setup_by_dir.insert(demo_dir.clone(), false);
    let platform = kikan::PlatformState {
        data_dir,
        db_filename: "mokumo.db",
        meta_db,
        pools: Arc::new(pools),
        active_profile: Arc::new(RwLock::new(demo_dir)),
        profile_dir_names,
        requires_setup_by_dir: Arc::new(requires_setup_by_dir),
        auth_profile_kind_dir: kikan::tenancy::ProfileDirName::from(
            SetupMode::Production.as_dir_name(),
        ),
        shutdown: CancellationToken::new(),
        started_at: std::time::Instant::now(),
        mdns_status: kikan::MdnsStatus::shared(),
        demo_install_ok: Arc::new(AtomicBool::new(true)),
        is_first_launch: Arc::new(AtomicBool::new(false)),
        setup_completed: Arc::new(AtomicBool::new(false)),
        profile_db_initializer: Arc::new(NoOpProfileDbInitializer),
        sidecar_recoveries: Arc::new(RwLock::new(std::collections::HashMap::new())),
        reset_pins: Arc::new(dashmap::DashMap::new()),
    };
    let control_plane = kikan::ControlPlaneState {
        platform,
        login_limiter: Arc::new(kikan::rate_limit::RateLimiter::new(
            10,
            std::time::Duration::from_mins(15),
        )),
        recovery_limiter: Arc::new(kikan::rate_limit::RateLimiter::new(
            5,
            std::time::Duration::from_mins(15),
        )),
        regen_limiter: Arc::new(kikan::rate_limit::RateLimiter::new(
            3,
            std::time::Duration::from_hours(1),
        )),
        switch_limiter: Arc::new(kikan::rate_limit::RateLimiter::new(
            3,
            std::time::Duration::from_mins(15),
        )),
        setup_token: None,
        setup_in_progress: Arc::new(AtomicBool::new(false)),
        activity_writer: Arc::new(kikan::SqliteActivityWriter::new()),
        recovery_writer: None,
    };
    StubAppState { control_plane }
}

pub struct NoOpProfileDbInitializer;

impl kikan::platform_state::ProfileDbInitializer for NoOpProfileDbInitializer {
    fn initialize<'a>(
        &'a self,
        _database_url: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<sea_orm::DatabaseConnection, kikan::db::DatabaseSetupError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(kikan::db::DatabaseSetupError::Migration(
                sea_orm::DbErr::Custom("not supported in test".to_string()),
            ))
        })
    }
}

pub fn make_migration(
    name: &'static str,
    deps: Vec<&'static str>,
    target: MigrationTarget,
) -> Box<dyn Migration> {
    Box::new(SimpleMigration {
        name,
        deps: deps.into_iter().collect(),
        target,
        sql: format!("CREATE TABLE IF NOT EXISTS test_{name} (id INTEGER PRIMARY KEY)"),
    })
}

pub fn failing_migration(name: &'static str, deps: Vec<&'static str>) -> Box<dyn Migration> {
    Box::new(SimpleMigration {
        name,
        deps,
        target: MigrationTarget::PerProfile,
        sql: "INVALID SQL STATEMENT HERE".to_string(),
    })
}

struct SimpleMigration {
    name: &'static str,
    deps: Vec<&'static str>,
    target: MigrationTarget,
    sql: String,
}

#[async_trait::async_trait]
impl Migration for SimpleMigration {
    fn name(&self) -> &'static str {
        self.name
    }

    fn graft_id(&self) -> GraftId {
        GraftId::new("stub")
    }

    fn target(&self) -> MigrationTarget {
        self.target
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        self.deps
            .iter()
            .map(|&name| MigrationRef {
                graft: GraftId::new("stub"),
                name,
            })
            .collect()
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), sea_orm::DbErr> {
        conn.execute_unprepared(&self.sql).await?;
        Ok(())
    }
}

fn _assert_graft_build_domain_state_is_send() {
    fn require_send<T: Send>(_t: T) {}
    fn inner(graft: &StubGraft, ctx: &EngineContext) {
        require_send(graft.build_domain_state(ctx));
    }
    let _ = inner;
}
