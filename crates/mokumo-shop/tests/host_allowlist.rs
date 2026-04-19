use axum::body::Body;
use http::Request;
use tower::ServiceExt;

mod common;

async fn build_test_app() -> (axum::Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("host_test");
    mokumo_shop::startup::ensure_data_dirs(&data_dir).unwrap();

    let db_path = data_dir.join("mokumo.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = mokumo_shop::db::initialize_database(&url).await.unwrap();

    let recovery_dir = data_dir.join("recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();
    let shutdown = tokio_util::sync::CancellationToken::new();
    let (router, _) = common::boot_router(
        data_dir,
        recovery_dir,
        pool.clone(),
        pool,
        kikan::SetupMode::Production,
        shutdown,
    )
    .await;
    (router, tmp)
}

#[tokio::test]
async fn host_evil_rejected() {
    let (app, _tmp) = build_test_app().await;

    let req = Request::builder()
        .uri("/api/health")
        .header("host", "evil.com")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 403);

    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(ct, "application/json");

    let cc = resp
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(cc, "no-store");

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["code"], "HOST_NOT_ALLOWED");
    assert_eq!(parsed["message"], "Host header not allowed");
    assert!(parsed["details"].is_null());
}

#[tokio::test]
async fn host_loopback_accepted() {
    let (app, _tmp) = build_test_app().await;

    let req = Request::builder()
        .uri("/api/health")
        .header("host", "127.0.0.1:6565")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
}
