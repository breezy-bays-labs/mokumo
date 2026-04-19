#[path = "support/mod.rs"]
mod support;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use kikan::{BootConfig, Engine, EngineError, SetupMode};
use sea_orm::{Database, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use support::{StubGraft, stub_app_state};
use tower_sessions_sqlx_store::SqliteStore;

async fn in_memory_db() -> DatabaseConnection {
    Database::connect("sqlite::memory:").await.unwrap()
}

async fn test_runtime() -> (DatabaseConnection, SqliteStore) {
    let pool = in_memory_db().await;
    let sqlx_pool = pool.get_sqlite_connection_pool().clone();
    let store = SqliteStore::new(sqlx_pool);
    store.migrate().await.unwrap();
    (pool, store)
}

#[tokio::test]
async fn build_router_composes_layers_and_serves_404() {
    use axum::body::Body;
    use http::{Request, StatusCode};
    use tower::util::ServiceExt;

    let graft = StubGraft::diamond();
    let config = BootConfig::new(std::path::PathBuf::from("/tmp/test-engine-router"));
    let (pool, store) = test_runtime().await;
    let demo_db = in_memory_db().await;
    let engine = Engine::new(config, &graft, pool.clone(), store).unwrap();

    let state = stub_app_state(demo_db, pool, "/tmp/test-engine-router".into());
    let router = engine.build_router(state);
    let request = Request::builder()
        .uri("/unknown")
        .header("host", "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn build_router_rejects_disallowed_host() {
    use axum::body::Body;
    use http::{Request, StatusCode};
    use tower::util::ServiceExt;

    let graft = StubGraft::diamond();
    let config = BootConfig::new(std::path::PathBuf::from("/tmp/test-engine-router-host"));
    let (pool, store) = test_runtime().await;
    let demo_db = in_memory_db().await;
    let engine = Engine::new(config, &graft, pool.clone(), store).unwrap();

    let state = stub_app_state(demo_db, pool, "/tmp/test-engine-router-host".into());
    let router = engine.build_router(state);
    let request = Request::builder()
        .uri("/unknown")
        .header("host", "evil.com")
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn engine_new_collects_graft_and_subgraft_migrations() {
    let graft = StubGraft::diamond();
    let config = BootConfig::new(std::path::PathBuf::from("/tmp/test-engine"));
    let (pool, store) = test_runtime().await;
    let engine = Engine::new(config, &graft, pool, store).unwrap();

    let tenancy = engine.tenancy();
    assert_eq!(tenancy.data_dir(), std::path::Path::new("/tmp/test-engine"));
}

#[tokio::test]
async fn engine_run_migrations_applies_all_to_db() {
    let graft = StubGraft::diamond();
    let config = BootConfig::new(std::path::PathBuf::from("/tmp/test-engine"));
    let (pool, store) = test_runtime().await;
    let engine = Engine::new(config, &graft, pool, store).unwrap();

    let db = in_memory_db().await;
    engine.run_migrations(&db).await.unwrap();

    #[derive(Debug, FromQueryResult)]
    struct CountRow {
        cnt: i64,
    }

    let stub_count: Vec<CountRow> = CountRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT COUNT(*) as cnt FROM kikan_migrations WHERE graft_id = 'stub'",
    ))
    .all(&db)
    .await
    .unwrap();
    assert_eq!(stub_count[0].cnt, 4);

    let table_count: Vec<CountRow> = CountRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT COUNT(*) as cnt FROM sqlite_master WHERE type='table' AND name LIKE 'test_%'",
    ))
    .all(&db)
    .await
    .unwrap();
    assert_eq!(table_count[0].cnt, 4);
}

#[tokio::test]
async fn profile_id_display_and_setup_mode_roundtrip() {
    use kikan::{ProfileId, SetupMode};

    let demo = SetupMode::Demo;
    assert_eq!(demo.to_string(), "demo");
    assert_eq!("demo".parse::<SetupMode>().unwrap(), SetupMode::Demo);

    let prod = SetupMode::Production;
    assert_eq!(prod.to_string(), "production");
    assert_eq!(
        "production".parse::<SetupMode>().unwrap(),
        SetupMode::Production
    );

    let pid = ProfileId::new(SetupMode::Demo);
    assert_eq!(pid.to_string(), "demo");
    assert_eq!(pid.get(), SetupMode::Demo);

    assert!("invalid".parse::<SetupMode>().is_err());
}

