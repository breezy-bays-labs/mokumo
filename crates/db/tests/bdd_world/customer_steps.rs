use cucumber::{given, then, when};
use mokumo_core::activity::traits::ActivityLogRepository;
use mokumo_core::customer::traits::CustomerRepository;
use mokumo_core::customer::{CreateCustomer, CustomerId, UpdateCustomer};
use mokumo_core::pagination::PageParams;
use mokumo_db::activity::repo::SqliteActivityLogRepo;
use mokumo_db::customer::repo::SqliteCustomerRepo;

use super::DbWorld;

fn customer_repo(w: &DbWorld) -> SqliteCustomerRepo {
    SqliteCustomerRepo::new(w.pool.clone())
}

fn activity_repo(w: &DbWorld) -> SqliteActivityLogRepo {
    SqliteActivityLogRepo::new(w.pool.clone())
}

fn make_create_request(name: &str) -> CreateCustomer {
    CreateCustomer {
        display_name: name.to_string(),
        company_name: None,
        email: None,
        phone: None,
        address_line1: None,
        address_line2: None,
        city: None,
        state: None,
        postal_code: None,
        country: None,
        notes: None,
        portal_enabled: None,
        tax_exempt: None,
        payment_terms: None,
        credit_limit_cents: None,
        lead_source: None,
        tags: None,
    }
}

fn make_update_request(display_name: Option<&str>) -> UpdateCustomer {
    UpdateCustomer {
        display_name: display_name.map(String::from),
        company_name: None,
        email: None,
        phone: None,
        address_line1: None,
        address_line2: None,
        city: None,
        state: None,
        postal_code: None,
        country: None,
        notes: None,
        portal_enabled: None,
        tax_exempt: None,
        payment_terms: None,
        credit_limit_cents: None,
        lead_source: None,
        tags: None,
    }
}

// ---- Given steps ----

#[given("an empty database")]
async fn empty_database(_w: &mut DbWorld) {
    // Database is already empty from DbWorld::new() initialization
}

#[given(expr = "a customer {string} exists in the database")]
async fn customer_exists(w: &mut DbWorld, name: String) {
    let repo = customer_repo(w);
    let req = make_create_request(&name);
    let customer = repo.create(&req).await.expect("failed to create customer");
    w.last_customer = Some(customer);
}

#[given(expr = "that customer has {int} activity entry")]
async fn customer_has_n_activity_entries(w: &mut DbWorld, count: i32) {
    let customer = w.last_customer.as_ref().expect("no customer created yet");
    let repo = activity_repo(w);
    let params = PageParams::new(Some(1), Some(100));
    let (_, total) = repo
        .list(Some("customer"), Some(&customer.id.to_string()), params)
        .await
        .expect("failed to list activity entries");
    assert_eq!(
        total, count as i64,
        "Expected {} activity entries, found {}",
        count, total
    );
}

#[given(expr = "a customer {string} exists with {int} activity entry")]
async fn customer_exists_with_entries(w: &mut DbWorld, name: String, count: i32) {
    customer_exists(w, name).await;
    customer_has_n_activity_entries(w, count).await;
}

// ---- When steps ----

#[when(expr = "a customer {string} is created")]
async fn create_customer(w: &mut DbWorld, name: String) {
    let repo = customer_repo(w);
    let req = make_create_request(&name);
    match repo.create(&req).await {
        Ok(customer) => w.last_customer = Some(customer),
        Err(e) => w.last_error = Some(e),
    }
}

#[when(expr = "that customer's display name is changed to {string}")]
async fn update_customer_name(w: &mut DbWorld, new_name: String) {
    let customer = w.last_customer.as_ref().expect("no customer to update");
    let id = customer.id;
    let repo = customer_repo(w);
    let req = make_update_request(Some(&new_name));
    match repo.update(&id, &req).await {
        Ok(customer) => w.last_customer = Some(customer),
        Err(e) => w.last_error = Some(e),
    }
}

#[when("that customer is soft-deleted")]
async fn soft_delete_customer(w: &mut DbWorld) {
    let customer = w
        .last_customer
        .as_ref()
        .expect("no customer to soft-delete");
    let id = customer.id;
    let repo = customer_repo(w);
    match repo.soft_delete(&id).await {
        Ok(customer) => w.last_customer = Some(customer),
        Err(e) => w.last_error = Some(e),
    }
}

#[when("an update to a non-existent customer is attempted")]
async fn update_nonexistent(w: &mut DbWorld) {
    let repo = customer_repo(w);
    let fake_id = CustomerId::generate();
    let req = make_update_request(Some("Should Not Exist"));
    match repo.update(&fake_id, &req).await {
        Ok(customer) => w.last_customer = Some(customer),
        Err(e) => w.last_error = Some(e),
    }
}

#[when("a customer is created, then updated, then soft-deleted")]
async fn create_update_delete(w: &mut DbWorld) {
    let repo = customer_repo(w);

    // Create
    let req = make_create_request("Lifecycle Corp");
    let customer = repo.create(&req).await.expect("failed to create");
    let id = customer.id;
    w.last_customer = Some(customer);

    // Update
    let update_req = make_update_request(Some("Lifecycle Corp Updated"));
    let customer = repo
        .update(&id, &update_req)
        .await
        .expect("failed to update");
    w.last_customer = Some(customer);

    // Soft-delete
    let customer = repo.soft_delete(&id).await.expect("failed to soft-delete");
    w.last_customer = Some(customer);
}

