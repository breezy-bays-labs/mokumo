use super::ApiWorld;
use cucumber::{given, then, when};

#[given(expr = "a customer {string} exists")]
async fn customer_with_name_exists(w: &mut ApiWorld, name: String) {
    let body = serde_json::json!({ "display_name": name });
    let resp = w.server.post("/api/customers").json(&body).await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let json: serde_json::Value = resp.json();
    let id = json["id"].as_str().unwrap().to_string();
    w.last_customer_id = Some(id.clone());
    w.customer_names.insert(name, id.clone());
    w.customer_ids.push(id);
}

#[given(expr = "a customer {string} exists with email {string}")]
async fn customer_with_name_and_email_exists(w: &mut ApiWorld, name: String, email: String) {
    let body = serde_json::json!({ "display_name": name, "email": email });
    let resp = w.server.post("/api/customers").json(&body).await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let json: serde_json::Value = resp.json();
    let id = json["id"].as_str().unwrap().to_string();
    w.last_customer_id = Some(id.clone());
    w.customer_names.insert(name, id.clone());
    w.customer_ids.push(id);
}

#[given("a customer exists")]
async fn customer_exists(w: &mut ApiWorld) {
    let body = serde_json::json!({ "display_name": "Test Customer" });
    let resp = w.server.post("/api/customers").json(&body).await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let json: serde_json::Value = resp.json();
    let id = json["id"].as_str().unwrap().to_string();
    w.last_customer_id = Some(id.clone());
    w.customer_ids.push(id);
}

#[given(expr = "{int} customers exist")]
async fn n_customers_exist(w: &mut ApiWorld, count: usize) {
    for i in 0..count {
        let body = serde_json::json!({ "display_name": format!("Customer {}", i + 1) });
        let resp = w.server.post("/api/customers").json(&body).await;
        resp.assert_status(axum::http::StatusCode::CREATED);
        let json: serde_json::Value = resp.json();
        let id = json["id"].as_str().unwrap().to_string();
        w.customer_ids.push(id);
    }
}

#[given("that customer has been deleted")]
async fn that_customer_deleted(w: &mut ApiWorld) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    let resp = w.server.delete(&format!("/api/customers/{id}")).await;
    resp.assert_status(axum::http::StatusCode::OK);
}

#[given(expr = "{string} has been deleted")]
async fn named_customer_deleted(w: &mut ApiWorld, name: String) {
    let id = w
        .customer_names
        .get(&name)
        .unwrap_or_else(|| panic!("customer '{name}' not found by name"))
        .clone();
    let resp = w.server.delete(&format!("/api/customers/{id}")).await;
    resp.assert_status(axum::http::StatusCode::OK);
}

#[when(expr = "I create a customer with display name {string}")]
async fn create_customer_with_name(w: &mut ApiWorld, name: String) {
    let body = serde_json::json!({ "display_name": name });
    w.response = Some(w.server.post("/api/customers").json(&body).await);
}

#[when("I create a customer with full details")]
async fn create_customer_full_details(w: &mut ApiWorld) {
    let body = serde_json::json!({
        "display_name": "Full Details Corp",
        "company_name": "Full Details Inc",
        "email": "contact@fulldetails.com",
        "phone": "555-1234",
        "address_line1": "123 Main St",
        "address_line2": "Suite 100",
        "city": "Portland",
        "state": "OR",
        "postal_code": "97201",
        "country": "US",
        "notes": "VIP customer",
        "portal_enabled": true,
        "tax_exempt": true,
        "payment_terms": "net_30",
        "credit_limit_cents": 500000,
        "lead_source": "referral",
        "tags": "[\"vip\",\"wholesale\"]"
    });
    w.response = Some(w.server.post("/api/customers").json(&body).await);
}

#[when("I retrieve that customer by ID")]
async fn retrieve_customer_by_id(w: &mut ApiWorld) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    w.response = Some(w.server.get(&format!("/api/customers/{id}")).await);
}

#[when("I retrieve a customer with a random UUID")]
async fn retrieve_random_customer(w: &mut ApiWorld) {
    let random_id = uuid::Uuid::new_v4();
    w.response = Some(w.server.get(&format!("/api/customers/{random_id}")).await);
}

#[when("I list customers")]
async fn list_customers(w: &mut ApiWorld) {
    w.response = Some(w.server.get("/api/customers").await);
}

#[when(expr = "I update that customer's display name to {string}")]
async fn update_customer_name(w: &mut ApiWorld, name: String) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    let body = serde_json::json!({ "display_name": name });
    w.response = Some(
        w.server
            .put(&format!("/api/customers/{id}"))
            .json(&body)
            .await,
    );
}

#[when("I update that customer with email set to null")]
async fn update_customer_clear_email(w: &mut ApiWorld) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    let body = serde_json::json!({ "email": null });
    w.response = Some(
        w.server
            .put(&format!("/api/customers/{id}"))
            .json(&body)
            .await,
    );
}

#[when("I update that customer")]
async fn update_customer_generic(w: &mut ApiWorld) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    // Delay to ensure updated_at (second-level precision) differs from created_at
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let body = serde_json::json!({ "display_name": "Updated Customer" });
    w.response = Some(
        w.server
            .put(&format!("/api/customers/{id}"))
            .json(&body)
            .await,
    );
}

