pub mod activity_steps;
pub mod auth_steps;
pub mod customer_steps;
pub mod demo_steps;
pub mod diagnostics_steps;
pub mod discovery_steps;
pub mod ephemeral_bind_steps;
pub mod health_steps;
pub mod lock_steps;
pub mod mdns_retry_steps;
pub mod offline_startup_steps;
pub mod port_fallback_steps;
pub mod regen_steps;
pub mod restore_steps;
pub mod shop_logo_steps;
pub mod shutdown_steps;

use std::collections::HashMap;
use std::path::PathBuf;

use axum_test::TestServer;
use axum_test::TestWebSocket;
use cucumber::{World, given, then, when};
use mokumo_db::DatabaseConnection;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use mokumo_api::discovery::{MdnsStatus, SharedMdnsStatus};
use mokumo_api::{ServerConfig, build_app_with_shutdown, ensure_data_dirs};

#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct ApiWorld {
    pub server: TestServer,
    pub response: Option<axum_test::TestResponse>,
    pub shutdown_token: CancellationToken,
    pub ws_clients: Vec<TestWebSocket>,
    pub last_received_event: Option<serde_json::Value>,
    pub last_broadcast_type: Option<String>,
    pub broadcast_response: Option<axum_test::TestResponse>,
    pub previous_uptime: Option<u64>,
    pub last_customer_id: Option<String>,
    pub customer_ids: Vec<String>,
    pub customer_names: HashMap<String, String>,
    pub db: DatabaseConnection,
    pub db_pool: SqlitePool,
    pub session_pool: SqlitePool,
    pub mdns_status: SharedMdnsStatus,
    pub mdns_host: String,
    pub mdns_should_fail: bool,
    // Auth fields
    pub setup_token: Option<String>,
    pub recovery_codes: Vec<String>,
    pub original_recovery_codes: Vec<String>,
    pub auth_done: bool,
    // File-drop reset fields
    pub recovery_dir: PathBuf,
    pub last_pin: Option<String>,
    // Hold the tempdir alive for the lifetime of the world
    pub _tmp: tempfile::TempDir,
    // Restore step state
    pub restore_data_dir: Option<PathBuf>,
    pub restore_file_tmp: Option<tempfile::TempDir>,
    pub restore_in_progress_simulated: bool,
    // Ephemeral bind step state
    pub ephemeral_addr: Option<std::net::SocketAddr>,
    pub ephemeral_listener: Option<tokio::net::TcpListener>,
}

impl ApiWorld {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let data_dir = tmp.path().join("bdd_test");
        ensure_data_dirs(&data_dir).expect("failed to create data dirs");

        let recovery_dir = tmp.path().join("recovery");
        std::fs::create_dir_all(&recovery_dir).expect("failed to create recovery dir");

        // Use production/ subdirectory matching the dual-directory layout
        let db_path = data_dir.join("production").join("mokumo.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let db = mokumo_shop::db::initialize_database(&database_url)
            .await
            .expect("failed to initialize database");
        let pool = db.get_sqlite_connection_pool().clone();

        // Open the session pool so BDD steps can manipulate sessions directly
        let session_db_path = data_dir.join("sessions.db");
        let session_url = format!("sqlite:{}?mode=rwc", session_db_path.display());
        let session_pool = mokumo_db::open_raw_sqlite_pool(&session_url)
            .await
            .expect("failed to open session database for BDD");

        let config = ServerConfig {
            port: 0,
            host: "0.0.0.0".into(),
            data_dir,
            recovery_dir: recovery_dir.clone(),
            #[cfg(debug_assertions)]
            ws_ping_ms: None,
        };

        let shutdown_token = CancellationToken::new();
        let mdns_status = MdnsStatus::shared();
        let (app, setup_token, _ws) = build_app_with_shutdown(
            &config,
            db.clone(),
            db.clone(),
            kikan::SetupMode::Production,
            shutdown_token.clone(),
            mdns_status.clone(),
        )
        .await
        .unwrap();

        // Pre-bind with OS-assigned port to bypass axum-test's reserve_port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind test listener");

        let shutdown = shutdown_token.clone();
        let serve =
            axum::serve(listener, app.into_make_service()).with_graceful_shutdown(async move {
                shutdown.cancelled().await;
            });

        let server = TestServer::builder().save_cookies().build(serve);

        Self {
            server,
            response: None,
            shutdown_token,
            ws_clients: Vec::new(),
            last_received_event: None,
            last_broadcast_type: None,
            broadcast_response: None,
            previous_uptime: None,
            last_customer_id: None,
            customer_ids: Vec::new(),
            customer_names: HashMap::new(),
            db,
            db_pool: pool,
            session_pool,
            mdns_status,
            mdns_host: "0.0.0.0".into(),
            mdns_should_fail: false,
            setup_token,
            recovery_codes: Vec::new(),
            original_recovery_codes: Vec::new(),
            auth_done: false,
            recovery_dir,
            last_pin: None,
            _tmp: tmp,
            restore_data_dir: None,
            restore_file_tmp: None,
            restore_in_progress_simulated: false,
            ephemeral_addr: None,
            ephemeral_listener: None,
        }
    }

