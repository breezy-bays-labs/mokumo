use super::ApiWorld;
use cucumber::{given, then, when};

// ---------------------------------------------------------------------------
// Storage-health setup helpers
// ---------------------------------------------------------------------------

/// Rebuild the world with separate demo and production databases.
///
/// `fragment_demo` — if true, fragment the demo database (insert + delete many
/// rows) so that its freelist / page_count exceeds 20 %.
/// `fragment_production` — same but for production.
async fn rebuild_with_separate_storage_dbs(
    w: &mut ApiWorld,
    fragment_demo: bool,
    fragment_production: bool,
) {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let data_dir = tmp.path().join("storage_test");
    mokumo_api::ensure_data_dirs(&data_dir).expect("failed to create data dirs");

    // Active profile = production (default for storage tests).
    std::fs::write(data_dir.join("active_profile"), "production").unwrap();

    // Initialize both database files.
    let prod_url = format!(
        "sqlite:{}?mode=rwc",
        data_dir.join("production").join("mokumo.db").display()
    );
    let demo_url = format!(
        "sqlite:{}?mode=rwc",
        data_dir.join("demo").join("mokumo.db").display()
    );
    let prod_db = mokumo_db::initialize_database(&prod_url)
        .await
        .expect("failed to init production db");
    let demo_db = mokumo_db::initialize_database(&demo_url)
        .await
        .expect("failed to init demo db");

    if fragment_production {
        fragment_db(prod_db.get_sqlite_connection_pool()).await;
    }
    if fragment_demo {
        fragment_db(demo_db.get_sqlite_connection_pool()).await;
    }

    let recovery_dir = tmp.path().join("recovery");
    std::fs::create_dir_all(&recovery_dir).expect("failed to create recovery dir");

    let session_db_path = data_dir.join("sessions.db");
    let session_url = format!("sqlite:{}?mode=rwc", session_db_path.display());
    let session_pool = mokumo_db::open_raw_sqlite_pool(&session_url)
        .await
        .expect("failed to open session db");

    let config = mokumo_api::ServerConfig {
        port: 0,
        host: "0.0.0.0".into(),
        data_dir,
        recovery_dir: recovery_dir.clone(),
        #[cfg(debug_assertions)]
        ws_ping_ms: None,
    };

    let shutdown_token = tokio_util::sync::CancellationToken::new();
    let mdns_status = mokumo_api::discovery::MdnsStatus::shared();
    let (app, setup_token, _ws) = mokumo_api::build_app_with_shutdown(
        &config,
        demo_db.clone(),
        prod_db.clone(),
        kikan::SetupMode::Production,
        shutdown_token.clone(),
        mdns_status.clone(),
    )
    .await
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind test listener");

    let shutdown = shutdown_token.clone();
    let serve = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async move { shutdown.cancelled().await });

    let server = axum_test::TestServer::builder().save_cookies().build(serve);

    // Replace old world components.
    w.shutdown_token.cancel();
    w.server = server;
    w.shutdown_token = shutdown_token;
    w.db = prod_db;
    w.db_pool = mokumo_db::DatabaseConnection::get_sqlite_connection_pool(&w.db).clone();
    w.session_pool = session_pool;
    w.setup_token = setup_token;
    w.recovery_dir = recovery_dir;
    w._tmp = tmp;
}

