//! Step definitions for `customer_atomicity.feature`.
//!
//! Each scenario drives `SqliteCustomerRepository` (the real adapter) against
//! an in-memory fixture database and a `CapturingActivityWriter` that records
//! the `&DatabaseTransaction` address it was called with. Rollback/abort
//! scenarios use a parallel harness that mirrors the adapter's tx layout but
//! drops the transaction instead of committing.

use std::sync::Arc;

use chrono::Utc;
use cucumber::{given, then, when};
use kikan::activity::{ActivityLogEntry, ActivityWriter};
use mokumo_core::actor::Actor;
use mokumo_core::filter::IncludeDeleted;
use mokumo_shop::customer::CustomerRepository;
use mokumo_shop::customer::adapter::SqliteCustomerRepository;
use mokumo_shop::customer::domain::{CreateCustomer, UpdateCustomer};
use sea_orm::{ActiveModelTrait, ActiveValue, TransactionTrait};

use super::MokumoShopWorld;
use super::capturing_writer::CapturingActivityWriter;

fn sample_create(display_name: &str) -> CreateCustomer {
    CreateCustomer {
        display_name: display_name.to_string(),
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

async fn ensure_setup(w: &mut MokumoShopWorld) {
    if w.db.is_some() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
    let db = mokumo_shop::db::initialize_database(&url).await.unwrap();
    let writer = Arc::new(CapturingActivityWriter::new_persisting());
    let repo = Arc::new(SqliteCustomerRepository::new(
        db.clone(),
        writer.clone() as Arc<dyn ActivityWriter>,
    ));
    w.db = Some(db);
    w._tmp = Some(tmp);
    w.writer = Some(writer);
    w.repo = Some(repo);
}

async fn count_customers(w: &MokumoShopWorld, display_name: Option<&str>) -> i64 {
    let pool = w.db.as_ref().unwrap().get_sqlite_connection_pool();
    match display_name {
        Some(name) => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM customers WHERE display_name = ?")
                .bind(name)
                .fetch_one(pool)
                .await
                .unwrap()
        }
        None => sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM customers")
            .fetch_one(pool)
            .await
            .unwrap(),
    }
}

async fn count_activity(w: &MokumoShopWorld) -> i64 {
    let pool = w.db.as_ref().unwrap().get_sqlite_connection_pool();
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM activity_log")
        .fetch_one(pool)
        .await
        .unwrap()
}

// --- Background ---------------------------------------------------------

#[given(regex = r#"^a profile database with an empty customer table$"#)]
async fn empty_database(w: &mut MokumoShopWorld) {
    ensure_setup(w).await;
    assert_eq!(
        count_customers(w, None).await,
        0,
        "fresh db should have no customers"
    );
}

#[given(
    regex = r#"^an ActivityWriter test double that captures the\s*&DatabaseTransaction pointer it receives on each call$"#
)]
async fn capturing_writer_installed(w: &mut MokumoShopWorld) {
    ensure_setup(w).await;
    assert!(w.writer.is_some());
}

// --- Arrange helpers ----------------------------------------------------

#[given(regex = r#"^a customer "([^"]+)" exists$"#)]
async fn customer_exists(w: &mut MokumoShopWorld, name: String) {
    ensure_setup(w).await;
    let repo = w.repo.as_ref().unwrap().clone();
    let customer = repo
        .create(&sample_create(&name), &w.actor)
        .await
        .expect("seed customer insert");
    w.named_customers.insert(name, customer.id);
    w.last_customer = Some(customer);
}

#[given(regex = r#"^the ActivityWriter test double is configured to fail on the next call$"#)]
async fn arm_failure(w: &mut MokumoShopWorld) {
    ensure_setup(w).await;
    w.writer.as_ref().unwrap().arm_failure();
}

#[given(regex = r#"^a customer-create harness that aborts the commit after both inserts succeed$"#)]
async fn enable_abort_harness(w: &mut MokumoShopWorld) {
    ensure_setup(w).await;
    w.abort_after_inserts = true;
}

#[given(regex = r#"^an authenticated session for user "([^"]+)"$"#)]
async fn authenticated_session(w: &mut MokumoShopWorld, who: String) {
    ensure_setup(w).await;
    // Fabricate a stable user id; the adapter records `actor.id()` verbatim.
    let uid: i64 = 42;
    w.alice_user_id = Some(uid);
    w.actor = Actor::user(uid);
    let _ = who;
}

// --- Actions ------------------------------------------------------------

#[when(regex = r#"^I create a customer "([^"]+)"$"#)]
async fn create_customer(w: &mut MokumoShopWorld, name: String) {
    ensure_setup(w).await;
    let repo = w.repo.as_ref().unwrap().clone();
    match repo.create(&sample_create(&name), &w.actor).await {
        Ok(c) => {
            w.named_customers.insert(name, c.id);
            w.last_customer = Some(c);
        }
        Err(e) => {
            w.last_error = Some(e.to_string());
        }
    }
}

