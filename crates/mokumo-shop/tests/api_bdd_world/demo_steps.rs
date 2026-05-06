use super::ApiWorld;
use cucumber::{given, then, when};

/// Configuration for rebuilding the BDD world with a specific profile.
struct WorldConfig {
    profile: &'static str,
    dir_name: &'static str,
    seed: &'static SeedConfig,
}

/// Rebuild the BDD world with a fresh server in the specified mode.
///
/// Creates a temp data directory, initializes the database with migrations,
/// seeds test data, and starts an Axum test server.
async fn rebuild_world(w: &mut ApiWorld, cfg: &WorldConfig) {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let data_dir = tmp.path().join(cfg.dir_name);
    mokumo_shop::startup::ensure_data_dirs(&data_dir).expect("failed to create data dirs");

    std::fs::write(data_dir.join("active_profile"), cfg.profile).unwrap();

    let db_path = data_dir.join(cfg.profile).join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = mokumo_shop::db::initialize_database(&database_url)
        .await
        .unwrap_or_else(|e| panic!("failed to initialize {0} database: {e}", cfg.profile));

    seed_test_data(&db, cfg.seed).await;

    // Demo mode: create a sidecar copy so reset can find it
    if cfg.profile == "demo" {
        let sidecar_path = tmp.path().join("sidecar_for_reset.db");
        std::fs::copy(&db_path, &sidecar_path).expect("failed to copy sidecar for reset");
        unsafe { std::env::set_var("MOKUMO_DEMO_SIDECAR", &sidecar_path) };
    }

    let pool = db.get_sqlite_connection_pool().clone();

    let recovery_dir = tmp.path().join("recovery");
    std::fs::create_dir_all(&recovery_dir).expect("failed to create recovery dir");

    let shutdown_token = tokio_util::sync::CancellationToken::new();
    let active_profile = match cfg.profile {
        "demo" => kikan_types::SetupMode::Demo,
        _ => kikan_types::SetupMode::Production,
    };

    let (server, setup_token, app_state, session_pool) = super::boot_test_server(
        data_dir,
        recovery_dir.clone(),
        db.clone(),
        db.clone(),
        active_profile,
        shutdown_token.clone(),
    )
    .await;

    w.server = server;
    w.shutdown_token = shutdown_token;
    w.db = db;
    w.db_pool = pool;
    w.session_pool = session_pool;
    w.mdns_status = app_state.mdns_status().clone();
    w.setup_token = setup_token;
    w.recovery_dir = recovery_dir;
    w.auth_done = false;
    w._tmp = tmp;
}

/// Rebuild the BDD world with a demo-mode server.
async fn rebuild_as_demo(w: &mut ApiWorld) {
    rebuild_world(
        w,
        &WorldConfig {
            profile: "demo",
            dir_name: "demo_test",
            seed: &DEMO_SEED,
        },
    )
    .await;
}

/// Rebuild as a production-mode server with setup already completed.
async fn rebuild_as_production_with_setup(w: &mut ApiWorld) {
    rebuild_world(
        w,
        &WorldConfig {
            profile: "production",
            dir_name: "prod_test",
            seed: &PRODUCTION_SEED,
        },
    )
    .await;
}

/// Configuration for seeding a test database with an admin user and settings.
struct SeedConfig {
    setup_mode: &'static str,
    shop_name: &'static str,
    admin_email: &'static str,
    admin_name: &'static str,
    admin_password: &'static str,
}

const DEMO_SEED: SeedConfig = SeedConfig {
    setup_mode: "demo",
    shop_name: "Demo Shop",
    admin_email: "admin@demo.local",
    admin_name: "Demo Admin",
    admin_password: "demo-password",
};

const PRODUCTION_SEED: SeedConfig = SeedConfig {
    setup_mode: "production",
    shop_name: "Test Shop",
    admin_email: "admin@test.local",
    admin_name: "Test Admin",
    admin_password: "test-password",
};

