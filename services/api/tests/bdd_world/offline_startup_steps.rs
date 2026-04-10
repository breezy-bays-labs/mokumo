use super::ApiWorld;
use cucumber::{given, then, when};

// ---- Given steps ----

#[given("setup is completed")]
async fn setup_completed(w: &mut ApiWorld) {
    w.ensure_auth().await;
}

// ---- When steps ----

#[when("the diagnostics endpoint is requested")]
async fn request_diagnostics(w: &mut ApiWorld) {
    w.response = Some(w.server.get("/api/diagnostics").await);
}

// ---- Then steps ----

#[then("the health response includes database ok")]
async fn health_database_ok(w: &mut ApiWorld) {
    let resp = w.server.get("/api/health").await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(
        body["database"].as_str(),
        Some("ok"),
        "Expected health.database to be \"ok\", got: {:?}",
        body["database"]
    );
}

#[then("the diagnostics return 200")]
async fn diagnostics_return_200(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    resp.assert_status_ok();
}

#[then("the diagnostics show mdns_active is false")]
async fn diagnostics_mdns_inactive(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    let body: serde_json::Value = resp.json();
    let mdns_active = body["runtime"]["mdns_active"]
        .as_bool()
        .expect("runtime.mdns_active should be a boolean");
    assert!(
        !mdns_active,
        "Expected runtime.mdns_active to be false, got true"
    );
}