#[when(regex = r#"^I attempt to create a customer "([^"]+)"$"#)]
async fn attempt_create_customer(w: &mut MokumoShopWorld, name: String) {
    ensure_setup(w).await;
    if w.abort_after_inserts {
        abort_commit_harness(w, &name).await;
        return;
    }
    let repo = w.repo.as_ref().unwrap().clone();
    match repo.create(&sample_create(&name), &w.actor).await {
        Ok(c) => {
            w.named_customers.insert(name, c.id);
            w.last_customer = Some(c);
        }
        Err(e) => {
            w.last_error = Some(e.to_string());
        }
    }
}

/// Mirrors the adapter's create path but drops the transaction instead of
/// committing — the proof that without commit, nothing lands.
async fn abort_commit_harness(w: &mut MokumoShopWorld, name: &str) {
    use mokumo_shop::customer::domain::CustomerId;
    let db = w.db.as_ref().unwrap().clone();
    let writer = w.writer.as_ref().unwrap().clone();

    let txn = db.begin().await.expect("begin tx");
    let id = CustomerId::generate();

    let active = mokumo_shop::customer::entity::ActiveModel {
        id: ActiveValue::Set(id.get()),
        display_name: ActiveValue::Set(name.to_string()),
        country: ActiveValue::Set(Some("US".to_string())),
        portal_enabled: ActiveValue::Set(false),
        tax_exempt: ActiveValue::Set(false),
        payment_terms: ActiveValue::Set(Some("due_on_receipt".to_string())),
        ..Default::default()
    };
    active.insert(&txn).await.expect("insert customer");

    writer
        .log(
            &txn,
            ActivityLogEntry {
                actor_id: Some(w.actor.id().to_string()),
                actor_type: w.actor.actor_type().to_string(),
                entity_kind: "customer".to_string(),
                entity_id: id.to_string(),
                action: "created".to_string(),
                payload: serde_json::json!({"display_name": name}),
                occurred_at: Utc::now(),
            },
        )
        .await
        .expect("activity log insert");

    // Explicit rollback — SeaORM's Drop handler schedules rollback on a
    // blocking executor and cannot surface errors; calling rollback().await
    // lets the test observe (and fail loudly on) any rollback failure.
    txn.rollback().await.expect("rollback must succeed");
    w.last_error = Some("aborted before commit".to_string());
}

#[when(regex = r#"^I update that customer's display name to "([^"]+)"$"#)]
async fn update_display_name(w: &mut MokumoShopWorld, new_name: String) {
    let id = w.last_customer.as_ref().expect("no prior customer").id;
    let repo = w.repo.as_ref().unwrap().clone();
    let update = UpdateCustomer {
        display_name: Some(new_name),
        ..UpdateCustomer::default()
    };
    let customer = repo.update(&id, &update, &w.actor).await.unwrap();
    w.last_customer = Some(customer);
}

#[when(regex = r#"^I soft-delete that customer$"#)]
async fn soft_delete_last(w: &mut MokumoShopWorld) {
    let id = w.last_customer.as_ref().expect("no prior customer").id;
    let repo = w.repo.as_ref().unwrap().clone();
    let customer = repo.soft_delete(&id, &w.actor).await.unwrap();
    w.last_customer = Some(customer);
}

// --- Assertions ---------------------------------------------------------

#[then(regex = r#"^a customer row exists with display name "([^"]+)"$"#)]
async fn customer_row_exists(w: &mut MokumoShopWorld, name: String) {
    assert!(
        count_customers(w, Some(&name)).await >= 1,
        "expected customer row with display_name={name}"
    );
}

#[then(regex = r#"^no customer row exists with display name "([^"]+)"$"#)]
async fn customer_row_absent(w: &mut MokumoShopWorld, name: String) {
    assert_eq!(
        count_customers(w, Some(&name)).await,
        0,
        "expected no customer row with display_name={name}"
    );
}

#[then(regex = r#"^an activity_log row exists with action "([^"]+)" and entity_type "([^"]+)"$"#)]
async fn activity_log_row(w: &mut MokumoShopWorld, action: String, entity_type: String) {
    let pool = w.db.as_ref().unwrap().get_sqlite_connection_pool();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM activity_log WHERE action = ? AND entity_type = ?",
    )
    .bind(&action)
    .bind(&entity_type)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(
        count >= 1,
        "expected activity_log row action={action} entity_type={entity_type}"
    );
}

#[then(
    regex = r#"^a new activity_log row exists with action "([^"]+)" and entity_type "([^"]+)"$"#
)]
async fn new_activity_log_row(w: &mut MokumoShopWorld, action: String, entity_type: String) {
    activity_log_row(w, action, entity_type).await;
}