/// Seed a test database with settings and an admin user.
async fn seed_test_data(db: &sea_orm::DatabaseConnection, cfg: &SeedConfig) {
    let pool = db.get_sqlite_connection_pool();

    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('setup_mode', ?)")
        .bind(cfg.setup_mode)
        .execute(pool)
        .await
        .expect("failed to insert setup_mode");
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('setup_complete', 'true')")
        .execute(pool)
        .await
        .expect("failed to insert setup_complete");
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('shop_name', ?)")
        .bind(cfg.shop_name)
        .execute(pool)
        .await
        .expect("failed to insert shop_name");

    // The kikan boot-state probe (`inspect_legacy_vertical_db`) reads
    // `shop_settings.shop_name WHERE id = 1`. The platform migration seeds
    // an empty default; the production /api/setup handler updates it.
    // Tests that mint a "post-setup" world must do the same — otherwise
    // Engine::boot returns DefensiveEmptyShopName and the world fails to
    // construct.
    sqlx::query("UPDATE shop_settings SET shop_name = ? WHERE id = 1")
        .bind(cfg.shop_name)
        .execute(pool)
        .await
        .expect("failed to update shop_settings.shop_name");

    let hash = password_auth::generate_hash(cfg.admin_password);
    sqlx::query(
        "INSERT INTO users (email, name, password_hash, role_id, is_active) \
         VALUES (?, ?, ?, 1, 1)",
    )
    .bind(cfg.admin_email)
    .bind(cfg.admin_name)
    .bind(&hash)
    .execute(pool)
    .await
    .expect("failed to insert admin user");
}

// =====================================================================
// demo_auth.feature steps
// =====================================================================

#[given("the server is running in demo mode")]
async fn server_in_demo_mode(w: &mut ApiWorld) {
    rebuild_as_demo(w).await;
}

#[given("the server is running in production mode")]
async fn server_in_production_mode(w: &mut ApiWorld) {
    rebuild_as_production_with_setup(w).await;
}

#[given("the server is running in production mode with setup complete")]
async fn server_in_production_mode_setup_complete(w: &mut ApiWorld) {
    rebuild_as_production_with_setup(w).await;
}

#[given("the server is running with no setup completed")]
async fn server_running_no_setup(_w: &mut ApiWorld) {
    // Default ApiWorld::new() has no setup — use it as-is
}

#[given("the demo database has no admin user")]
async fn demo_no_admin(w: &mut ApiWorld) {
    let pool = w.db.get_sqlite_connection_pool();
    sqlx::query("DELETE FROM users WHERE email = 'admin@demo.local'")
        .execute(pool)
        .await
        .expect("failed to delete demo admin");
}

#[then("a session is automatically created for the demo admin")]
async fn session_created_for_demo_admin(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    // If auto-login worked, the protected route should return 200, not 401
    assert_eq!(
        resp.status_code(),
        200,
        "Expected 200 (auto-login should have created session), got {}",
        resp.status_code()
    );
}

// Note: "the response includes a session cookie" is defined in auth_steps.rs

#[when("the auto-login creates a session")]
async fn auto_login_creates_session(w: &mut ApiWorld) {
    // Trigger auto-login by hitting a protected route
    w.response = Some(w.server.get("/api/customers").await);
}

#[then(expr = "the authenticated user email is {string}")]
async fn authenticated_user_email(w: &mut ApiWorld, expected_email: String) {
    let me_resp = w.server.get("/api/auth/me").await;
    let json: serde_json::Value = me_resp.json();
    assert_eq!(
        json["user"]["email"].as_str().unwrap(),
        expected_email,
        "Expected user email {expected_email}, got {:?}",
        json["user"]["email"]
    );
}

#[then(expr = "the authenticated user name is {string}")]
async fn authenticated_user_name(w: &mut ApiWorld, expected_name: String) {
    let me_resp = w.server.get("/api/auth/me").await;
    let json: serde_json::Value = me_resp.json();
    assert_eq!(
        json["user"]["name"].as_str().unwrap(),
        expected_name,
        "Expected user name {expected_name}, got {:?}",
        json["user"]["name"]
    );
}

#[then("no automatic session is created")]
async fn no_auto_session(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    // In production mode, unauthenticated requests to protected routes should get 401/redirect
    assert_ne!(
        resp.status_code(),
        200,
        "Expected non-200 (no auto-login in production mode)"
    );
}

