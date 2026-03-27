use super::ApiWorld;
use cucumber::{given, then, when};

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

// --- Database failure ---

#[given("the database is unavailable")]
async fn database_unavailable(w: &mut ApiWorld) {
    // Create a pool and immediately close it — queries will fail with PoolClosed
    let bad_pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to create in-memory pool");
    bad_pool.close().await;

    let config = mokumo_api::ServerConfig {
        port: 0,
        host: "127.0.0.1".into(),
        data_dir: std::path::PathBuf::from("/tmp/mokumo_bdd_bad_db"),
    };

    let shutdown = tokio_util::sync::CancellationToken::new();
    let mdns_status = mokumo_api::discovery::MdnsStatus::shared();
    let app = mokumo_api::build_app_with_shutdown(&config, bad_pool, shutdown.clone(), mdns_status);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind test listener");

    let shutdown_clone = shutdown.clone();
    let serve = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async move { shutdown_clone.cancelled().await });

    let server = axum_test::TestServer::builder()
        .build(serve)
        .expect("failed to create test server");

    w.server = server;
    w.shutdown_token = shutdown;
}
