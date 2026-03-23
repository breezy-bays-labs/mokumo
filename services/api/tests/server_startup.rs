use axum::body::Body;
use http::Request;
use tower::ServiceExt;

use mokumo_api::{ServerConfig, build_app, ensure_data_dirs};

#[test]
fn ensure_data_dirs_creates_all_subdirectories() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("app_data");

    ensure_data_dirs(&data_dir).unwrap();

    assert!(data_dir.exists(), "data_dir should exist");
    assert!(data_dir.join("logs").exists(), "logs/ should exist");
    assert!(data_dir.join("backups").exists(), "backups/ should exist");
}

#[tokio::test]
async fn full_startup_flow_with_temp_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("mokumo_test");

    let config = ServerConfig {
        port: 0,
        host: "127.0.0.1".into(),
        data_dir: data_dir.clone(),
    };

    ensure_data_dirs(&config.data_dir).unwrap();

    let db_path = data_dir.join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = mokumo_db::initialize_database(&database_url).await.unwrap();

    let _app = build_app(&config, pool);

    assert!(db_path.exists(), "database file should exist");
    assert!(data_dir.join("logs").exists(), "logs/ should exist");
    assert!(data_dir.join("backups").exists(), "backups/ should exist");
}

#[tokio::test]
async fn health_endpoint_returns_ok_and_version() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("health_test");
    ensure_data_dirs(&data_dir).unwrap();

    let db_path = data_dir.join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = mokumo_db::initialize_database(&database_url).await.unwrap();

    let config = ServerConfig {
        port: 0,
        host: "127.0.0.1".into(),
        data_dir,
    };

    let app = build_app(&config, pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
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
async fn spa_fallback_returns_json_404_for_unknown_api_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("spa_test");
    ensure_data_dirs(&data_dir).unwrap();

    let db_path = data_dir.join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = mokumo_db::initialize_database(&database_url).await.unwrap();

    let config = ServerConfig {
        port: 0,
        host: "127.0.0.1".into(),
        data_dir,
    };

    let app = build_app(&config, pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/unknown")
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
    assert_eq!(json["error"], "not_found");
}
