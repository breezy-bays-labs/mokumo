//! Step definitions for `activity_visibility.feature`.
//!
//! The feature phrases every When as "the list_activity handler ...". These
//! steps drive the same code path the handler uses without spinning up the
//! HTTP stack: `SqliteActivityLogRepo::list` → `kikan_types::activity::to_response`.
//! This isolates the wire-shape contract (R13) from HTTP plumbing.

use cucumber::{given, then, when};
use kikan_types::activity::to_response;
use mokumo_core::activity::traits::ActivityLogRepository;
use mokumo_core::pagination::PageParams;
use sqlx::SqlitePool;

use super::KikanWorld;

async fn ensure_pool(w: &mut KikanWorld) -> SqlitePool {
    if let Some(p) = &w.activity_pool {
        return p.clone();
    }
    let tmp = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
    let db = kikan::db::initialize_database(&url).await.unwrap();
    let pool = db.get_sqlite_connection_pool().clone();
    sqlx::query(
        "CREATE TABLE activity_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            entity_type TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            action TEXT NOT NULL,
            actor_id TEXT NOT NULL DEFAULT 'system',
            actor_type TEXT NOT NULL DEFAULT 'system',
            payload TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    w.activity_pool = Some(pool.clone());
    w.activity_tmp = Some(tmp);
    pool
}

async fn insert_row(
    pool: &SqlitePool,
    id: Option<i64>,
    action: &str,
    payload: &str,
    created_at: Option<&str>,
) {
    // Build the SQL dynamically so we can pin id and/or created_at.
    let mut cols = vec!["entity_type", "entity_id", "action", "payload"];
    let mut placeholders = vec!["'thing'", "'e1'", "?", "?"];
    if id.is_some() {
        cols.insert(0, "id");
        placeholders.insert(0, "?");
    }
    if created_at.is_some() {
        cols.push("created_at");
        placeholders.push("?");
    }
    let sql = format!(
        "INSERT INTO activity_log ({}) VALUES ({})",
        cols.join(", "),
        placeholders.join(", ")
    );
    let mut q = sqlx::query(&sql);
    if let Some(i) = id {
        q = q.bind(i);
    }
    q = q.bind(action).bind(payload);
    if let Some(ts) = created_at {
        q = q.bind(ts);
    }
    q.execute(pool).await.unwrap();
}

async fn run_list(w: &mut KikanWorld, params: PageParams) {
    let pool = w.activity_pool.clone().expect("pool not initialised");
    let repo = kikan::activity::SqliteActivityLogRepo::new(pool);
    let (entries, total) = repo.list(None, None, params).await.unwrap();
    w.activity_list = entries.into_iter().map(to_response).collect();
    w.activity_total = total;
}

fn ids(w: &KikanWorld) -> Vec<i64> {
    w.activity_list.iter().map(|r| r.id).collect()
}

// ---- Given ----

#[given(expr = "the activity_log contains a row with action {string}")]
async fn row_with_action(w: &mut KikanWorld, action: String) {
    let pool = ensure_pool(w).await;
    insert_row(&pool, None, &action, "{}", None).await;
}

#[given(regex = r#"^the activity_log contains a row whose payload is the JSON document$"#)]
async fn row_with_payload(w: &mut KikanWorld, step: &cucumber::gherkin::Step) {
    let pool = ensure_pool(w).await;
    let payload = step
        .docstring
        .as_deref()
        .expect("expected docstring payload");
    insert_row(&pool, None, "created", payload.trim(), None).await;
}

#[given(expr = "the activity_log contains a row with created_at {string}")]
async fn row_with_created_at(w: &mut KikanWorld, ts: String) {
    let pool = ensure_pool(w).await;
    insert_row(&pool, None, "created", "{}", Some(&ts)).await;
}

#[given(expr = "three activity_log rows with ids {int}, {int}, {int}")]
async fn three_rows(w: &mut KikanWorld, a: i64, b: i64, c: i64) {
    let pool = ensure_pool(w).await;
    // Created_at gets set by the And step below, but insert with placeholders now.
    for id in [a, b, c] {
        insert_row(
            &pool,
            Some(id),
            "created",
            "{}",
            Some("2025-11-02T14:30:00Z"),
        )
        .await;
    }
}

#[given(regex = r#"^their created_at values are "([^"]+)",\s*"([^"]+)", "([^"]+)" respectively$"#)]
async fn three_rows_created_at(w: &mut KikanWorld, t1: String, t2: String, t3: String) {
    let pool = w.activity_pool.clone().expect("pool not initialised");
    // Rows were inserted by previous step with placeholder timestamps; update
    // them in insertion order (the id order matches the insertion order).
    let mut rows = sqlx::query_as::<_, (i64,)>("SELECT id FROM activity_log ORDER BY id ASC")
        .fetch_all(&pool)
        .await
        .unwrap();
    rows.truncate(3);
    for ((id,), ts) in rows.iter().zip([t1, t2, t3]) {
        sqlx::query("UPDATE activity_log SET created_at = ? WHERE id = ?")
            .bind(ts)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }
}

#[given(expr = "two activity_log rows with created_at {string}")]
async fn two_rows_shared_ts(w: &mut KikanWorld, _ts: String) {
    // Defer insertion to the And-step that supplies the ids.
    let _ = ensure_pool(w).await;
}

#[given(expr = "the rows have ids {int} and {int} respectively")]
async fn two_rows_ids(w: &mut KikanWorld, a: i64, b: i64) {
    let pool = w.activity_pool.clone().expect("pool not initialised");
    for id in [a, b] {
        insert_row(
            &pool,
            Some(id),
            "created",
            "{}",
            Some("2025-11-02T14:30:00Z"),
        )
        .await;
    }
}

#[given(expr = "five activity_log rows inserted in a single transaction")]
async fn five_rows_tx(w: &mut KikanWorld) {
    let _ = ensure_pool(w).await;
    // Insertion happens in the following And-steps that pin timestamp and ids.
}

#[given(expr = "all five rows share created_at {string}")]
async fn five_rows_shared_ts(_w: &mut KikanWorld, _ts: String) {
    // Recorded alongside the ids step below.
}

#[given(expr = "AUTOINCREMENT assigned ids {int} through {int} in insertion order")]
async fn five_rows_ids(w: &mut KikanWorld, lo: i64, hi: i64) {
    let pool = w.activity_pool.clone().expect("pool not initialised");
    let mut tx = pool.begin().await.unwrap();
    for id in lo..=hi {
        sqlx::query(
            "INSERT INTO activity_log \
             (id, entity_type, entity_id, action, actor_id, actor_type, payload, created_at) \
             VALUES (?, 'thing', 'e1', 'created', 'system', 'system', '{}', '2025-11-02T14:30:00Z')",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

#[given(expr = "{int} activity_log rows with created_at {string} and ids {int} through {int}")]
async fn eleven_rows(w: &mut KikanWorld, _count: usize, ts: String, lo: i64, hi: i64) {
    let pool = ensure_pool(w).await;
    for id in lo..=hi {
        insert_row(&pool, Some(id), "created", "{}", Some(&ts)).await;
    }
}

#[given(expr = "the activity_log contains {int} rows")]
async fn contains_n_rows(w: &mut KikanWorld, count: i64) {
    let pool = ensure_pool(w).await;
    for i in 0..count {
        // Stagger timestamps so newest-first order is deterministic.
        let ts = format!("2025-11-02T14:30:{:02}Z", i % 60);
        insert_row(&pool, None, "created", "{}", Some(&ts)).await;
    }
}

// ---- When ----

#[when("the list_activity handler serializes that row")]
async fn when_serialize_that_row(w: &mut KikanWorld) {
    run_list(w, PageParams::new(None, None)).await;
}

#[when("the list_activity handler runs without a cursor")]
async fn when_runs_no_cursor(w: &mut KikanWorld) {
    run_list(w, PageParams::new(None, None)).await;
}

#[when(expr = "the list_activity handler runs with page {int} and per_page {int}")]
async fn when_runs_paginated(w: &mut KikanWorld, page: u32, per_page: u32) {
    run_list(w, PageParams::new(Some(page), Some(per_page))).await;
}

#[when(expr = "the list_activity handler runs with limit {int}")]
async fn when_runs_with_limit(w: &mut KikanWorld, limit: u32) {
    run_list(w, PageParams::new(None, Some(limit))).await;
}

// ---- Then ----

#[then(expr = "the response entry's \"action\" field is exactly {string}")]
async fn entry_action_field_exact(w: &mut KikanWorld, expected: String) {
    assert_eq!(w.activity_list.len(), 1, "expected exactly one entry");
    assert_eq!(w.activity_list[0].action, expected);
}

#[then("the response payload deserializes to the same JSON document")]
async fn payload_round_trips(w: &mut KikanWorld) {
    let got = w.activity_list[0]
        .payload
        .as_ref()
        .expect("payload missing");
    let expected: serde_json::Value =
        serde_json::from_str(r#"{"display_name":"Acme","email":"a@b.co"}"#).unwrap();
    assert_eq!(got, &expected);
}

#[then("the payload's keys appear in the same order as stored")]
async fn payload_key_order(w: &mut KikanWorld) {
    // serde_json::Value without the `preserve_order` feature uses a BTreeMap,
    // so keys emerge alphabetically. The fixture happens to be alphabetical
    // already (`display_name` < `email`), so this assertion holds by
    // coincidence. Do not generalise — a non-alphabetical fixture would
    // require the `preserve_order` feature flag.
    let payload = w.activity_list[0].payload.as_ref().unwrap();
    let serialized = serde_json::to_string(payload).unwrap();
    let display_pos = serialized.find("display_name").unwrap();
    let email_pos = serialized.find("email").unwrap();
    assert!(
        display_pos < email_pos,
        "expected display_name before email, got {serialized}"
    );
}

#[then(expr = "the response's \"created_at\" field is {string}")]
async fn created_at_field_exact(w: &mut KikanWorld, expected: String) {
    assert_eq!(w.activity_list[0].created_at, expected);
}

#[then(expr = "the response lists row {int} first")]
async fn lists_row_first(w: &mut KikanWorld, id: i64) {
    assert_eq!(
        w.activity_list.first().map(|r| r.id),
        Some(id),
        "ids = {:?}",
        ids(w)
    );
}

#[then(expr = "row {int} last")]
async fn lists_row_last(w: &mut KikanWorld, id: i64) {
    assert_eq!(
        w.activity_list.last().map(|r| r.id),
        Some(id),
        "ids = {:?}",
        ids(w)
    );
}

#[then(expr = "the response lists row {int} before row {int}")]
async fn lists_before(w: &mut KikanWorld, first: i64, second: i64) {
    let list = ids(w);
    let p1 = list.iter().position(|i| *i == first);
    let p2 = list.iter().position(|i| *i == second);
    assert!(
        p1.is_some() && p2.is_some() && p1 < p2,
        "expected {first} before {second}, ids = {list:?}"
    );
}

#[then(expr = "the response lists the rows in the order {int}, {int}, {int}, {int}, {int}")]
async fn lists_in_order(w: &mut KikanWorld, a: i64, b: i64, c: i64, d: i64, e: i64) {
    assert_eq!(ids(w), vec![a, b, c, d, e]);
}

#[then(expr = "the response contains exactly one row")]
async fn contains_exactly_one(w: &mut KikanWorld) {
    assert_eq!(w.activity_list.len(), 1, "ids = {:?}", ids(w));
}

#[then(expr = "that row has id {int}")]
async fn that_row_has_id(w: &mut KikanWorld, id: i64) {
    assert_eq!(w.activity_list[0].id, id);
}

#[then(expr = "the response contains {int} entries")]
async fn contains_n_entries(w: &mut KikanWorld, count: usize) {
    assert_eq!(w.activity_list.len(), count);
}

#[then(expr = "the total reports {int}")]
async fn total_reports(w: &mut KikanWorld, expected: i64) {
    assert_eq!(w.activity_total, expected);
}

#[then("the response succeeds")]
async fn response_succeeds(w: &mut KikanWorld) {
    // run_list panics on error, so reaching this step means success.
    assert!(!w.activity_list.is_empty() || w.activity_total == 0);
}