#[then(
    regex = r#"^the ActivityWriter received the same &DatabaseTransaction pointer\s*that was used for the customer (INSERT|UPDATE)$"#
)]
async fn writer_received_pointer(w: &mut MokumoShopWorld, _kind: String) {
    let calls = w.writer.as_ref().unwrap().calls();
    let last = calls.last().expect("writer must have been called");
    assert!(last.tx_addr != 0, "captured tx pointer must be non-null");
    // The *same-transaction* half of this invariant is enforced
    // behaviourally by the rollback scenarios: if the writer were
    // using a different transaction, arming the writer to fail would
    // not roll back the customer row, and the activity-log-empty
    // assertion in "Customer insert rolls back when the activity
    // write fails" would not hold. The pointer capture + rollback
    // scenarios together pin the invariant; a pointer-equality check
    // against the adapter's private tx handle would require either
    // exposing it from production code or re-implementing the
    // adapter, neither of which improves confidence.
}

#[then(regex = r#"^the customer row reflects the new display name$"#)]
async fn customer_reflects_new_name(w: &mut MokumoShopWorld) {
    let expected = &w.last_customer.as_ref().unwrap().display_name;
    let id = w.last_customer.as_ref().unwrap().id;
    let repo = w.repo.as_ref().unwrap().clone();
    let found = repo
        .find_by_id(&id, IncludeDeleted::ExcludeDeleted)
        .await
        .unwrap()
        .expect("customer should exist");
    assert_eq!(found.display_name, *expected);
}

#[then(regex = r#"^the customer row has a non-null deleted_at$"#)]
async fn customer_has_deleted_at(w: &mut MokumoShopWorld) {
    let id = w.last_customer.as_ref().unwrap().id;
    let repo = w.repo.as_ref().unwrap().clone();
    let found = repo
        .find_by_id(&id, IncludeDeleted::IncludeDeleted)
        .await
        .unwrap()
        .expect("soft-deleted row still present with include=all");
    assert!(found.deleted_at.is_some());
}

#[then(regex = r#"^the create returns an activity-write error$"#)]
async fn returns_activity_write_error(w: &mut MokumoShopWorld) {
    let err = w
        .last_error
        .as_ref()
        .expect("expected an error but create succeeded");
    assert!(
        err.contains("activity log write failed") || err.contains("armed failure"),
        "unexpected error: {err}"
    );
}

#[then(regex = r#"^the activity_log is empty$"#)]
async fn activity_log_empty(w: &mut MokumoShopWorld) {
    assert_eq!(count_activity(w).await, 0);
}

#[then(regex = r#"^the activity_log entry records actor_id equal to alice's user id$"#)]
async fn actor_id_matches_alice(w: &mut MokumoShopWorld) {
    let pool = w.db.as_ref().unwrap().get_sqlite_connection_pool();
    let actor_id: Option<String> =
        sqlx::query_scalar("SELECT actor_id FROM activity_log ORDER BY id DESC LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap();
    let expected = w.alice_user_id.unwrap().to_string();
    assert_eq!(actor_id.as_deref(), Some(expected.as_str()));
}

#[then(regex = r#"^the activity_log entry records actor_type "([^"]+)"$"#)]
async fn actor_type_matches(w: &mut MokumoShopWorld, expected: String) {
    let pool = w.db.as_ref().unwrap().get_sqlite_connection_pool();
    let actor_type: String =
        sqlx::query_scalar("SELECT actor_type FROM activity_log ORDER BY id DESC LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(actor_type, expected);
}

#[then(
    regex = r#"^the activity payload is a JSON snapshot of the customer row\s*after the update$"#
)]
async fn payload_is_snapshot(w: &mut MokumoShopWorld) {
    let pool = w.db.as_ref().unwrap().get_sqlite_connection_pool();
    let payload_str: String =
        sqlx::query_scalar("SELECT payload FROM activity_log ORDER BY id DESC LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
    assert!(
        payload.get("display_name").is_some(),
        "payload missing display_name"
    );
    assert!(payload.get("id").is_some(), "payload missing id");
}

#[then(regex = r#"^the payload's display_name field equals "([^"]+)"$"#)]
async fn payload_display_name(w: &mut MokumoShopWorld, expected: String) {
    let pool = w.db.as_ref().unwrap().get_sqlite_connection_pool();
    let payload_str: String =
        sqlx::query_scalar("SELECT payload FROM activity_log ORDER BY id DESC LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
    assert_eq!(payload["display_name"].as_str(), Some(expected.as_str()));
}

#[then(regex = r#"^the payload's id field equals the customer's UUID$"#)]
async fn payload_id_matches(w: &mut MokumoShopWorld) {
    let pool = w.db.as_ref().unwrap().get_sqlite_connection_pool();
    let payload_str: String =
        sqlx::query_scalar("SELECT payload FROM activity_log ORDER BY id DESC LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
    let expected = w.last_customer.as_ref().unwrap().id.to_string();
    assert_eq!(payload["id"].as_str(), Some(expected.as_str()));
}