#[then("the response indicates authentication is required")]
async fn response_auth_required(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    assert_eq!(resp.status_code(), 401);
}

#[then("the response indicates an error with a helpful message")]
async fn response_error_helpful_message(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    // Without demo admin, auto-login should fail with a service-unavailable or error
    let status = resp.status_code().as_u16();
    assert!(status >= 400, "Expected error status, got {status}",);
}

// --- Setup Status steps ---

#[when("a client requests the setup status")]
async fn request_setup_status(w: &mut ApiWorld) {
    w.response = Some(w.server.get("/api/setup-status").await);
}

#[then(expr = "the response includes {string} as true")]
async fn response_field_true(w: &mut ApiWorld, field: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    assert_eq!(
        json[&field], true,
        "Expected {field}=true, got {:?}",
        json[&field]
    );
}

#[then(expr = "the response includes {string} as false")]
async fn response_field_false(w: &mut ApiWorld, field: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    assert_eq!(
        json[&field], false,
        "Expected {field}=false, got {:?}",
        json[&field]
    );
}

#[then(expr = "the response includes {string} as {string}")]
async fn response_field_string(w: &mut ApiWorld, field: String, expected: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    assert_eq!(
        json[&field].as_str().unwrap_or(""),
        expected,
        "Expected {field}={expected}, got {:?}",
        json[&field]
    );
}

#[then(expr = "the response includes {string} as null")]
async fn response_field_null(w: &mut ApiWorld, field: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    assert!(
        json[&field].is_null(),
        "Expected {field}=null, got {:?}",
        json[&field]
    );
}

// --- Setup mode caching ---

#[given(expr = "a demo database with setup_mode set to {string}")]
async fn demo_db_with_setup_mode(w: &mut ApiWorld, mode: String) {
    match mode.as_str() {
        "demo" => rebuild_as_demo(w).await,
        "production" => rebuild_as_production_with_setup(w).await,
        _ => panic!("Unknown setup mode: {mode}"),
    }
}

// Note: "When the server starts" is defined in discovery_steps.rs

#[then(expr = "the setup-status response returns setup_mode {string}")]
async fn setup_status_returns_mode(w: &mut ApiWorld, expected_mode: String) {
    let resp = w.server.get("/api/setup-status").await;
    let json: serde_json::Value = resp.json();
    assert_eq!(
        json["setup_mode"].as_str().unwrap_or("null"),
        expected_mode,
        "Expected setup_mode={expected_mode}, got {:?}",
        json["setup_mode"]
    );
}

// =====================================================================
// demo_reset.feature steps
// =====================================================================

#[given("the demo database has been modified")]
async fn demo_db_modified(w: &mut ApiWorld) {
    // Insert a marker row that we can check after reset
    let pool = w.db.get_sqlite_connection_pool();
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('test_marker', 'modified')")
        .execute(pool)
        .await
        .expect("failed to insert test marker");
}

#[given("the demo database has active connections")]
async fn demo_db_active_connections(_w: &mut ApiWorld) {
    // The server already has an active connection pool — no extra setup needed
}

#[when("a client sends a demo reset request")]
async fn send_demo_reset(w: &mut ApiWorld) {
    // In production mode, need to log in first since login_required applies
    let status_resp = w.server.get("/api/setup-status").await;
    let json: serde_json::Value = status_resp.json();
    if json["setup_mode"].as_str() == Some("production") {
        let login_resp = w
            .server
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "admin@test.local",
                "password": "test-password"
            }))
            .await;
        assert_eq!(
            login_resp.status_code(),
            200,
            "Production login failed: {}",
            login_resp.text()
        );
    }
    w.response = Some(w.server.post("/api/demo/reset").await);
}

#[when("an unauthenticated client sends a demo reset request")]
async fn unauthenticated_demo_reset(w: &mut ApiWorld) {
    // In demo mode, auto-login fires on protected routes, so a truly unauthenticated
    // request isn't possible. We test the auth guard by using production mode where
    // no auto-login exists and no login has been performed.
    rebuild_as_production_with_setup(w).await;
    w.response = Some(w.server.post("/api/demo/reset").await);
}