/// Folded into a single test because `MOKUMO_DATA_DIR` is process-global.
/// Running "unset then read" and "set then read" as two parallel `#[tokio::test]`s
/// in the same binary races: the set from one leaks into the unset side of the
/// other, surfacing as an intermittent failure on CI (#594 seam-check).
/// Asserting both paths sequentially here makes the contract race-free without
/// pulling in a cross-test mutex crate.
#[tokio::test]
async fn headless_from_args_env_data_dir_contract() {
    // SAFETY: test-only; this is the only test that reads MOKUMO_DATA_DIR.
    unsafe { std::env::remove_var("MOKUMO_DATA_DIR") };

    // Unset → error pointing at the missing env var.
    let err = BootConfig::headless_from_args().unwrap_err();
    assert!(matches!(err, EngineError::Boot(_)));
    assert!(err.to_string().contains("MOKUMO_DATA_DIR"));

    // Set → data_dir echoes the env var and bind_addr defaults.
    // SAFETY: test-only; cleared at the end of this test.
    unsafe { std::env::set_var("MOKUMO_DATA_DIR", "/tmp/kikan-test-headless") };
    let config = BootConfig::headless_from_args().unwrap();
    // SAFETY: restore the clean baseline for any future test.
    unsafe { std::env::remove_var("MOKUMO_DATA_DIR") };
    assert_eq!(
        config.data_dir,
        std::path::PathBuf::from("/tmp/kikan-test-headless")
    );
    assert_eq!(
        config.bind_addr,
        "127.0.0.1:3000".parse::<std::net::SocketAddr>().unwrap()
    );
}

#[tokio::test]
async fn setup_mode_serde_wire_format_canary() {
    use kikan::SetupMode;

    let demo_json = serde_json::to_string(&SetupMode::Demo).unwrap();
    assert_eq!(
        demo_json, "\"demo\"",
        "Demo must serialize as lowercase 'demo'"
    );

    let prod_json = serde_json::to_string(&SetupMode::Production).unwrap();
    assert_eq!(
        prod_json, "\"production\"",
        "Production must serialize as lowercase 'production'"
    );

    let demo_parsed: SetupMode = serde_json::from_str("\"demo\"").unwrap();
    assert_eq!(demo_parsed, SetupMode::Demo);

    let prod_parsed: SetupMode = serde_json::from_str("\"production\"").unwrap();
    assert_eq!(prod_parsed, SetupMode::Production);

    assert_eq!(SetupMode::Demo.as_str(), "demo");
    assert_eq!(SetupMode::Production.as_dir_name(), "production");

    assert_eq!("Demo".parse::<SetupMode>().unwrap(), SetupMode::Demo);
    assert_eq!(
        "PRODUCTION".parse::<SetupMode>().unwrap(),
        SetupMode::Production
    );
}

#[tokio::test]
async fn deployment_mode_serde_roundtrip() {
    use kikan::DeploymentMode;

    let lan = DeploymentMode::Lan;
    let json = serde_json::to_string(&lan).unwrap();
    assert_eq!(json, "\"lan\"");
    let parsed: DeploymentMode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, DeploymentMode::Lan);

    let loopback = DeploymentMode::Loopback;
    let json = serde_json::to_string(&loopback).unwrap();
    assert_eq!(json, "\"loopback\"");
}

#[tokio::test]
async fn boot_returns_engine_and_app_state() {
    let dir = tempfile::tempdir().unwrap();
    let graft = StubGraft::diamond();
    let config = BootConfig::new(dir.path().to_path_buf());

    let demo_db = in_memory_db().await;
    let production_db = in_memory_db().await;

    let session_pool = production_db.get_sqlite_connection_pool().clone();
    let session_store = SqliteStore::new(session_pool);
    session_store.migrate().await.unwrap();

    let profile_db_init: kikan::platform_state::SharedProfileDbInitializer =
        Arc::new(support::NoOpProfileDbInitializer);

    let (engine, _state) = Engine::<StubGraft>::boot(
        config,
        &graft,
        demo_db,
        production_db,
        SetupMode::Demo,
        session_store,
        profile_db_init,
        Arc::new(AtomicBool::new(false)),
        None,
        Arc::new(AtomicBool::new(true)),
        dir.path().to_path_buf(),
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("Engine::boot should succeed");

    assert_eq!(engine.tenancy().data_dir(), dir.path());
}
