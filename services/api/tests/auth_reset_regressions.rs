use std::path::PathBuf;

use axum_test::TestServer;
use mokumo_api::auth::reset::recovery_file_path_for_email;
use mokumo_api::{ServerConfig, build_app, ensure_data_dirs};
use mokumo_core::setup::SetupMode;
use mokumo_core::user::traits::UserRepository;
use mokumo_core::user::{CreateUser, RoleId};
use mokumo_db::DatabaseConnection;
use mokumo_db::user::repo::SeaOrmUserRepo;
use serde_json::json;

struct RunningServer {
    server: TestServer,
    db: DatabaseConnection,
    recovery_dir: PathBuf,
    _setup_token: Option<String>,
    _tmp: tempfile::TempDir,
}

impl RunningServer {
    async fn start(name: &str) -> Self {
        Self::start_inner(name, false).await
    }

    /// Start a server that preserves cookies across requests (needed for session tests).
    async fn start_with_cookies(name: &str) -> Self {
        Self::start_inner(name, true).await
    }

    async fn start_inner(name: &str, save_cookies: bool) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join(name);
        let recovery_dir = tmp.path().join("recovery");
        ensure_data_dirs(&data_dir).unwrap();
        std::fs::create_dir_all(&recovery_dir).unwrap();

        let db_path = data_dir.join("mokumo.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let db = mokumo_db::initialize_database(&database_url).await.unwrap();

        let config = ServerConfig {
            port: 0,
            host: "127.0.0.1".into(),
            data_dir,
            recovery_dir: recovery_dir.clone(),
        };

        let (app, setup_token) = build_app(&config, db.clone(), db.clone(), SetupMode::Production)
            .await
            .unwrap();
        let server = if save_cookies {
            TestServer::builder().save_cookies().build(app).unwrap()
        } else {
            TestServer::new(app).unwrap()
        };

        Self {
            server,
            db,
            recovery_dir,
            _setup_token: setup_token,
            _tmp: tmp,
        }
    }
}

fn extract_pin_from_html(html: &str) -> String {
    if let Some(start) = html.find("font-weight:bold\">") {
        let after = &html[start + "font-weight:bold\">".len()..];
        if let Some(end) = after.find("</p>") {
            return after[..end].trim().to_string();
        }
    }
    panic!(
        "failed to extract PIN from recovery HTML: {}",
        &html[..html.len().min(500)]
    )
}

#[tokio::test]
async fn file_drop_reset_uses_separate_files_per_user() {
    let server = RunningServer::start("file_drop_isolation").await;
    let repo = SeaOrmUserRepo::new(server.db.clone());

    repo.create_admin_with_setup("admin@shop.local", "Admin", "password123", "Test Shop")
        .await
        .unwrap();
    repo.create(&CreateUser {
        email: "staff@shop.local".into(),
        name: "Staff".into(),
        password: "password123".into(),
        role_id: RoleId::new(2),
    })
    .await
    .unwrap();

    let admin_response = server
        .server
        .post("/api/auth/forgot-password")
        .json(&json!({ "email": "admin@shop.local" }))
        .await;
    assert_eq!(admin_response.status_code(), http::StatusCode::OK);

    let admin_file = recovery_file_path_for_email(&server.recovery_dir, "admin@shop.local");
    assert!(admin_file.exists(), "admin recovery file should exist");
    let admin_html = std::fs::read_to_string(&admin_file).unwrap();
    let admin_pin = extract_pin_from_html(&admin_html);

    let staff_response = server
        .server
        .post("/api/auth/forgot-password")
        .json(&json!({ "email": "staff@shop.local" }))
        .await;
    assert_eq!(staff_response.status_code(), http::StatusCode::OK);

    let staff_file = recovery_file_path_for_email(&server.recovery_dir, "staff@shop.local");
    assert!(staff_file.exists(), "staff recovery file should exist");
    assert_ne!(
        admin_file, staff_file,
        "users should not share a recovery file"
    );
    assert!(
        admin_file.exists(),
        "admin recovery file should remain after staff request"
    );

    let staff_html = std::fs::read_to_string(&staff_file).unwrap();
    let staff_pin = extract_pin_from_html(&staff_html);
    assert_ne!(
        admin_pin, staff_pin,
        "each user should get their own reset PIN"
    );

    let reset_response = server
        .server
        .post("/api/auth/reset-password")
        .json(&json!({
            "email": "admin@shop.local",
            "pin": admin_pin,
            "new_password": "new-password-456",
        }))
        .await;
    assert_eq!(reset_response.status_code(), http::StatusCode::OK);

    assert!(
        !admin_file.exists(),
        "successful reset should remove that user's file"
    );
    assert!(
        staff_file.exists(),
        "another user's recovery file should survive a different reset"
    );
}