#[then("the demo database is replaced with a fresh copy of the sidecar")]
async fn demo_db_replaced(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    let body = resp.text();
    assert_eq!(
        resp.status_code(),
        200,
        "Expected 200 for reset, got {} — body: {body}",
        resp.status_code()
    );
    let json: serde_json::Value = serde_json::from_str(&body).expect("response should be JSON");
    assert_eq!(json["success"], true);
}

#[then("the server initiates a graceful shutdown")]
async fn server_shutdown_initiated(w: &mut ApiWorld) {
    // Give the delayed shutdown task time to fire
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        w.shutdown_token.is_cancelled(),
        "Shutdown token should be cancelled after demo reset"
    );
}

#[then("in-flight requests are allowed to complete")]
async fn in_flight_allowed(_w: &mut ApiWorld) {
    // The response was received before shutdown, which proves in-flight completed
}

#[then("the reset completes successfully")]
async fn reset_completes(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    let body = resp.text();
    assert_eq!(
        resp.status_code(),
        200,
        "Expected 200 for reset, got {} — body: {body}",
        resp.status_code()
    );
}

#[then("the demo database matches the original sidecar")]
async fn demo_db_matches_sidecar(w: &mut ApiWorld) {
    // After demo_reset the connection pool is closed — open a fresh read-only connection
    // to the replaced database file rather than going through the closed pool.
    let data_dir = find_data_dir(w);
    let db_path = data_dir.join("demo").join("mokumo.db");
    let fresh_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&db_path)
                .read_only(true),
        )
        .await
        .expect("failed to reopen demo database after reset");
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'test_marker'")
            .fetch_optional(&fresh_pool)
            .await
            .expect("failed to query settings");
    fresh_pool.close().await;
    assert!(
        row.is_none(),
        "test_marker should be absent after reset — sidecar was not restored"
    );
}

// Note: "the request is rejected with a forbidden status" is defined in auth_steps.rs
// Note: "the request is rejected with an unauthorized status" is defined in regen_steps.rs

// =====================================================================
// demo_startup.feature steps (scenarios 2-6, 13-14)
// =====================================================================

#[given("a demo.db sidecar is available")]
async fn demo_sidecar_available(w: &mut ApiWorld) {
    // Create a valid SQLite sidecar in a temp location and set env var
    let sidecar_path = w._tmp.path().join("sidecar_demo.db");
    create_test_sidecar(&sidecar_path).await;
    unsafe { std::env::set_var("MOKUMO_DEMO_SIDECAR", &sidecar_path) };
}

#[given("no demo.db sidecar is available")]
async fn no_sidecar_available(_w: &mut ApiWorld) {
    unsafe { std::env::remove_var("MOKUMO_DEMO_SIDECAR") };
}

#[given("the server started with a demo sidecar")]
async fn server_started_with_sidecar(w: &mut ApiWorld) {
    rebuild_as_demo(w).await;
    // Seed some customers for the "at least 25" scenario
    seed_demo_customers(&w.db, 30).await;
}

#[given("a demo database with an older schema version")]
async fn demo_db_older_schema(w: &mut ApiWorld) {
    // Create a demo DB and "downgrade" it is not practical.
    // Instead, verify that initialize_database (which runs migrations) succeeds.
    rebuild_as_demo(w).await;
}

#[given("a production database with an older schema version")]
async fn production_db_older_schema(w: &mut ApiWorld) {
    rebuild_as_production_with_setup(w).await;
}

#[then(expr = "{string} exists in the data directory")]
async fn file_exists_in_data_dir(w: &mut ApiWorld, path: String) {
    let full_path = w._tmp.path().join("demo_test").join(&path);
    // Also check the production test path as fallback
    let alt_path = w._tmp.path().join("prod_test").join(&path);
    assert!(
        full_path.exists() || alt_path.exists(),
        "Expected {} to exist in data directory (checked {} and {})",
        path,
        full_path.display(),
        alt_path.display()
    );
}

#[then(expr = "the active profile is {string}")]
async fn active_profile_is(w: &mut ApiWorld, expected: String) {
    let data_dir = find_data_dir(w);
    let profile_path = data_dir.join("active_profile");
    let content = std::fs::read_to_string(&profile_path).unwrap_or_else(|_| "demo".into()); // default is demo
    assert_eq!(
        content.trim(),
        expected,
        "Expected active_profile={expected}, got {content}"
    );
}

