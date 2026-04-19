use super::ApiWorld;
use cucumber::{given, then, when};

// --- Steps for scenarios 1-3: automatic activity logging ---

#[when(expr = "I create a customer {string}")]
async fn create_customer(w: &mut ApiWorld, name: String) {
    let body = serde_json::json!({ "display_name": name });
    let resp = w.server.post("/api/customers").json(&body).await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let json: serde_json::Value = resp.json();
    let id = json["id"].as_str().unwrap().to_string();
    w.last_customer_id = Some(id.clone());
    w.customer_ids.push(id);
}

#[when("I delete that customer")]
async fn delete_that_customer(w: &mut ApiWorld) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    let resp = w.server.delete(&format!("/api/customers/{id}")).await;
    resp.assert_status(axum::http::StatusCode::OK);
}

#[then(expr = "the activity log for that customer should have {int} entry")]
async fn activity_log_entry_count(w: &mut ApiWorld, count: usize) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    w.response = Some(
        w.server
            .get(&format!(
                "/api/activity?entity_type=customer&entity_id={id}"
            ))
            .await,
    );
    let json: serde_json::Value = w.response.as_ref().unwrap().json();
    let items = json["items"].as_array().expect("items should be an array");
    assert_eq!(
        items.len(),
        count,
        "Expected {count} activity entries, got {}",
        items.len()
    );
}

#[then(expr = "the latest activity action should be {string}")]
async fn latest_action_should_be(w: &mut ApiWorld, expected: String) {
    let resp = w
        .response
        .as_ref()
        .expect("no response — query activity first");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    let latest = items.first().expect("no activity entries");
    assert_eq!(latest["action"].as_str().unwrap(), expected);
}

#[then("the activity actor should be the authenticated user")]
async fn activity_actor_should_be_authenticated_user(w: &mut ApiWorld) {
    // Get the actual authenticated user's ID from /api/auth/me
    let me_resp = w.server.get("/api/auth/me").await;
    me_resp.assert_status(axum::http::StatusCode::OK);
    let me_json: serde_json::Value = me_resp.json();
    let expected_user_id = me_json["user"]["id"]
        .as_i64()
        .expect("user.id should be numeric");

    let resp = w
        .response
        .as_ref()
        .expect("no response — query activity first");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    let latest = items.first().expect("no activity entries");
    let actor_id = latest["actor_id"].as_str().unwrap();
    let actor_type = latest["actor_type"].as_str().unwrap();
    assert_eq!(
        actor_id,
        expected_user_id.to_string(),
        "actor_id should match the authenticated user's ID"
    );
    assert_eq!(actor_type, "user", "actor_type should be 'user'");
}

#[then("the activity payload should contain the customer snapshot")]
async fn payload_contains_customer_snapshot(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    let latest = items.first().expect("no activity entries");
    let payload = &latest["payload"];
    assert!(
        payload["id"].is_string(),
        "payload should contain customer id"
    );
    assert!(
        payload["display_name"].is_string(),
        "payload should contain display_name"
    );
    assert!(
        payload["created_at"].is_string(),
        "payload should contain created_at"
    );
}

#[then(expr = "the latest activity action for that customer should be {string}")]
async fn latest_action_for_customer(w: &mut ApiWorld, expected: String) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    w.response = Some(
        w.server
            .get(&format!(
                "/api/activity?entity_type=customer&entity_id={id}"
            ))
            .await,
    );
    let json: serde_json::Value = w.response.as_ref().unwrap().json();
    let items = json["items"].as_array().expect("items should be an array");
    let latest = items.first().expect("no activity entries");
    assert_eq!(latest["action"].as_str().unwrap(), expected);
}

#[then("the activity payload should reflect the updated name")]
async fn payload_reflects_updated_name(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    let latest = items.first().expect("no activity entries");
    let payload = &latest["payload"];
    assert_eq!(
        payload["display_name"].as_str().unwrap(),
        "Acme Industries",
        "payload should reflect the updated name"
    );
}

#[given(expr = "that customer has been updated twice")]
async fn customer_updated_twice(w: &mut ApiWorld) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    // Service layer already logged "created" when the customer was made.
    // Seed 2 "updated" entries via SQL to simulate two updates.
    for i in 0..2 {
        let ts = format!("2026-01-01T00:00:0{}Z", i + 1);
        sqlx::query(
            "INSERT INTO activity_log (entity_type, entity_id, action, actor_id, actor_type, payload, created_at) \
             VALUES ('customer', ?1, 'updated', 'system', 'system', '{}', ?2)",
        )
        .bind(id)
        .bind(&ts)
        .execute(&w.db_pool)
        .await
        .expect("failed to seed activity entry");
    }
}