    /// Programmatically complete setup and login so protected routes are accessible.
    /// Uses the setup API with the real token to ensure AppState is properly updated.
    pub async fn ensure_auth(&mut self) {
        if self.auth_done {
            return;
        }

        let token = self
            .setup_token
            .as_ref()
            .expect("setup_token should be set for fresh server")
            .clone();

        // Use the actual setup endpoint so AppState.setup_completed gets updated
        let resp = self
            .server
            .post("/api/setup")
            .json(&serde_json::json!({
                "shop_name": "Test Shop",
                "admin_name": "Admin",
                "admin_email": "admin@test.local",
                "admin_password": "testpassword123",
                "setup_token": token
            }))
            .await;
        assert_eq!(
            resp.status_code(),
            201,
            "BDD auth bootstrap setup failed: {}",
            resp.text()
        );

        let body: serde_json::Value = resp.json();
        if let Some(codes) = body["recovery_codes"].as_array() {
            self.recovery_codes = codes
                .iter()
                .filter_map(|c| c.as_str().map(String::from))
                .collect();
            self.original_recovery_codes = self.recovery_codes.clone();
        }

        // Setup auto-logs in, but verify we're authenticated
        let me_resp = self.server.get("/api/auth/me").await;
        assert_eq!(
            me_resp.status_code(),
            200,
            "BDD auth bootstrap: not authenticated after setup"
        );

        self.auth_done = true;
    }
}

// ---- Existing steps ----

#[given("the API server is running")]
async fn server_running(w: &mut ApiWorld) {
    w.ensure_auth().await;
}

#[when(expr = "I request GET {string}")]
async fn get_request(w: &mut ApiWorld, path: String) {
    w.response = Some(w.server.get(&path).await);
}

#[then(expr = "the response status should be {int}")]
async fn check_status(w: &mut ApiWorld, status: u16) {
    let resp = w.response.as_ref().expect("no response captured");
    resp.assert_status(axum::http::StatusCode::from_u16(status).unwrap());
}

// ---- WebSocket connection steps ----

#[when(expr = "a client connects to {string}")]
async fn client_connects(w: &mut ApiWorld, path: String) {
    let ws = w.server.get_websocket(&path).await.into_websocket().await;
    w.ws_clients.push(ws);
}

#[given(expr = "a client is connected to {string}")]
async fn client_already_connected(w: &mut ApiWorld, path: String) {
    w.ensure_auth().await;
    let ws = w.server.get_websocket(&path).await.into_websocket().await;
    w.ws_clients.push(ws);
}

#[then("the connection is accepted")]
async fn connection_accepted(w: &mut ApiWorld) {
    assert!(!w.ws_clients.is_empty(), "Expected at least one WS client");
}

