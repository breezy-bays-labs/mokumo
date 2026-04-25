use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use cucumber::{given, then, when};
use kikan::EngineError;
use sea_orm::Database;
use tokio_util::sync::CancellationToken;

use super::PlatformBddWorld;

const VERTICAL_DB_FILE: &str = "mokumo.db";

#[derive(Debug)]
pub struct LegacyRefuseCtx {
    pub data_dir: tempfile::TempDir,
    pub vertical_db_path: PathBuf,
    pub boot_result: Option<Result<(), EngineError>>,
}

fn seed_legacy_vertical(production: &std::path::Path, shop_name: &str) -> PathBuf {
    std::fs::create_dir_all(production).expect("mkdir production");
    let vertical = production.join(VERTICAL_DB_FILE);
    let conn = rusqlite::Connection::open(&vertical).expect("open legacy vertical db");
    conn.execute_batch(
        "CREATE TABLE roles (id INTEGER PRIMARY KEY, name TEXT);
         INSERT INTO roles (id, name) VALUES (1, 'Admin');
         CREATE TABLE users (
             id INTEGER PRIMARY KEY,
             role_id INTEGER NOT NULL,
             is_active INTEGER NOT NULL DEFAULT 1,
             deleted_at TEXT
         );
         INSERT INTO users (role_id, is_active, deleted_at) VALUES (1, 1, NULL);
         CREATE TABLE shop_settings (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             shop_name TEXT NOT NULL DEFAULT ''
         );",
    )
    .expect("seed legacy vertical schema");
    conn.execute(
        "INSERT INTO shop_settings (id, shop_name) VALUES (1, ?1)",
        rusqlite::params![shop_name],
    )
    .expect("seed shop_settings row");
    vertical
}

#[given("a legacy production database with an admin user and an empty shop_name")]
async fn given_legacy_empty_shop_name(w: &mut PlatformBddWorld) {
    let dir = tempfile::tempdir().unwrap();
    let vertical = seed_legacy_vertical(&dir.path().join("production"), "");
    w.legacy_refuse = Some(LegacyRefuseCtx {
        data_dir: dir,
        vertical_db_path: vertical,
        boot_result: None,
    });
}

#[when("the engine boots")]
async fn when_engine_boots(w: &mut PlatformBddWorld) {
    use kikan::tenancy::ProfileDirName;
    use kikan_types::SetupMode;

    let ctx = w.legacy_refuse.as_mut().unwrap();
    let data_dir = ctx.data_dir.path().to_path_buf();

    let meta_db = Database::connect("sqlite::memory:").await.unwrap();
    let demo_db = Database::connect("sqlite::memory:").await.unwrap();
    let production_db = Database::connect("sqlite::memory:").await.unwrap();

    let session_pool = production_db.get_sqlite_connection_pool().clone();
    let session_store = tower_sessions_sqlx_store::SqliteStore::new(session_pool);
    session_store.migrate().await.unwrap();

    let mut pools = std::collections::HashMap::with_capacity(2);
    pools.insert(ProfileDirName::from(SetupMode::Demo.as_dir_name()), demo_db);
    pools.insert(
        ProfileDirName::from(SetupMode::Production.as_dir_name()),
        production_db,
    );
    let active_profile = ProfileDirName::from(SetupMode::Production.as_dir_name());

    let recovery_dir = data_dir.join("recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();

    let graft = mokumo_shop::graft::MokumoApp::new(None).with_recovery_dir(recovery_dir);
    let profile_initializer: kikan::platform_state::SharedProfileDbInitializer =
        Arc::new(mokumo_shop::profile_db_init::MokumoProfileDbInitializer);
    let boot_config = kikan::BootConfig::new(data_dir);

    let result = kikan::Engine::<mokumo_shop::graft::MokumoApp>::boot(
        boot_config,
        &graft,
        meta_db,
        pools,
        active_profile,
        session_store,
        profile_initializer,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(true)),
        CancellationToken::new(),
    )
    .await
    .map(|_| ());
    ctx.boot_result = Some(result);
}

#[then("the engine refuses to boot with DefensiveEmptyShopName pointing at the production db")]
async fn then_refused_with_defensive_empty(w: &mut PlatformBddWorld) {
    let ctx = w.legacy_refuse.as_ref().unwrap();
    let result = ctx.boot_result.as_ref().expect("boot was invoked");
    let err = result
        .as_ref()
        .expect_err("expected Engine::boot to fail with DefensiveEmptyShopName");
    let EngineError::DefensiveEmptyShopName { path } = err else {
        panic!("expected DefensiveEmptyShopName, got {err:?}");
    };
    assert_eq!(path, &ctx.vertical_db_path);
}