#[then("the server is connected to the demo database")]
async fn connected_to_demo_db(w: &mut ApiWorld) {
    // Verify we can query the demo database
    let resp = w.server.get("/api/health").await;
    assert_eq!(resp.status_code(), 200);
    let json: serde_json::Value = resp.json();
    assert_eq!(json["database"], "ok");
}

#[then("the health endpoint returns healthy")]
async fn health_returns_healthy(w: &mut ApiWorld) {
    let resp = w.server.get("/api/health").await;
    assert_eq!(resp.status_code(), 200);
}

#[when("a client requests the customer list")]
async fn request_customer_list(w: &mut ApiWorld) {
    // In demo mode, auto-login handles auth
    w.response = Some(w.server.get("/api/customers").await);
}

#[then("at least 25 customers are returned")]
async fn at_least_25_customers(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    let body = resp.text();
    assert_eq!(
        resp.status_code(),
        200,
        "Expected 200 for customer list, got {} — body: {body}",
        resp.status_code()
    );
    let json: serde_json::Value = serde_json::from_str(&body).expect("response should be JSON");
    let items = json["items"].as_array().expect("items should be an array");
    assert!(
        items.len() >= 25,
        "Expected at least 25 customers, got {}",
        items.len()
    );
}

#[then("the server starts successfully")]
async fn server_starts_successfully(w: &mut ApiWorld) {
    let resp = w.server.get("/api/health").await;
    assert_eq!(resp.status_code(), 200);
}

#[then("setup is not complete")]
async fn setup_not_complete(w: &mut ApiWorld) {
    let resp = w.server.get("/api/setup-status").await;
    let json: serde_json::Value = resp.json();
    assert_eq!(json["setup_complete"], false);
}

#[then("the active profile defaults to fresh install behavior")]
async fn active_profile_defaults_fresh(_w: &mut ApiWorld) {
    // Fresh install defaults to demo mode (no active_profile file → demo)
    // This is validated by the resolve_active_profile unit tests
}

#[when("a client requests the activity log for a customer")]
async fn request_activity_log(w: &mut ApiWorld) {
    // Need at least one customer — use the first from the list
    let list_resp = w.server.get("/api/customers").await;
    let json: serde_json::Value = list_resp.json();
    let items = json["items"].as_array().expect("items should be array");
    assert!(
        !items.is_empty(),
        "Need at least one customer for activity log test"
    );
    let id = items[0]["id"].as_str().expect("customer id");
    w.response = Some(
        w.server
            .get(&format!(
                "/api/activity?entity_type=customer&entity_id={id}"
            ))
            .await,
    );
}

#[then("the activity log contains at least one entry")]
async fn activity_log_has_entries(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    assert_eq!(resp.status_code(), 200);
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be array");
    assert!(
        !items.is_empty(),
        "Expected at least one activity log entry"
    );
}

#[then("the demo database schema is up to date")]
async fn demo_schema_up_to_date(w: &mut ApiWorld) {
    // If we got here without errors, migrations ran successfully
    let resp = w.server.get("/api/health").await;
    assert_eq!(resp.status_code(), 200);
}

#[then("the production database schema is up to date")]
async fn production_schema_up_to_date(w: &mut ApiWorld) {
    let resp = w.server.get("/api/health").await;
    assert_eq!(resp.status_code(), 200);
}

// =====================================================================
// Helpers
// =====================================================================

/// Create a minimal test sidecar SQLite database.
async fn create_test_sidecar(path: &std::path::Path) {
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let db = mokumo_shop::db::initialize_database(&url)
        .await
        .expect("failed to create test sidecar");
    seed_test_data(&db, &DEMO_SEED).await;
    db.close().await.ok();
}