#[tokio::test]
async fn recovery_code_rate_limit_returns_generic_400() {
    let server = RunningServer::start("recover_rate_limit").await;
    let repo = SeaOrmUserRepo::new(server.db.clone());

    let (_, recovery_codes) = repo
        .create_admin_with_setup(
            "ratelimit@shop.local",
            "Rate Limit Admin",
            "password123",
            "Test Shop",
        )
        .await
        .unwrap();

    // Make 5 invalid attempts (within the rate limit)
    for i in 0..5 {
        let response = server
            .server
            .post("/api/auth/recover")
            .json(&json!({
                "email": "ratelimit@shop.local",
                "recovery_code": "INVALID-CODE",
                "new_password": "newpassword123",
            }))
            .await;

        assert_eq!(
            response.status_code(),
            http::StatusCode::BAD_REQUEST,
            "attempt {i} should return 400 (invalid code)"
        );
    }

    // 6th attempt should be rate-limited but return the SAME generic response
    let response = server
        .server
        .post("/api/auth/recover")
        .json(&json!({
            "email": "ratelimit@shop.local",
            "recovery_code": recovery_codes[0].clone(),
            "new_password": "newpassword123",
        }))
        .await;

    assert_eq!(
        response.status_code(),
        http::StatusCode::BAD_REQUEST,
        "rate-limited attempt should return 400, not 429"
    );
    let body: serde_json::Value = response.json();
    assert_eq!(
        body["code"], "validation_error",
        "rate-limited response should be indistinguishable from invalid code"
    );
    assert_eq!(
        body["message"], "Invalid or used recovery code",
        "rate-limited response message should match normal rejection"
    );

    // Verify the valid recovery code was NOT consumed (rate limit blocked before DB check)
    // A fresh server would allow this code — but same server, limit still active
}

