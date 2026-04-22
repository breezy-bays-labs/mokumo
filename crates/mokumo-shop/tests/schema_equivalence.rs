use kikan::{BootConfig, Engine};
use mokumo_shop::graft::MokumoApp;
use mokumo_shop::migrations::Migrator;
use sea_orm::{Database, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use sea_orm_migration::MigratorTrait;
use sea_orm_migration::sea_orm;
use tower_sessions_sqlx_store::SqliteStore;

async fn test_session_store(db: &DatabaseConnection) -> SqliteStore {
    let store = SqliteStore::new(db.get_sqlite_connection_pool().clone());
    store.migrate().await.unwrap();
    store
}

#[derive(Debug, FromQueryResult, PartialEq, Eq)]
struct MasterRow {
    sql: Option<String>,
}

async fn get_app_schema(db: &DatabaseConnection) -> Vec<String> {
    let rows: Vec<MasterRow> = MasterRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT sql FROM sqlite_master WHERE type IN ('table', 'index', 'trigger') AND sql IS NOT NULL AND name NOT LIKE 'kikan_%' AND name != 'seaql_migrations' AND name NOT LIKE 'tower_sessions%' ORDER BY name",
    ))
    .all(db)
    .await
    .unwrap();
    rows.into_iter().filter_map(|r| r.sql).collect()
}

#[tokio::test]
async fn kikan_engine_produces_identical_app_schema_to_legacy_migrator() {
    let tmp = tempfile::tempdir().unwrap();

    let legacy_path = tmp.path().join("legacy.db");
    let legacy_url = format!("sqlite:{}?mode=rwc", legacy_path.display());
    let legacy_db = Database::connect(&legacy_url).await.unwrap();
    // Platform tables (users, roles, shop_settings) are now owned by kikan.
    // Run them first so vertical migrations that ALTER TABLE users succeed.
    kikan::migrations::platform::run_platform_migrations(&legacy_db)
        .await
        .unwrap();
    Migrator::up(&legacy_db, None).await.unwrap();
    let legacy_schema = get_app_schema(&legacy_db).await;
    drop(legacy_db);

    let kikan_path = tmp.path().join("kikan.db");
    let kikan_url = format!("sqlite:{}?mode=rwc", kikan_path.display());
    let kikan_db = Database::connect(&kikan_url).await.unwrap();
    let store = test_session_store(&kikan_db).await;
    let graft = MokumoApp::default();
    let config = BootConfig::new(tmp.path().to_path_buf());
    let engine = Engine::new(config, &graft, kikan_db.clone(), store).unwrap();
    engine.run_migrations(&kikan_db).await.unwrap();
    let kikan_schema = get_app_schema(&kikan_db).await;
    drop(kikan_db);

    assert_eq!(
        legacy_schema.len(),
        kikan_schema.len(),
        "app schema count mismatch: legacy={}, kikan={}",
        legacy_schema.len(),
        kikan_schema.len()
    );

    for (i, (legacy, kikan)) in legacy_schema.iter().zip(kikan_schema.iter()).enumerate() {
        assert_eq!(
            legacy, kikan,
            "schema mismatch at index {i}:\nlegacy: {legacy}\nkikan:  {kikan}"
        );
    }
}

#[tokio::test]
async fn mokumo_app_backfill_preserves_seaql_table() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("backfill.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = Database::connect(&url).await.unwrap();

    // Platform tables run first so vertical migrations succeed.
    kikan::migrations::platform::run_platform_migrations(&db)
        .await
        .unwrap();
    Migrator::up(&db, None).await.unwrap();

    let store = test_session_store(&db).await;
    let graft = MokumoApp::default();
    let config = BootConfig::new(tmp.path().to_path_buf());
    let engine = Engine::new(config, &graft, db.clone(), store).unwrap();

    engine.run_migrations(&db).await.unwrap();

    #[derive(Debug, FromQueryResult)]
    struct CountRow {
        cnt: i64,
    }

    let seaql: Vec<CountRow> = CountRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT COUNT(*) as cnt FROM seaql_migrations",
    ))
    .all(&db)
    .await
    .unwrap();
    assert!(seaql[0].cnt > 0, "seaql_migrations should be preserved");

    let kikan_rows: Vec<CountRow> = CountRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT COUNT(*) as cnt FROM kikan_migrations WHERE graft_id = 'mokumo'",
    ))
    .all(&db)
    .await
    .unwrap();
    assert!(
        kikan_rows[0].cnt >= 8,
        "should have backfilled + bootstrap migrations"
    );
}
