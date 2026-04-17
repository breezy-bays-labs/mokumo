use std::path::PathBuf;

use axum_test::TestServer;
use kikan::SetupMode;
use kikan::auth::SeaOrmUserRepo;
use kikan::auth::UserRepository;
use mokumo_api::{ServerConfig, build_app, ensure_data_dirs};
use mokumo_db::DatabaseConnection;
use serde_json::json;

struct RunningServer {
    server: TestServer,
    db: DatabaseConnection,
    setup_token: Option<String>,
    _recovery_dir: PathBuf,
    _tmp: tempfile::TempDir,
}

impl RunningServer {
    async fn start(name: &str) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join(name);
        let recovery_dir = tmp.path().join("recovery");
        ensure_data_dirs(&data_dir).unwrap();
        std::fs::create_dir_all(&recovery_dir).unwrap();

        let db_path = data_dir.join("mokumo.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let db = mokumo_shop::db::initialize_database(&database_url)
            .await
            .unwrap();

        let config = ServerConfig {
            port: 0,
            host: "127.0.0.1".into(),
            data_dir,
            recovery_dir: recovery_dir.clone(),
            #[cfg(debug_assertions)]
            ws_ping_ms: None,
        };

        let (app, setup_token) = build_app(&config, db.clone(), db.clone(), SetupMode::Production)
            .await
            .unwrap();
        let server = TestServer::new(app);

        Self {
            server,
            db,
            setup_token,
            _recovery_dir: recovery_dir,
            _tmp: tmp,
        }
    }
}

#[tokio::test]
async fn concurrent_setup_requests_only_create_one_admin() {
    let server = RunningServer::start("concurrent_setup").await;
    let setup_token = server
        .setup_token
        .clone()
        .expect("fresh server should have token");

    let first = server.server.post("/api/setup").json(&json!({
        "shop_name": "Test Shop",
        "admin_name": "Admin One",
        "admin_email": "admin-one@test.local",
        "admin_password": "password123",
        "setup_token": setup_token,
    }));
    let second = server.server.post("/api/setup").json(&json!({
        "shop_name": "Test Shop",
        "admin_name": "Admin Two",
        "admin_email": "admin-two@test.local",
        "admin_password": "password123",
        "setup_token": server.setup_token.clone().unwrap(),
    }));

    let (first, second) = tokio::join!(first, second);
    let statuses = [first.status_code(), second.status_code()];

    assert!(
        statuses.contains(&http::StatusCode::CREATED),
        "one request should complete setup successfully, got {statuses:?}"
    );
    assert!(
        statuses.contains(&http::StatusCode::CONFLICT)
            || statuses.contains(&http::StatusCode::FORBIDDEN),
        "the competing request should be rejected, got {statuses:?}"
    );

    let user_count = SeaOrmUserRepo::new(server.db.clone())
        .count()
        .await
        .unwrap();
    assert_eq!(user_count, 1, "setup must only create one admin");
}
