use axum::Router;
use axum::body::Body;
use http::Request;
use tower::ServiceExt;

use kikan_types::SetupMode;
use mokumo_shop::startup::ensure_data_dirs;

mod common;

/// Create a test app with a temp database. Returns the router and tempdir
/// (tempdir must be held alive for the duration of the test).
async fn test_app(name: &str) -> (Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join(name);
    ensure_data_dirs(&data_dir).unwrap();
    // Use the production subdir so the health handler's db_path computation
    // (data_dir/production/mokumo.db) resolves to the actual database file.
    let db_path = data_dir.join("production").join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = mokumo_shop::db::initialize_database(&database_url)
        .await
        .unwrap();
    let recovery_dir = data_dir.join("recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();
    let shutdown = tokio_util::sync::CancellationToken::new();
    let (app, _) = common::boot_router(
        data_dir,
        recovery_dir,
        pool.clone(),
        pool,
        SetupMode::Production,
        shutdown,
    )
    .await;
    (app, tmp)
}

#[test]
fn ensure_data_dirs_creates_all_subdirectories() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("app_data");

    ensure_data_dirs(&data_dir).unwrap();

    assert!(data_dir.exists(), "data_dir should exist");
    assert!(data_dir.join("logs").exists(), "logs/ should exist");
}

#[tokio::test]
async fn full_startup_flow_with_temp_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("mokumo_test");
    ensure_data_dirs(&data_dir).unwrap();

    let db_path = data_dir.join("production").join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = mokumo_shop::db::initialize_database(&database_url)
        .await
        .unwrap();

    let recovery_dir = data_dir.join("recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();
    let shutdown = tokio_util::sync::CancellationToken::new();
    let _app = common::boot_router(
        data_dir.clone(),
        recovery_dir,
        pool.clone(),
        pool,
        SetupMode::Production,
        shutdown,
    )
    .await;

    assert!(db_path.exists(), "database file should exist");
    assert!(data_dir.join("logs").exists(), "logs/ should exist");
}

#[tokio::test]
async fn health_endpoint_returns_ok_and_version() {
    let (app, _tmp) = test_app("health_test").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header("host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), http::StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string(), "version should be a string");
}

#[tokio::test]
async fn health_endpoint_returns_500_error_body_on_db_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("health_503_test");
    ensure_data_dirs(&data_dir).unwrap();

    let db_path = data_dir.join("production").join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = mokumo_shop::db::initialize_database(&database_url)
        .await
        .unwrap();

    let recovery_dir = data_dir.join("recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();
    let shutdown = tokio_util::sync::CancellationToken::new();
    let (app, _) = common::boot_router(
        data_dir,
        recovery_dir,
        db.clone(),
        db.clone(),
        SetupMode::Production,
        shutdown,
    )
    .await;

    // Close the connection AFTER build to simulate database failure at request time
    db.close().await.ok();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header("host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // AppError maps database errors to 500 with a redacted ErrorBody
    assert_eq!(response.status(), http::StatusCode::INTERNAL_SERVER_ERROR);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["code"].as_str().unwrap(), "internal_error");
    assert_eq!(
        json["message"].as_str().unwrap(),
        "An internal error occurred"
    );
}

#[tokio::test]
async fn spa_fallback_returns_json_404_for_unknown_api_paths() {
    let (app, _tmp) = test_app("spa_test").await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/unknown")
                .header("host", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(content_type, "application/json");

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"].as_str().unwrap(), "not_found");
    assert!(json["message"].as_str().is_some(), "Expected message field");
}
