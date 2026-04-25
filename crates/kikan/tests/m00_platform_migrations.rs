//! Schema-shape proofs for the four M00 platform migrations.
//!
//! Each test applies `run_platform_migrations` against a fresh in-memory
//! SQLite connection and asserts the expected structural invariants of
//! the introduced tables, triggers, and indexes via `PRAGMA` +
//! `sqlite_master` queries. The assertions cover the pieces that
//! downstream code relies on: column names, FK ordering, the CHECK
//! constraints, and the two trigger names that the admin UI surfaces
//! when users hit them.
//!
//! Kept as a single file because all four migrations share a setup
//! (`make_db()`) and we want one failure report rather than four.

use sea_orm::ConnectionTrait;
use sea_orm::DatabaseConnection;
use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

async fn make_db() -> (DatabaseConnection, SqlitePool) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite must connect");
    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone());
    kikan::migrations::platform::run_platform_migrations(&db)
        .await
        .expect("platform migrations must succeed");
    (db, pool)
}

async fn column_names(pool: &SqlitePool, table: &str) -> Vec<String> {
    sqlx::query(&format!("PRAGMA table_info({table})"))
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect()
}

async fn table_exists(pool: &SqlitePool, table: &str) -> bool {
    let rows = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name = ?")
        .bind(table)
        .fetch_all(pool)
        .await
        .unwrap();
    !rows.is_empty()
}

async fn trigger_exists(pool: &SqlitePool, name: &str) -> bool {
    let rows = sqlx::query("SELECT name FROM sqlite_master WHERE type='trigger' AND name = ?")
        .bind(name)
        .fetch_all(pool)
        .await
        .unwrap();
    !rows.is_empty()
}

async fn index_exists(pool: &SqlitePool, name: &str) -> bool {
    let rows = sqlx::query("SELECT name FROM sqlite_master WHERE type='index' AND name = ?")
        .bind(name)
        .fetch_all(pool)
        .await
        .unwrap();
    !rows.is_empty()
}

#[tokio::test]
async fn profile_user_roles_table_has_expected_shape() {
    let (_db, pool) = make_db().await;

    assert!(
        table_exists(&pool, "profile_user_roles").await,
        "profile_user_roles table must exist after migrations"
    );

    let cols = column_names(&pool, "profile_user_roles").await;
    assert_eq!(
        cols,
        vec!["profile_id", "user_id", "role", "granted_at"],
        "profile_user_roles column set changed unexpectedly"
    );

    assert!(
        index_exists(&pool, "idx_profile_user_roles_user_id").await,
        "secondary index on user_id must exist"
    );
}