/// Insert many large rows into the database then delete them all, leaving a high
/// freelist / page_count ratio (> 20 %).
async fn fragment_db(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _health_frag (id INTEGER PRIMARY KEY, data BLOB NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("create scratch table failed");

    let blob = vec![0xABu8; 4096];
    for _ in 0..64i32 {
        sqlx::query("INSERT INTO _health_frag (data) VALUES (?)")
            .bind(&blob)
            .execute(pool)
            .await
            .expect("insert failed");
    }
    sqlx::query("DELETE FROM _health_frag")
        .execute(pool)
        .await
        .expect("delete failed");

    // Checkpoint WAL so free pages are visible in the main DB file.
    let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await;
}

// --- Response field assertions ---

#[then(expr = "the response should include {string} with value {string}")]
async fn response_includes_field_with_value(w: &mut ApiWorld, field: String, expected: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    assert_eq!(
        json[&field].as_str().unwrap_or_default(),
        expected,
        "Expected {field}={expected}, got {:?}",
        json[&field]
    );
}

#[then(expr = "the response should include {string}")]
async fn response_includes_field(w: &mut ApiWorld, field: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    assert!(
        !json[&field].is_null(),
        "Expected field {field} to be present, got null"
    );
}

#[then(expr = "the response should include {string} as a non-negative integer")]
async fn response_includes_non_negative_int(w: &mut ApiWorld, field: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    json[&field].as_u64().unwrap_or_else(|| {
        panic!(
            "Expected {field} to be a non-negative integer, got {:?}",
            json[&field]
        )
    });
}

// --- Uptime tracking ---

#[given("I have recorded the uptime from a health check")]
async fn record_uptime(w: &mut ApiWorld) {
    let resp = w.server.get("/api/health").await;
    let json: serde_json::Value = resp.json();
    let uptime = json["uptime_seconds"]
        .as_u64()
        .expect("uptime_seconds should be a u64");
    w.previous_uptime = Some(uptime);
}

#[when(expr = "I request GET {string} after a brief delay")]
async fn get_after_delay(w: &mut ApiWorld, path: String) {
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    w.response = Some(w.server.get(&path).await);
}

#[then("the uptime should be greater than or equal to the previous value")]
async fn uptime_increased(w: &mut ApiWorld) {
    let previous = w.previous_uptime.expect("no previous uptime recorded");
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    let current = json["uptime_seconds"]
        .as_u64()
        .expect("uptime_seconds should be a u64");
    assert!(
        current >= previous,
        "Expected uptime {current} >= previous {previous}"
    );
}

// --- Cache control ---

#[then(expr = "the response should have header {string} with value {string}")]
async fn response_has_header(w: &mut ApiWorld, header: String, expected: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let header_value = resp.header(&header);
    let actual = header_value
        .to_str()
        .expect("header value is not valid UTF-8");
    assert_eq!(
        actual, expected,
        "Expected header {header}={expected}, got {actual}"
    );
}

// --- Public access ---

#[when(expr = "I request GET {string} without credentials")]
async fn get_without_credentials(w: &mut ApiWorld, path: String) {
    // No auth is implemented yet at M0, so this is identical to a normal GET
    w.response = Some(w.server.get(&path).await);
}

#[when(expr = "I POST to {string} without credentials")]
async fn post_without_credentials(w: &mut ApiWorld, path: String) {
    w.response = Some(w.server.post(&path).await);
}

#[then(expr = "the response status should not be {int}")]
async fn response_status_not(w: &mut ApiWorld, status: u16) {
    let resp = w.response.as_ref().expect("no response recorded");
    let actual = resp.status_code().as_u16();
    assert_ne!(actual, status, "Expected status != {status}, got {actual}");
}

#[then(expr = "a subsequent GET {string} returns install_ok as true")]
async fn subsequent_get_install_ok_true(w: &mut ApiWorld, path: String) {
    w.response = Some(w.server.get(&path).await);
    let resp = w.response.as_ref().expect("no response captured");
    assert_eq!(
        resp.status_code().as_u16(),
        200,
        "Expected 200 from {path}, got {}",
        resp.status_code().as_u16()
    );
    let body: serde_json::Value = resp.json();
    let install_ok = body["install_ok"].as_bool().unwrap_or(false);
    assert!(
        install_ok,
        "Expected install_ok=true in {path} response, got: {body}"
    );
}

// --- Storage-health step definitions ---

/// Set threshold to 0 → any available space satisfies it → disk_warning = false.
#[given("disk space is above the warning threshold")]
async fn disk_above_threshold(_w: &mut ApiWorld) {
    // SAFETY: single-threaded BDD scenario; no other thread reads this env var concurrently.
    unsafe {
        std::env::set_var("MOKUMO_DISK_WARNING_THRESHOLD_BYTES", "0");
    }
}

/// Set threshold to u64::MAX → available space is always less → disk_warning = true.
#[given("disk space is below the warning threshold")]
async fn disk_below_threshold(_w: &mut ApiWorld) {
    // SAFETY: single-threaded BDD scenario; no other thread reads this env var concurrently.
    unsafe {
        std::env::set_var(
            "MOKUMO_DISK_WARNING_THRESHOLD_BYTES",
            "18446744073709551615",
        );
    }
}

/// Fresh server with a non-fragmented production database (default state).
#[given("the active database is not fragmented")]
async fn active_db_not_fragmented(w: &mut ApiWorld) {
    // Rebuild with separate prod/demo DBs, neither fragmented.
    rebuild_with_separate_storage_dbs(w, false, false).await;
}

/// Rebuild with a heavily fragmented production database (active profile).
#[given("the active database is heavily fragmented")]
async fn active_db_heavily_fragmented(w: &mut ApiWorld) {
    rebuild_with_separate_storage_dbs(w, false, true).await;
}

/// Rebuild with the demo (inactive) database fragmented; production stays clean.
#[given("the inactive database is heavily fragmented")]
async fn inactive_db_heavily_fragmented(w: &mut ApiWorld) {
    rebuild_with_separate_storage_dbs(w, true, false).await;
}

// --- Database failure ---

#[given("the database is unavailable")]
async fn database_unavailable(w: &mut ApiWorld) {
    // Build the app with a VALID DB first (session store needs migration),
    // then close the pool to simulate DB failure at request time.
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let data_dir = tmp.path().join("bad_db_test");
    mokumo_api::ensure_data_dirs(&data_dir).expect("failed to create dirs");
    let db_path = data_dir.join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = mokumo_db::initialize_database(&database_url)
        .await
        .expect("failed to init db");

    let config = mokumo_api::ServerConfig {
        port: 0,
        host: "127.0.0.1".into(),
        recovery_dir: data_dir.join("recovery"),
        data_dir,
        #[cfg(debug_assertions)]
        ws_ping_ms: None,
    };

    let shutdown = tokio_util::sync::CancellationToken::new();
    let mdns_status = mokumo_api::discovery::MdnsStatus::shared();
    let (app, _, _ws) = mokumo_api::build_app_with_shutdown(
        &config,
        db.clone(),
        db.clone(),
        kikan::SetupMode::Production,
        shutdown.clone(),
        mdns_status,
    )
    .await
    .unwrap();

    // NOW close the pool to simulate database failure at request time
    db.close().await.ok();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind test listener");

    let shutdown_clone = shutdown.clone();
    let serve = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async move { shutdown_clone.cancelled().await });

    let server = axum_test::TestServer::builder().save_cookies().build(serve);

    w.server = server;
    w.shutdown_token = shutdown;
}
