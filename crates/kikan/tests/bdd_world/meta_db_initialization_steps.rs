use cucumber::{given, then, when};
use kikan::migrations::runner;
use kikan::{Migration, MigrationTarget};
use sea_orm::{DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::KikanWorld;

pub struct MetaDbInitCtx {
    pub data_dir: tempfile::TempDir,
    pub meta_db: Option<DatabaseConnection>,
    pub meta_migrations_applied: Vec<MigrationRow>,
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct MigrationRow {
    pub graft_id: String,
    pub name: String,
    pub applied_at: i64,
}

impl MetaDbInitCtx {
    pub fn meta_db_path(&self) -> PathBuf {
        self.data_dir.path().join("meta.db")
    }
}

async fn table_exists(db: &DatabaseConnection, table: &str) -> bool {
    #[derive(Debug, FromQueryResult)]
    struct CountRow {
        cnt: i64,
    }

    let rows: Vec<CountRow> = CountRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        format!("SELECT COUNT(*) as cnt FROM sqlite_master WHERE type='table' AND name='{table}'"),
    ))
    .all(db)
    .await
    .unwrap_or_default();

    rows.first().is_some_and(|r| r.cnt > 0)
}

async fn open_pool(path: &Path) -> DatabaseConnection {
    let url = format!("sqlite://{}?mode=rwc", path.display());
    kikan::db::initialize_database(&url).await.unwrap()
}

fn engine_platform_migrations() -> Vec<Arc<dyn Migration>> {
    let self_graft = kikan::SelfGraft;
    use kikan::SubGraft;
    self_graft.migrations().into_iter().map(Arc::from).collect()
}

#[given("a fresh data directory with no meta.db")]
async fn fresh_data_dir(w: &mut KikanWorld) {
    let dir = tempfile::tempdir().unwrap();
    assert!(!dir.path().join("meta.db").exists());
    w.meta_init = Some(MetaDbInitCtx {
        data_dir: dir,
        meta_db: None,
        meta_migrations_applied: Vec::new(),
    });
}

#[when("the engine boots")]
async fn engine_boots(w: &mut KikanWorld) {
    let ctx = w.meta_init.as_mut().unwrap();
    let pool = open_pool(&ctx.meta_db_path()).await;

    let migrations = engine_platform_migrations();
    runner::run_migrations_for_target(&pool, &migrations, MigrationTarget::Meta)
        .await
        .expect("run Meta migrations");

    let applied: Vec<MigrationRow> = MigrationRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT graft_id, name, applied_at FROM kikan_migrations ORDER BY name",
    ))
    .all(&pool)
    .await
    .unwrap();
    ctx.meta_migrations_applied = applied;
    ctx.meta_db = Some(pool);
}

#[then("a meta.db file exists at the data directory top level")]
async fn meta_db_file_exists(w: &mut KikanWorld) {
    let ctx = w.meta_init.as_ref().unwrap();
    assert!(
        ctx.meta_db_path().exists(),
        "meta.db not found at {}",
        ctx.meta_db_path().display()
    );
}

#[then("meta.db contains a kikan_migrations table")]
async fn kikan_migrations_table(w: &mut KikanWorld) {
    let ctx = w.meta_init.as_ref().unwrap();
    let db = ctx.meta_db.as_ref().unwrap();
    assert!(
        table_exists(db, "kikan_migrations").await,
        "kikan_migrations table missing on meta.db"
    );
}

#[then("meta.db contains a kikan_meta table")]
async fn kikan_meta_table(w: &mut KikanWorld) {
    let ctx = w.meta_init.as_ref().unwrap();
    let db = ctx.meta_db.as_ref().unwrap();
    assert!(
        table_exists(db, "kikan_meta").await,
        "kikan_meta table missing on meta.db"
    );
}

#[then("meta.db contains a profiles table")]
async fn profiles_table(w: &mut KikanWorld) {
    let ctx = w.meta_init.as_ref().unwrap();
    let db = ctx.meta_db.as_ref().unwrap();
    assert!(
        table_exists(db, "profiles").await,
        "profiles table missing on meta.db"
    );
}

#[then(
    "meta.db kikan_migrations records the engine-platform migrations under graft_id \"kikan::engine\""
)]
async fn engine_platform_migrations_recorded(w: &mut KikanWorld) {
    let ctx = w.meta_init.as_ref().unwrap();
    let engine_rows: Vec<&MigrationRow> = ctx
        .meta_migrations_applied
        .iter()
        .filter(|r| r.graft_id == "kikan::engine")
        .collect();

    let names: std::collections::HashSet<&str> =
        engine_rows.iter().map(|r| r.name.as_str()).collect();

    let expected: &[&str] = &[
        "m20260327_000000_users_and_roles",
        "m20260424_000000_profile_user_roles",
        "m20260424_000001_prevent_last_admin_deactivation",
        "m20260424_000002_active_integrations",
        "m20260424_000003_integration_event_log",
    ];

    for name in expected {
        assert!(
            names.contains(name),
            "expected engine-platform migration {name} under graft_id 'kikan::engine'; \
             got rows: {:?}",
            ctx.meta_migrations_applied
        );
    }

    assert!(
        engine_rows.iter().all(|r| r.applied_at > 0),
        "applied_at should be unix-epoch timestamp"
    );
}