#[tokio::test]
async fn profile_user_roles_rejects_invalid_role_values() {
    let (_db, pool) = make_db().await;

    // Seed FK targets: a user (`users.id`) plus the two profiles
    // referenced by the inserts below (`profiles.slug`).
    sqlx::query(
        "INSERT INTO users (id, email, name, password_hash)
         VALUES (1, 'admin@example.com', 'Admin', 'hash')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO profiles (slug, display_name, kind) VALUES ('demo', 'Demo', 'demo'), ('demo2', 'Demo 2', 'demo')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Accepted value.
    sqlx::query(
        "INSERT INTO profile_user_roles (profile_id, user_id, role) VALUES ('demo', 1, 'Admin')",
    )
    .execute(&pool)
    .await
    .expect("'Admin' is a valid role value");

    // Rejected value — CHECK constraint fires. Use a distinct (profile_id,
    // user_id) pair so a PK-conflict failure can't masquerade as a CHECK
    // failure (the previous insert was ('demo', 1, 'Admin')).
    let bad = sqlx::query(
        "INSERT INTO profile_user_roles (profile_id, user_id, role) VALUES ('demo2', 1, 'Owner')",
    )
    .execute(&pool)
    .await;
    assert!(
        bad.is_err(),
        "CHECK constraint must reject role values outside ('Admin', 'User')"
    );
}

#[tokio::test]
async fn last_admin_deactivation_guard_refuses_last_active_admin() {
    let (db, pool) = make_db().await;

    // Two Admins, both active. Deactivating either is allowed.
    db.execute_unprepared(
        "INSERT INTO users (id, email, name, password_hash, role_id, is_active)
         VALUES (1, 'alice@example.com', 'Alice', 'hash', 1, 1),
                (2, 'bob@example.com',   'Bob',   'hash', 1, 1)",
    )
    .await
    .unwrap();

    db.execute_unprepared("UPDATE users SET is_active = 0 WHERE id = 2")
        .await
        .expect("deactivating a non-last Admin succeeds");

    // Now only one Admin is active. Deactivating the remaining one must abort.
    let rejected = db
        .execute_unprepared("UPDATE users SET is_active = 0 WHERE id = 1")
        .await;
    assert!(
        rejected.is_err(),
        "deactivating the last active Admin must be refused by the trigger"
    );

    // The row must still be active.
    let row = sqlx::query("SELECT is_active FROM users WHERE id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let is_active: i64 = row.get("is_active");
    assert_eq!(is_active, 1, "refused UPDATE must not have partial effect");
}

#[tokio::test]
async fn last_admin_demote_guard_refuses_role_change_on_last_admin() {
    let (db, _pool) = make_db().await;

    db.execute_unprepared(
        "INSERT INTO users (id, email, name, password_hash, role_id, is_active)
         VALUES (1, 'alice@example.com', 'Alice', 'hash', 1, 1)",
    )
    .await
    .unwrap();

    let rejected = db
        .execute_unprepared("UPDATE users SET role_id = 2 WHERE id = 1")
        .await;
    assert!(
        rejected.is_err(),
        "demoting the last active Admin must be refused by the trigger"
    );
}

#[tokio::test]
async fn active_integrations_table_has_expected_shape() {
    let (_db, pool) = make_db().await;

    assert!(table_exists(&pool, "active_integrations").await);

    let cols = column_names(&pool, "active_integrations").await;
    assert_eq!(
        cols,
        vec![
            "integration_id",
            "enabled_at",
            "credentials_ciphertext",
            "credentials_nonce",
            "last_sync_at",
            "schema_version",
            "created_at",
            "updated_at",
        ],
        "active_integrations column set changed unexpectedly"
    );

    assert!(
        trigger_exists(&pool, "active_integrations_updated_at").await,
        "updated_at trigger must exist on active_integrations"
    );
}

#[tokio::test]
async fn active_integrations_updated_at_trigger_touches_updated_at() {
    let (db, pool) = make_db().await;

    db.execute_unprepared(
        "INSERT INTO active_integrations (integration_id, enabled_at, schema_version)
         VALUES ('stripe', '2026-04-24T00:00:00Z', 1)",
    )
    .await
    .unwrap();

    let before =
        sqlx::query("SELECT updated_at FROM active_integrations WHERE integration_id='stripe'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get::<String, _>("updated_at");

    // Migration uses millisecond-precision strftime ('%f'), so a few ms
    // is enough for the timestamp to advance.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    db.execute_unprepared(
        "UPDATE active_integrations SET last_sync_at = '2026-04-24T00:00:01Z' WHERE integration_id='stripe'",
    )
    .await
    .unwrap();

    let after =
        sqlx::query("SELECT updated_at FROM active_integrations WHERE integration_id='stripe'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get::<String, _>("updated_at");

    assert_ne!(before, after, "trigger must update the updated_at column");
}

#[tokio::test]
async fn active_integrations_credential_columns_must_be_symmetric() {
    let (db, _pool) = make_db().await;

    // Both NULL — zeroed credentials, accepted.
    db.execute_unprepared(
        "INSERT INTO active_integrations (integration_id, schema_version)
         VALUES ('zeroed', 1)",
    )
    .await
    .expect("both credential columns NULL is a valid state");

    // Both NOT NULL — sealed envelope, accepted.
    db.execute_unprepared(
        "INSERT INTO active_integrations
            (integration_id, credentials_ciphertext, credentials_nonce, schema_version)
         VALUES ('sealed', x'01', x'02', 1)",
    )
    .await
    .expect("both credential columns NOT NULL is a valid state");

    // ciphertext present, nonce NULL — rejected.
    let ciphertext_only = db
        .execute_unprepared(
            "INSERT INTO active_integrations
                (integration_id, credentials_ciphertext, schema_version)
             VALUES ('half-ciphertext', x'01', 1)",
        )
        .await;
    assert!(
        ciphertext_only.is_err(),
        "ciphertext without nonce must be rejected by the symmetric-nullability CHECK"
    );

    // nonce present, ciphertext NULL — rejected.
    let nonce_only = db
        .execute_unprepared(
            "INSERT INTO active_integrations
                (integration_id, credentials_nonce, schema_version)
             VALUES ('half-nonce', x'02', 1)",
        )
        .await;
    assert!(
        nonce_only.is_err(),
        "nonce without ciphertext must be rejected by the symmetric-nullability CHECK"
    );
}

#[tokio::test]
async fn integration_event_log_table_has_expected_shape() {
    let (_db, pool) = make_db().await;

    assert!(table_exists(&pool, "integration_event_log").await);

    let cols = column_names(&pool, "integration_event_log").await;
    assert_eq!(
        cols,
        vec![
            "id",
            "integration_id",
            "at",
            "event_type",
            "status",
            "error",
            "payload_redacted",
        ],
    );

    assert!(
        index_exists(&pool, "idx_integration_event_log_integration_at").await,
        "event log index must exist for integration_id + at lookup"
    );
}

#[tokio::test]
async fn integration_event_log_enforces_status_error_invariant() {
    let (db, _pool) = make_db().await;

    db.execute_unprepared(
        "INSERT INTO active_integrations (integration_id, schema_version)
         VALUES ('stripe', 1)",
    )
    .await
    .unwrap();

    // ok + error NULL is accepted.
    db.execute_unprepared(
        "INSERT INTO integration_event_log (integration_id, event_type, status)
         VALUES ('stripe', 'checkout.session.completed', 'ok')",
    )
    .await
    .expect("ok + error IS NULL satisfies the invariant");

    // error + error NOT NULL is accepted.
    db.execute_unprepared(
        "INSERT INTO integration_event_log (integration_id, event_type, status, error)
         VALUES ('stripe', 'catalog.refresh', 'error', 'upstream 500')",
    )
    .await
    .expect("error + error NOT NULL satisfies the invariant");

    // ok + error NOT NULL is rejected.
    let mixed_ok = db
        .execute_unprepared(
            "INSERT INTO integration_event_log (integration_id, event_type, status, error)
             VALUES ('stripe', 'catalog.refresh', 'ok', 'should not be here')",
        )
        .await;
    assert!(
        mixed_ok.is_err(),
        "ok status with a non-null error column must be rejected"
    );

    // error + error NULL is rejected.
    let missing_error = db
        .execute_unprepared(
            "INSERT INTO integration_event_log (integration_id, event_type, status)
             VALUES ('stripe', 'catalog.refresh', 'error')",
        )
        .await;
    assert!(
        missing_error.is_err(),
        "error status without a message must be rejected"
    );
}

#[tokio::test]
async fn integration_event_log_fk_requires_integration_row() {
    let (_db, pool) = make_db().await;

    // Without an active_integrations row, the FK must refuse the insert.
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();

    let orphan = sqlx::query(
        "INSERT INTO integration_event_log (integration_id, event_type, status)
         VALUES ('unknown-integration', 'sync.completed', 'ok')",
    )
    .execute(&pool)
    .await;
    assert!(
        orphan.is_err(),
        "FK constraint must refuse events for unknown integrations"
    );
}
