use axum_test::TestServer;
use axum_test::TestWebSocket;
use cucumber::{World, given, then, when};
use tokio_util::sync::CancellationToken;

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
    // Hold the tempdir alive for the lifetime of the world
    _tmp: tempfile::TempDir,
}

impl ApiWorld {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let data_dir = tmp.path().join("bdd_test");
        ensure_data_dirs(&data_dir).expect("failed to create data dirs");

        let db_path = data_dir.join("mokumo.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = mokumo_db::initialize_database(&database_url)
            .await
            .expect("failed to initialize database");

        let config = ServerConfig {
            port: 0,
            host: "127.0.0.1".into(),
            data_dir,
        };

        let shutdown_token = CancellationToken::new();
        let app = build_app_with_shutdown(&config, pool, shutdown_token.clone());

        // Real TCP for ALL scenarios: WebSocket tests require actual sockets
        // (axum-test's mock transport doesn't support WS upgrade). Using the same
        // transport for HTTP-only scenarios keeps the test world simple and tests
        // more realistic. We use HttpIpPort with port 0 (OS-assigned) instead of
        // HttpRandomPort to avoid the portpicker crate, which can fail in
        // constrained CI environments.
        let server = TestServer::new_with_config(
            app,
            axum_test::TestServerConfig {
                // Real TCP for ALL scenarios: WebSocket tests require actual sockets
                // (axum-test's mock transport doesn't support WS upgrade). Using the
                // same transport for HTTP-only scenarios keeps the world simple and
                // tests more realistic. HttpIpPort with None/None avoids the
                // portpicker crate used by HttpRandomPort, which can fail in
                // constrained CI environments — the OS assigns the port directly.
                transport: Some(axum_test::Transport::HttpIpPort {
                    ip: None,
                    port: None,
                }),
                ..Default::default()
            },
        )
        .expect("failed to create test server");

        Self {
            server,
            response: None,
            shutdown_token,
            ws_clients: Vec::new(),
            last_received_event: None,
            last_broadcast_type: None,
            broadcast_response: None,
            _tmp: tmp,
        }
    }
}

// ---- Existing steps ----

#[given("the API server is running")]
async fn server_running(_w: &mut ApiWorld) {
    // Server is created in World::new — this step is a no-op placeholder
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
    // The fact that we got here means the connection survived the text message.
    assert!(!w.ws_clients.is_empty(), "Expected at least one WS client");
}

// ---- WebSocket broadcast steps ----

#[given(expr = "{int} clients are connected to {string}")]
async fn n_clients_already_connected(w: &mut ApiWorld, count: usize, path: String) {
    for _ in 0..count {
        let ws = w.server.get_websocket(&path).await.into_websocket().await;
        w.ws_clients.push(ws);
    }
}

#[given("no clients are connected")]
async fn no_clients_connected(_w: &mut ApiWorld) {
    // No-op — no clients connected by default
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