#[when("I retrieve that customer by ID including deleted")]
async fn retrieve_customer_including_deleted(w: &mut ApiWorld) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    w.response = Some(
        w.server
            .get(&format!("/api/customers/{id}?include_deleted=true"))
            .await,
    );
}

#[when("I list customers including deleted")]
async fn list_customers_including_deleted(w: &mut ApiWorld) {
    w.response = Some(w.server.get("/api/customers?include_deleted=true").await);
}

#[when("I delete that customer again")]
async fn delete_customer_again(w: &mut ApiWorld) {
    let id = w.last_customer_id.as_ref().expect("no customer created");
    w.response = Some(w.server.delete(&format!("/api/customers/{id}")).await);
}

#[then("the customer should have a UUID identifier")]
async fn customer_has_uuid(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let id = json["id"].as_str().expect("id should be a string");
    uuid::Uuid::parse_str(id).expect("id should be a valid UUID");
}

#[then(expr = "the customer display name should be {string}")]
async fn customer_display_name(w: &mut ApiWorld, expected: String) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    assert_eq!(json["display_name"].as_str().unwrap(), expected);
}

#[then("the customer email should be null")]
async fn customer_email_is_null(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    assert!(
        json["email"].is_null(),
        "Expected email to be null, got {:?}",
        json["email"]
    );
}

#[then(expr = "the customer email should be {string}")]
async fn customer_email_should_be(w: &mut ApiWorld, expected: String) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    assert_eq!(
        json["email"].as_str().unwrap(),
        expected,
        "Expected email={expected}, got {:?}",
        json["email"]
    );
}

#[then("the customer should have all provided fields populated")]
async fn customer_has_all_fields(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    assert_eq!(json["display_name"], "Full Details Corp");
    assert_eq!(json["company_name"], "Full Details Inc");
    assert_eq!(json["email"], "contact@fulldetails.com");
    assert_eq!(json["phone"], "555-1234");
    assert_eq!(json["address_line1"], "123 Main St");
    assert_eq!(json["address_line2"], "Suite 100");
    assert_eq!(json["city"], "Portland");
    assert_eq!(json["state"], "OR");
    assert_eq!(json["postal_code"], "97201");
    assert_eq!(json["country"], "US");
    assert_eq!(json["notes"], "VIP customer");
    assert_eq!(json["portal_enabled"], true);
    assert_eq!(json["tax_exempt"], true);
    assert_eq!(json["payment_terms"], "net_30");
    assert_eq!(json["credit_limit_cents"], 500000);
    assert_eq!(json["lead_source"], "referral");
    assert_eq!(json["tags"], "[\"vip\",\"wholesale\"]");
}

#[then(expr = "the error code should be {string}")]
async fn error_code_should_be(w: &mut ApiWorld, code: String) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    assert_eq!(json["code"].as_str().unwrap(), code);
}

#[then(expr = "the response should contain {int} items")]
async fn response_contains_n_items(w: &mut ApiWorld, count: usize) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    assert_eq!(
        items.len(),
        count,
        "Expected {count} items, got {}",
        items.len()
    );
}

#[then(expr = "the total should be {int}")]
async fn total_should_be(w: &mut ApiWorld, total: i64) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    assert_eq!(json["total"].as_i64().unwrap(), total);
}

#[then("the customer's updated_at should be later than its created_at")]
async fn updated_at_later_than_created_at(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let created = json["created_at"].as_str().unwrap();
    let updated = json["updated_at"].as_str().unwrap();
    assert!(
        updated > created,
        "Expected updated_at ({updated}) > created_at ({created})"
    );
}

#[then(expr = "the customer should have a {string} timestamp")]
async fn customer_has_timestamp(w: &mut ApiWorld, field: String) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    assert!(
        !json[&field].is_null(),
        "Expected {field} to be present, got null"
    );
    json[&field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} should be a string"));
}

#[then(expr = "the list should contain {string}")]
async fn list_contains(w: &mut ApiWorld, name: String) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    assert!(
        items
            .iter()
            .any(|i| i["display_name"].as_str() == Some(&name)),
        "Expected list to contain '{name}'"
    );
}

#[then(expr = "the list should not contain {string}")]
async fn list_not_contains(w: &mut ApiWorld, name: String) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    assert!(
        !items
            .iter()
            .any(|i| i["display_name"].as_str() == Some(&name)),
        "Expected list NOT to contain '{name}'"
    );
}

#[then(expr = "the list should contain both {string} and {string}")]
async fn list_contains_both(w: &mut ApiWorld, name1: String, name2: String) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    let names: Vec<&str> = items
        .iter()
        .filter_map(|i| i["display_name"].as_str())
        .collect();
    assert!(
        names.contains(&name1.as_str()),
        "Expected '{name1}' in list, got: {names:?}"
    );
    assert!(
        names.contains(&name2.as_str()),
        "Expected '{name2}' in list, got: {names:?}"
    );
}

#[then(expr = "{string} should have a {string} timestamp")]
async fn named_customer_has_timestamp(w: &mut ApiWorld, name: String, field: String) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let items = json["items"].as_array().expect("items should be an array");
    let customer = items
        .iter()
        .find(|i| i["display_name"].as_str() == Some(&name))
        .unwrap_or_else(|| panic!("Customer '{name}' not found in list"));
    assert!(
        !customer[&field].is_null(),
        "Expected '{name}' to have '{field}', got null"
    );
}