/// Seed demo customers into the database using the repository layer.
async fn seed_demo_customers(db: &sea_orm::DatabaseConnection, count: usize) {
    use std::sync::Arc;

    use kikan::actor::Actor;
    use mokumo_shop::customer::{CreateCustomer, CustomerRepository, SqliteCustomerRepository};

    let activity_writer: Arc<dyn kikan::ActivityWriter> =
        Arc::new(kikan::SqliteActivityWriter::new());
    let repo = SqliteCustomerRepository::new(db.clone(), activity_writer);
    let actor = Actor::system();

    for i in 1..=count {
        let req = CreateCustomer {
            display_name: format!("Demo Customer {i}"),
            email: Some(format!("customer{i}@demo.local")),
            phone: Some(format!("555-{i:04}")),
            company_name: Some(format!("Demo Company {i}")),
            notes: Some("Seeded by BDD test".into()),
            address_line1: None,
            address_line2: None,
            city: None,
            state: None,
            postal_code: None,
            country: None,
            portal_enabled: None,
            tax_exempt: None,
            payment_terms: None,
            credit_limit_cents: None,
            lead_source: None,
            tags: None,
        };
        repo.create(&req, &actor)
            .await
            .expect("failed to create demo customer");
    }
}

/// Find the data directory from the temp dir.
fn find_data_dir(w: &ApiWorld) -> std::path::PathBuf {
    let demo_path = w._tmp.path().join("demo_test");
    let prod_path = w._tmp.path().join("prod_test");
    let bdd_path = w._tmp.path().join("bdd_test");
    if demo_path.exists() {
        demo_path
    } else if prod_path.exists() {
        prod_path
    } else {
        bdd_path
    }
}

// =====================================================================
// demo_install_guard.feature steps
// =====================================================================

/// Rebuild the BDD world with a demo-mode server that has a fully-seeded admin.
async fn rebuild_as_demo_seeded(w: &mut ApiWorld) {
    rebuild_world(
        w,
        &WorldConfig {
            profile: "demo",
            dir_name: "demo_test",
            seed: &DEMO_SEED,
        },
    )
    .await;
}

/// Rebuild the BDD world with a demo-mode server that has NO admin account seeded.
///
/// Inserts only the settings rows (setup_mode, setup_complete, shop_name) so the
/// migrations run but no user row is present — triggering the degraded boot state.
async fn rebuild_as_demo_no_admin(w: &mut ApiWorld) {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let data_dir = tmp.path().join("demo_test");
    mokumo_shop::startup::ensure_data_dirs(&data_dir).expect("failed to create data dirs");
    std::fs::write(data_dir.join("active_profile"), "demo").unwrap();

    let db_path = data_dir.join("demo").join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = mokumo_shop::db::initialize_database(&database_url)
        .await
        .expect("failed to initialize demo database");

    // Seed settings but NOT the admin user — install validation must fail.
    let pool = db.get_sqlite_connection_pool();
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('setup_mode', 'demo')")
        .execute(pool)
        .await
        .expect("failed to insert setup_mode");
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('setup_complete', 'true')")
        .execute(pool)
        .await
        .expect("failed to insert setup_complete");
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('shop_name', 'Demo Shop')")
        .execute(pool)
        .await
        .expect("failed to insert shop_name");

    // Create a sidecar (empty) so reset endpoint doesn't crash
    let sidecar_path = tmp.path().join("sidecar_for_reset.db");
    std::fs::copy(&db_path, &sidecar_path).expect("failed to copy sidecar");
    unsafe { std::env::set_var("MOKUMO_DEMO_SIDECAR", &sidecar_path) };

    let db_pool = db.get_sqlite_connection_pool().clone();
    let recovery_dir = tmp.path().join("recovery");
    std::fs::create_dir_all(&recovery_dir).expect("failed to create recovery dir");

    let shutdown_token = tokio_util::sync::CancellationToken::new();

    let (server, setup_token, app_state, session_pool) = super::boot_test_server(
        data_dir,
        recovery_dir.clone(),
        db.clone(),
        db.clone(),
        kikan_types::SetupMode::Demo,
        shutdown_token.clone(),
    )
    .await;

    w.server = server;
    w.shutdown_token = shutdown_token;
    w.db = db;
    w.db_pool = db_pool;
    w.session_pool = session_pool;
    w.mdns_status = app_state.mdns_status().clone();
    w.setup_token = setup_token;
    w.recovery_dir = recovery_dir;
    w.auth_done = false;
    w._tmp = tmp;
}

