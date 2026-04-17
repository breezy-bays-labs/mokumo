//! Entity-schema drift detection — verifies SeaORM entity files match the
//! actual database schema produced by migrations.
//!
//! For each table that has a committed SeaORM entity, this test compares the
//! entity's declared columns against the schema from PRAGMA table_info.
//! Drift in either direction (schema column missing from entity, or entity
//! column missing from schema) fails the test.
//!
//! See: issue #68, ADR `adr-seaorm-testing-standards.md` Decision 2

use mokumo_shop::db::initialize_database;
use sea_orm::{Iterable, entity::prelude::*};
use sqlx::Row;
use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn migrated_pool() -> (sqlx::SqlitePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("drift-check.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = initialize_database(&url).await.unwrap();
    let pool = db.get_sqlite_connection_pool().clone();
    (pool, dir)
}

/// Get column names from the actual database schema via PRAGMA.
async fn schema_columns(pool: &sqlx::SqlitePool, table: &str) -> BTreeSet<String> {
    let sql = format!("PRAGMA table_info('{}')", table);
    sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect()
}

/// Get column names declared by a SeaORM entity.
fn entity_columns<E: EntityTrait>() -> BTreeSet<String>
where
    E::Column: Iterable,
{
    E::Column::iter()
        .map(|col| col.as_str().to_owned())
        .collect()
}

/// Assert that entity columns and schema columns match exactly.
///
/// Reports clearly which columns are in the schema but missing from the entity,
/// and which are in the entity but missing from the schema.
fn assert_columns_match(table: &str, schema: &BTreeSet<String>, entity: &BTreeSet<String>) {
    let in_schema_not_entity: BTreeSet<_> = schema.difference(entity).collect();
    let in_entity_not_schema: BTreeSet<_> = entity.difference(schema).collect();

    if !in_schema_not_entity.is_empty() || !in_entity_not_schema.is_empty() {
        let mut msg = format!("Entity-schema drift detected for table '{}':\n", table);
        if !in_schema_not_entity.is_empty() {
            msg.push_str(&format!(
                "  Columns in schema but NOT in entity: {:?}\n",
                in_schema_not_entity
            ));
            msg.push_str("  → Add these columns to the entity, or update the migration.\n");
        }
        if !in_entity_not_schema.is_empty() {
            msg.push_str(&format!(
                "  Columns in entity but NOT in schema: {:?}\n",
                in_entity_not_schema
            ));
            msg.push_str("  → Remove these columns from the entity, or add a migration.\n");
        }
        panic!("{}", msg);
    }
}

// ---------------------------------------------------------------------------
// Drift checks — one test per entity
// ---------------------------------------------------------------------------

#[tokio::test]
async fn customer_entity_columns_match_schema() {
    let (pool, _dir) = migrated_pool().await;

    let schema = schema_columns(&pool, "customers").await;
    // Stage 3 V6c: `customers` entity now lives in the `mokumo-shop` vertical.
    // Schema is still owned by the `mokumo-db` migrator (migration continuity),
    // so the drift check runs cross-crate: migrator's tables vs vertical's entity.
    let entity = entity_columns::<mokumo_shop::customer::entity::Entity>();

    assert_columns_match("customers", &schema, &entity);
}

#[tokio::test]
async fn user_entity_columns_match_schema() {
    let (pool, _dir) = migrated_pool().await;

    let schema = schema_columns(&pool, "users").await;
    let entity = entity_columns::<kikan::auth::entity_user::Entity>();

    assert_columns_match("users", &schema, &entity);
}

#[tokio::test]
async fn role_entity_columns_match_schema() {
    let (pool, _dir) = migrated_pool().await;

    let schema = schema_columns(&pool, "roles").await;
    let entity = entity_columns::<mokumo_db::role::entity::Entity>();

    assert_columns_match("roles", &schema, &entity);
}