#[when(expr = "the activity log is queried for entity type {string}")]
async fn query_activity_by_type(w: &mut DbWorld, entity_type: String) {
    let repo = activity_repo(w);
    let params = PageParams::new(Some(1), Some(100));
    let (entries, total) = repo
        .list(Some(&entity_type), None, params)
        .await
        .expect("failed to query activity log");
    w.activity_query_result = Some((entries, total));
}

// ---- Then steps ----

#[then(expr = "the customer {string} should exist")]
async fn customer_should_exist(w: &mut DbWorld, name: String) {
    let customer = w.last_customer.as_ref().expect("no customer result");
    assert_eq!(customer.display_name, name);
}

#[then(regex = r#"^the activity log should contain an? "([^"]*)" entry for that customer$"#)]
async fn activity_log_has_entry(w: &mut DbWorld, action: String) {
    let customer = w.last_customer.as_ref().expect("no customer");
    let repo = activity_repo(w);
    let params = PageParams::new(Some(1), Some(100));
    let (entries, _) = repo
        .list(Some("customer"), Some(&customer.id.to_string()), params)
        .await
        .expect("failed to list activity entries");

    let found = entries.iter().any(|e| e.action == action);
    assert!(
        found,
        "Expected '{}' entry in activity log, found: {:?}",
        action,
        entries.iter().map(|e| &e.action).collect::<Vec<_>>()
    );
}

#[then("the activity entry should record the customer's details at creation")]
async fn activity_has_customer_details(w: &mut DbWorld) {
    let customer = w.last_customer.as_ref().expect("no customer");
    let repo = activity_repo(w);
    let params = PageParams::new(Some(1), Some(100));
    let (entries, _) = repo
        .list(Some("customer"), Some(&customer.id.to_string()), params)
        .await
        .expect("failed to list activity entries");

    let entry = entries
        .iter()
        .find(|e| e.action == "created")
        .expect("no 'created' entry found");

    let payload = &entry.payload;
    assert_eq!(
        payload.get("display_name").and_then(|v| v.as_str()),
        Some(customer.display_name.as_str()),
        "Activity payload should contain the customer's display_name"
    );
}

#[then(expr = "the customer's display name should be {string}")]
async fn customer_display_name_is(w: &mut DbWorld, expected: String) {
    let customer = w.last_customer.as_ref().expect("no customer");
    assert_eq!(customer.display_name, expected);
}

#[then("the customer should be marked as deleted")]
async fn customer_is_deleted(w: &mut DbWorld) {
    let customer = w.last_customer.as_ref().expect("no customer");
    assert!(
        customer.deleted_at.is_some(),
        "Expected customer to have deleted_at set"
    );
}

#[then(expr = "the activity log for {string} should still have {int} entry")]
async fn activity_log_count_for_customer(w: &mut DbWorld, _name: String, count: i32) {
    let customer = w.last_customer.as_ref().expect("no customer");
    let repo = activity_repo(w);
    let params = PageParams::new(Some(1), Some(100));
    let (_, total) = repo
        .list(Some("customer"), Some(&customer.id.to_string()), params)
        .await
        .expect("failed to list activity entries");
    assert_eq!(
        total, count as i64,
        "Expected {} activity entries, found {}",
        count, total
    );
}

#[then("no new activity entries should exist")]
async fn no_new_activity_entries(w: &mut DbWorld) {
    let repo = activity_repo(w);
    let params = PageParams::new(Some(1), Some(100));
    let (_, total) = repo
        .list(None, None, params)
        .await
        .expect("failed to list activity entries");
    // There should be exactly 1 entry (from creating "Acme Corp" in the Given step)
    assert_eq!(
        total, 1,
        "Expected only 1 total activity entry, found {}",
        total
    );
}

#[then(expr = "the activity log should contain {int} entries for that customer")]
async fn activity_log_has_n_entries(w: &mut DbWorld, count: i32) {
    let customer = w.last_customer.as_ref().expect("no customer");
    let repo = activity_repo(w);
    let params = PageParams::new(Some(1), Some(100));
    let (_, total) = repo
        .list(Some("customer"), Some(&customer.id.to_string()), params)
        .await
        .expect("failed to list activity entries");
    assert_eq!(
        total, count as i64,
        "Expected {} activity entries, found {}",
        count, total
    );
}

#[then(expr = "the actions should be {string}, {string}, {string} in order")]
async fn actions_in_order(w: &mut DbWorld, first: String, second: String, third: String) {
    let customer = w.last_customer.as_ref().expect("no customer");
    let repo = activity_repo(w);
    let params = PageParams::new(Some(1), Some(100));
    let (entries, _) = repo
        .list(Some("customer"), Some(&customer.id.to_string()), params)
        .await
        .expect("failed to list activity entries");

    // list() returns entries in DESC order (newest first) — reverse for chronological
    let mut actions: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
    actions.reverse();

    assert_eq!(
        actions,
        vec![first.as_str(), second.as_str(), third.as_str()],
        "Expected actions [{}, {}, {}] in order, got {:?}",
        first,
        second,
        third,
        actions
    );
}

#[then(expr = "the response should contain {int} entry for {string}")]
async fn response_contains_entries(w: &mut DbWorld, count: i32, _name: String) {
    let (_, total) = w
        .activity_query_result
        .as_ref()
        .expect("no activity query result — run a When step first");
    assert_eq!(
        *total, count as i64,
        "Expected {} entries, found {}",
        count, total
    );
}