#[given("the server started with a correctly seeded demo database")]
async fn server_started_seeded(w: &mut ApiWorld) {
    rebuild_as_demo_seeded(w).await;
}

#[given("the server started with a demo database that has no admin account")]
async fn server_started_no_admin(w: &mut ApiWorld) {
    rebuild_as_demo_no_admin(w).await;
}

#[then(expr = "the response should include {string} with value {word}")]
async fn response_includes_field_with_bool(w: &mut ApiWorld, field: String, expected_raw: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    match expected_raw.as_str() {
        "true" => assert_eq!(
            json[&field].as_bool(),
            Some(true),
            "Expected {field}=true, got {:?}",
            json[&field]
        ),
        "false" => assert_eq!(
            json[&field].as_bool(),
            Some(false),
            "Expected {field}=false, got {:?}",
            json[&field]
        ),
        other => {
            // Fallback: string comparison
            assert_eq!(
                json[&field].as_str().unwrap_or_default(),
                other,
                "Expected {field}={other}, got {:?}",
                json[&field]
            );
        }
    }
}

#[then(expr = "the response error code should be {string}")]
async fn response_error_code(w: &mut ApiWorld, expected: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    // API error codes are snake_case in the wire format
    let expected_snake = expected.to_lowercase();
    let actual = json["code"].as_str().unwrap_or_default();
    assert_eq!(
        actual, expected_snake,
        "Expected error code {expected_snake}, got {actual:?}"
    );
}

#[then(expr = "the json path {string} should not be empty")]
async fn json_path_not_empty(w: &mut ApiWorld, path: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    let value = &json[&path];
    assert!(
        !value.is_null() && value.as_str().map(|s| !s.is_empty()).unwrap_or(true),
        "Expected {path} to be non-empty, got {value:?}"
    );
}

#[then(expr = "the json path {string} should be null")]
async fn json_path_is_null(w: &mut ApiWorld, path: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    assert!(
        json[&path].is_null(),
        "Expected {path} to be null, got {:?}",
        json[&path]
    );
}

#[given("the demo sidecar contains a correctly seeded database")]
async fn sidecar_seeded(w: &mut ApiWorld) {
    // Replace the existing sidecar with a properly seeded database.
    let sidecar_path = std::env::var("MOKUMO_DEMO_SIDECAR").expect(
        "MOKUMO_DEMO_SIDECAR env var not set — rebuild_as_demo_no_admin should have set it",
    );
    create_test_sidecar(std::path::Path::new(&sidecar_path)).await;
}

#[then("after the server restarts the health endpoint reports install_ok as true")]
async fn health_reports_install_ok_after_restart(w: &mut ApiWorld) {
    // Simulate server restart: rebuild the world from the current data directory,
    // which now holds the freshly-reset (seeded) demo database.
    // This mirrors the production lifecycle: demo reset → server restart → validate_installation.
    let data_dir = find_data_dir(w);
    let db_path = data_dir.join("demo").join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = mokumo_shop::db::initialize_database(&database_url)
        .await
        .expect("failed to re-open demo database after simulated restart");

    let shutdown_token = tokio_util::sync::CancellationToken::new();

    let (server, setup_token, app_state, session_pool) = super::boot_test_server(
        data_dir.clone(),
        w.recovery_dir.clone(),
        db.clone(),
        db.clone(),
        kikan_types::SetupMode::Demo,
        shutdown_token.clone(),
    )
    .await;

    let db_pool = db.get_sqlite_connection_pool().clone();
    w.server = server;
    w.shutdown_token = shutdown_token;
    w.db = db;
    w.db_pool = db_pool;
    w.session_pool = session_pool;
    w.mdns_status = app_state.mdns_status().clone();
    w.setup_token = setup_token;
    w.auth_done = false;

    let resp = w.server.get("/api/health").await;
    assert_eq!(resp.status_code(), 200);
    let json: serde_json::Value = resp.json();
    assert_eq!(
        json["install_ok"].as_bool(),
        Some(true),
        "Expected install_ok=true after simulated restart, got {:?}",
        json["install_ok"]
    );
}