#[given(expr = "a customer exists with {int} activity entries")]
async fn customer_with_n_activity_entries(w: &mut ApiWorld, count: usize) {
    // Create a customer first — service layer logs 1 "created" entry automatically
    let body = serde_json::json!({ "display_name": "Bulk Activity Customer" });
    let resp = w.server.post("/api/customers").json(&body).await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let json: serde_json::Value = resp.json();
    let id = json["id"].as_str().unwrap().to_string();
    w.last_customer_id = Some(id.clone());

    // Seed remaining entries via SQL (service already created 1)
    let remaining = count.saturating_sub(1);
    for i in 0..remaining {
        let ts = format!(
            "2026-01-01T{:02}:{:02}:{:02}Z",
            i / 3600,
            (i % 3600) / 60,
            i % 60
        );
        sqlx::query(
            "INSERT INTO activity_log (entity_type, entity_id, action, actor_id, actor_type, payload, created_at) \
             VALUES ('customer', ?1, 'updated', 'system', 'system', '{}', ?2)",
        )
        .bind(&id)
        .bind(&ts)
        .execute(&w.db_pool)
        .await
        .expect("failed to seed activity entry");
    }
}

#[given("a customer has activity entries")]
async fn customer_has_activity_entries(w: &mut ApiWorld) {
    // Create a customer
    let body = serde_json::json!({ "display_name": "Activity Customer" });
    let resp = w.server.post("/api/customers").json(&body).await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let json: serde_json::Value = resp.json();
    let id = json["id"].as_str().unwrap().to_string();
    w.last_customer_id = Some(id.clone());

    // Seed a couple entries
    for action in ["created", "updated"] {
        sqlx::query(
            "INSERT INTO activity_log (entity_type, entity_id, action, actor_id, actor_type, payload) \
             VALUES ('customer', ?1, ?2, 'system', 'system', '{}')",
        )
        .bind(&id)
        .bind(action)
        .execute(&w.db_pool)
        .await
        .expect("failed to seed activity entry");
    }
}

#[when("I query activity for that customer")]
async fn query_activity_for_customer(w: &mut ApiWorld) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    w.response = Some(
        w.server
            .get(&format!(
                "/api/activity?entity_type=customer&entity_id={id}"
            ))
            .await,
    );
}

#[when(expr = "I query activity for entity type {string}")]
async fn query_activity_by_type(w: &mut ApiWorld, entity_type: String) {
    // S3 test strategy: seed activity entries for all known customers via direct SQL
    for id in &w.customer_ids {
        sqlx::query(
            "INSERT INTO activity_log (entity_type, entity_id, action, actor_id, actor_type, payload) \
             VALUES ('customer', ?1, 'created', 'system', 'system', '{}')",
        )
        .bind(id)
        .execute(&w.db_pool)
        .await
        .expect("failed to seed activity entry");
    }

    w.response = Some(
        w.server
            .get(&format!("/api/activity?entity_type={entity_type}"))
            .await,
    );
}

#[when(expr = "I query activity for that customer with {int} per page")]
async fn query_activity_paginated(w: &mut ApiWorld, per_page: u32) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    w.response = Some(
        w.server
            .get(&format!(
                "/api/activity?entity_type=customer&entity_id={id}&per_page={per_page}"
            ))
            .await,
    );
}

#[when("I query activity for a non-existent entity")]
async fn query_activity_nonexistent(w: &mut ApiWorld) {
    let fake_id = uuid::Uuid::new_v4();
    w.response = Some(
        w.server
            .get(&format!(
                "/api/activity?entity_type=customer&entity_id={fake_id}"
            ))
            .await,
    );
}

#[then(expr = "I should see {int} activity entries")]
async fn should_see_n_entries(w: &mut ApiWorld, count: usize) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    assert_eq!(
        items.len(),
        count,
        "Expected {count} activity entries, got {}",
        items.len()
    );
}

#[then("the entries should be in newest-first order")]
async fn entries_newest_first(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    for pair in items.windows(2) {
        let a = pair[0]["created_at"].as_str().unwrap();
        let b = pair[1]["created_at"].as_str().unwrap();
        assert!(a >= b, "Expected newest-first order, but {a} < {b}");
    }
}

#[then("I should see activity entries for both customers")]
async fn see_entries_for_both(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    let entity_ids: std::collections::HashSet<&str> = items
        .iter()
        .filter_map(|i| i["entity_id"].as_str())
        .collect();
    assert!(
        entity_ids.len() >= 2,
        "Expected entries for at least 2 customers, got entity_ids: {entity_ids:?}"
    );
}

#[then("there is no endpoint to update activity entries")]
async fn no_update_endpoint(w: &mut ApiWorld) {
    let resp = w.server.put("/api/activity/1").await;
    let status = resp.status_code().as_u16();
    assert!(
        status == 404 || status == 405,
        "Expected 404 or 405 for PUT /api/activity/1, got {status}"
    );
}

#[then("there is no endpoint to delete activity entries")]
async fn no_delete_endpoint(w: &mut ApiWorld) {
    let resp = w.server.delete("/api/activity/1").await;
    let status = resp.status_code().as_u16();
    assert!(
        status == 404 || status == 405,
        "Expected 404 or 405 for DELETE /api/activity/1, got {status}"
    );
}