#[tokio::test]
async fn recovery_code_rate_limit_is_per_email() {
    let server = RunningServer::start("recover_rate_limit_per_email").await;
    let repo = SeaOrmUserRepo::new(server.db.clone());

    repo.create_admin_with_setup("admin@shop.local", "Admin", "password123", "Test Shop")
        .await
        .unwrap();
    repo.create(&CreateUser {
        email: "staff@shop.local".into(),
        name: "Staff".into(),
        password: "password123".into(),
        role_id: RoleId::new(2),
    })
    .await
    .unwrap();

    // Exhaust rate limit for admin
    for _ in 0..5 {
        server
            .server
            .post("/api/auth/recover")
            .json(&json!({
                "email": "admin@shop.local",
                "recovery_code": "INVALID",
                "new_password": "newpassword123",
            }))
            .await;
    }

    // Staff should still be allowed (independent rate limit)
    let response = server
        .server
        .post("/api/auth/recover")
        .json(&json!({
            "email": "staff@shop.local",
            "recovery_code": "INVALID",
            "new_password": "newpassword123",
        }))
        .await;

    // Should get 400 (invalid code), not rate-limited
    assert_eq!(response.status_code(), http::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json();
    assert_eq!(body["code"], "validation_error");
}

#[tokio::test]
async fn recovery_code_reset_rejects_short_passwords() {
    let server = RunningServer::start("recover_short_password").await;
    let repo = SeaOrmUserRepo::new(server.db.clone());

    let (_, recovery_codes) = repo
        .create_admin_with_setup(
            "recover@shop.local",
            "Recover Admin",
            "password123",
            "Test Shop",
        )
        .await
        .unwrap();

    let response = server
        .server
        .post("/api/auth/recover")
        .json(&json!({
            "email": "recover@shop.local",
            "recovery_code": recovery_codes[0],
            "new_password": "short",
        }))
        .await;

    assert_eq!(response.status_code(), http::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json();
    assert_eq!(body["code"], "validation_error");
    assert_eq!(body["message"], "Password must be at least 8 characters");
}

#[tokio::test]
async fn forgot_password_returns_error_for_unknown_email() {
    let server = RunningServer::start("forgot_unknown_email").await;

    let response = server
        .server
        .post("/api/auth/forgot-password")
        .json(&json!({ "email": "nobody@shop.local" }))
        .await;

    assert_eq!(response.status_code(), http::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json();
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("No account found"),
        "expected 'No account found' in message, got: {:?}",
        body
    );
}

#[tokio::test]
async fn forgot_password_success_returns_recovery_file_path() {
    let server = RunningServer::start("forgot_returns_path").await;
    let repo = SeaOrmUserRepo::new(server.db.clone());

    repo.create_admin_with_setup("admin@shop.local", "Admin", "password123", "Test Shop")
        .await
        .unwrap();

    let response = server
        .server
        .post("/api/auth/forgot-password")
        .json(&json!({ "email": "admin@shop.local" }))
        .await;

    assert_eq!(response.status_code(), http::StatusCode::OK);
    let body: serde_json::Value = response.json();
    let path = body["recovery_file_path"]
        .as_str()
        .expect("response should include recovery_file_path");
    assert!(
        path.ends_with(".html"),
        "recovery_file_path should end with .html, got: {path}"
    );
    assert!(
        path.contains("mokumo-recovery-"),
        "recovery_file_path should contain 'mokumo-recovery-', got: {path}"
    );
}

/// Intentional behavior: recovery code regeneration does NOT invalidate existing
/// sessions. Session invalidation on credential change is deferred to M1
/// (per CAO + Ada review).
#[tokio::test]
async fn sessions_survive_recovery_code_regeneration() {
    let server = RunningServer::start_with_cookies("session_survives_regen").await;

    // Setup admin (creates recovery codes)
    let setup_token = server._setup_token.as_ref().unwrap();
    let resp = server
        .server
        .post("/api/setup")
        .json(&json!({
            "shop_name": "Test Shop",
            "admin_name": "Admin",
            "admin_email": "admin@shop.local",
            "admin_password": "password123",
            "setup_token": setup_token
        }))
        .await;
    assert_eq!(resp.status_code(), http::StatusCode::CREATED);

    // Verify we are authenticated (setup auto-logs in)
    let me_resp = server.server.get("/api/auth/me").await;
    assert_eq!(me_resp.status_code(), http::StatusCode::OK);

    // Regenerate recovery codes
    let regen_resp = server
        .server
        .post("/api/account/recovery-codes/regenerate")
        .json(&json!({ "password": "password123" }))
        .await;
    assert_eq!(regen_resp.status_code(), http::StatusCode::OK);
    let body: serde_json::Value = regen_resp.json();
    assert_eq!(
        body["recovery_codes"].as_array().unwrap().len(),
        10,
        "should receive 10 new codes"
    );

    // Session should still be valid after regeneration
    let me_after = server.server.get("/api/auth/me").await;
    assert_eq!(
        me_after.status_code(),
        http::StatusCode::OK,
        "session should remain valid after recovery code regeneration"
    );
}

#[tokio::test]
async fn file_drop_recovery_works_with_spaces_in_recovery_dir() {
    // macOS resolves Tauri's app_data_dir() to ~/Library/Application Support/...
    // Both the data dir and recovery dir may contain spaces — verify the full
    // forgot-password flow succeeds and creates the recovery file when they do.
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("Application Support");
    let recovery_dir = tmp.path().join("Recovery Files");
    ensure_data_dirs(&data_dir).unwrap();
    std::fs::create_dir_all(&recovery_dir).unwrap();

    let db_path = data_dir.join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = mokumo_db::initialize_database(&database_url).await.unwrap();

    let config = ServerConfig {
        port: 0,
        host: "127.0.0.1".into(),
        data_dir,
        recovery_dir: recovery_dir.clone(),
    };
    let (app, _setup_token) = build_app(&config, db.clone(), db.clone(), SetupMode::Production)
        .await
        .unwrap();

    let repo = SeaOrmUserRepo::new(db.clone());
    repo.create_admin_with_setup("admin@shop.local", "Admin", "password123", "Test Shop")
        .await
        .unwrap();

    let server = TestServer::new(app).unwrap();
    let response = server
        .post("/api/auth/forgot-password")
        .json(&json!({ "email": "admin@shop.local" }))
        .await;
    assert_eq!(
        response.status_code(),
        http::StatusCode::OK,
        "forgot-password must succeed when recovery_dir contains spaces"
    );

    let recovery_file = recovery_file_path_for_email(&recovery_dir, "admin@shop.local");
    assert!(
        recovery_file.exists(),
        "recovery file must be created when recovery_dir contains spaces"
    );
}