async fn assert_connection_count(w: &ApiWorld, count: usize) {
    let deadline = std::time::Duration::from_secs(2);
    let poll = std::time::Duration::from_millis(25);
    tokio::time::timeout(deadline, async {
        loop {
            let resp = w.server.get("/api/debug/connections").await;
            let json: serde_json::Value = resp.json();
            if json["count"].as_u64().unwrap() as usize == count {
                return;
            }
            tokio::time::sleep(poll).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Timed out waiting for connection count to reach {count}"));
}

#[then(expr = "the server tracks {int} connected client")]
async fn server_tracks_one_client(w: &mut ApiWorld, count: usize) {
    assert_connection_count(w, count).await;
}

#[then(expr = "the server tracks {int} connected clients")]
async fn server_tracks_clients(w: &mut ApiWorld, count: usize) {
    assert_connection_count(w, count).await;
}

#[when(expr = "{int} clients connect to {string}")]
async fn multiple_clients_connect(w: &mut ApiWorld, count: usize, path: String) {
    for _ in 0..count {
        let ws = w.server.get_websocket(&path).await.into_websocket().await;
        w.ws_clients.push(ws);
    }
}

#[when("the client disconnects")]
async fn client_disconnects(w: &mut ApiWorld) {
    if let Some(ws) = w.ws_clients.pop() {
        drop(ws);
    }
}

#[when("the client sends a text message")]
async fn client_sends_text(w: &mut ApiWorld) {
    if let Some(ws) = w.ws_clients.last_mut() {
        ws.send_text("hello").await;
    }
}

#[then("the connection remains open")]
async fn connection_remains_open(w: &mut ApiWorld) {
    assert!(!w.ws_clients.is_empty(), "Expected at least one WS client");
}

// ---- WebSocket broadcast steps ----

#[given(expr = "{int} clients are connected to {string}")]
async fn n_clients_already_connected(w: &mut ApiWorld, count: usize, path: String) {
    w.ensure_auth().await;
    for _ in 0..count {
        let ws = w.server.get_websocket(&path).await.into_websocket().await;
        w.ws_clients.push(ws);
    }
}

#[given("no clients are connected")]
async fn no_clients_connected(w: &mut ApiWorld) {
    w.ensure_auth().await;
}

#[when(expr = "a {string} event is broadcast")]
async fn broadcast_event(w: &mut ApiWorld, event_type: String) {
    let body = serde_json::json!({
        "type": event_type,
    });
    w.last_broadcast_type = Some(event_type);
    w.broadcast_response = Some(w.server.post("/api/debug/broadcast").json(&body).await);
    // Yield to let sender tasks forward the broadcast
    tokio::task::yield_now().await;
}

#[when(expr = "a {string} event is broadcast with payload '{}'")]
async fn broadcast_event_with_payload(w: &mut ApiWorld, event_type: String, payload: String) {
    let payload_value: serde_json::Value =
        serde_json::from_str(&payload).expect("invalid payload JSON");
    let body = serde_json::json!({
        "type": event_type,
        "payload": payload_value,
    });
    w.last_broadcast_type = Some(event_type);
    w.broadcast_response = Some(w.server.post("/api/debug/broadcast").json(&body).await);
    tokio::task::yield_now().await;
}

#[then(expr = "the client receives a message with type {string}")]
async fn client_receives_message_with_type(w: &mut ApiWorld, expected_type: String) {
    let ws = w.ws_clients.first_mut().expect("no WS client");
    let text = ws.receive_text().await;
    let event: serde_json::Value = serde_json::from_str(&text).expect("invalid JSON from WS");
    assert_eq!(event["type"], expected_type);
    w.last_received_event = Some(event);
}

#[then(expr = "the message has version {int}")]
async fn message_has_version(w: &mut ApiWorld, version: u64) {
    let event = w.last_received_event.as_ref().expect("no received event");
    assert_eq!(event["v"], version);
}

#[then(expr = "the message has topic {string}")]
async fn message_has_topic(w: &mut ApiWorld, topic: String) {
    let event = w.last_received_event.as_ref().expect("no received event");
    assert_eq!(event["topic"], topic);
}

#[then(expr = "all {int} clients receive the message")]
async fn all_clients_receive_message(w: &mut ApiWorld, count: usize) {
    let expected_type = w
        .last_broadcast_type
        .as_ref()
        .expect("no broadcast event type stored");
    assert_eq!(w.ws_clients.len(), count, "Expected {count} clients");
    for ws in &mut w.ws_clients {
        let text = ws.receive_text().await;
        let event: serde_json::Value = serde_json::from_str(&text).expect("invalid JSON from WS");
        assert_eq!(event["type"], expected_type.as_str());
    }
}

#[then(expr = "the message payload contains {string} with value {int}")]
async fn payload_contains_int(w: &mut ApiWorld, key: String, value: i64) {
    let event = w.last_received_event.as_ref().expect("no received event");
    assert_eq!(event["payload"][&key], value);
}

#[then(expr = "the message payload contains {string} with value {string}")]
async fn payload_contains_string(w: &mut ApiWorld, key: String, value: String) {
    let event = w.last_received_event.as_ref().expect("no received event");
    assert_eq!(event["payload"][&key], value);
}

#[then("the broadcast completes without error")]
async fn broadcast_completes_without_error(w: &mut ApiWorld) {
    let resp = w
        .broadcast_response
        .as_ref()
        .expect("no broadcast response");
    resp.assert_status(axum::http::StatusCode::OK);
}

// ---- WebSocket shutdown steps ----

#[when("the server begins shutting down")]
async fn server_begins_shutdown(w: &mut ApiWorld) {
    // Deregister mDNS before cancelling token (mirrors production shutdown handler)
    if w.mdns_status.read().active {
        let mut s = w.mdns_status.write();
        s.active = false;
    }
    w.shutdown_token.cancel();
    // Yield to let the sender task send the close frame
    tokio::task::yield_now().await;
}

#[then(expr = "the client receives a close frame with code {int}")]
async fn client_receives_close_frame(w: &mut ApiWorld, code: u16) {
    let ws = w.ws_clients.last_mut().expect("no WS client");
    let msg = ws.receive_message().await;
    match msg {
        axum_test::WsMessage::Close(Some(frame)) => {
            let actual: u16 = frame.code.into();
            assert_eq!(actual, code, "Expected close code {code}, got {actual}");
        }
        other => panic!("Expected Close frame with code {code}, got: {other:?}"),
    }
}
