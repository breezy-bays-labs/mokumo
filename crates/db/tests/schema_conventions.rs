//! Schema convention tests — PRAGMA introspection for migration quality.
//!
//! These tests run all migrations against a fresh SQLite database, then
//! introspect the resulting schema with PRAGMA queries to verify conventions.
//! They are ORM-agnostic: they verify the database itself, not the ORM's
//! understanding of it.
//!
//! See: issue #52, ADR `adr-seaorm-testing-standards.md`

use mokumo_db::initialize_database;
use sqlx::Row;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Spin up a temp database with all migrations applied.
async fn migrated_pool() -> (sqlx::SqlitePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = initialize_database(&url).await.unwrap();
    (pool, dir)
}

#[derive(Debug)]
struct ColumnInfo {
    name: String,
    col_type: String,
    notnull: bool,
    dflt_value: Option<String>,
}

async fn table_columns(pool: &sqlx::SqlitePool, table: &str) -> Vec<ColumnInfo> {
    let sql = format!("PRAGMA table_info('{}')", table);
    sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| ColumnInfo {
            name: row.get("name"),
            col_type: row.get("type"),
            notnull: row.get::<bool, _>("notnull"),
            dflt_value: row.get("dflt_value"),
        })
        .collect()
}

async fn trigger_names(pool: &sqlx::SqlitePool, table: &str) -> Vec<String> {
    sqlx::query("SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?")
        .bind(table)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get("name"))
        .collect()
}

#[derive(Debug)]
struct ForeignKeyInfo {
    table: String,
    from: String,
    to: String,
    on_delete: String,
}

async fn foreign_keys(pool: &sqlx::SqlitePool, table: &str) -> Vec<ForeignKeyInfo> {
    let sql = format!("PRAGMA foreign_key_list('{}')", table);
    sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| ForeignKeyInfo {
            table: row.get("table"),
            from: row.get("from"),
            to: row.get("to"),
            on_delete: row.get("on_delete"),
        })
        .collect()
}

#[derive(Debug)]
struct IndexInfo {
    name: String,
    partial: bool,
}

async fn index_list(pool: &sqlx::SqlitePool, table: &str) -> Vec<IndexInfo> {
    let sql = format!("PRAGMA index_list('{}')", table);
    sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| IndexInfo {
            name: row.get("name"),
            partial: row.get::<bool, _>("partial"),
        })
        .collect()
}

async fn index_sql(pool: &sqlx::SqlitePool, index_name: &str) -> Option<String> {
    sqlx::query("SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?")
        .bind(index_name)
        .fetch_optional(pool)
        .await
        .unwrap()
        .map(|row| row.get("sql"))
}

async fn user_tables(pool: &sqlx::SqlitePool) -> Vec<String> {
    sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' \
         AND name NOT LIKE 'sqlite_%' \
         AND name NOT LIKE '_sqlx_%' \
         ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.get("name"))
    .collect()
}

// ---------------------------------------------------------------------------
// Convention: money columns (_cents suffix) must be INTEGER, never REAL or TEXT
// ---------------------------------------------------------------------------

#[tokio::test]
async fn money_columns_are_integer() {
    let (pool, _dir) = migrated_pool().await;

    // Auto-discover: any column ending in _cents must be INTEGER.
    // This avoids a static registry that drifts as tables grow.
    for table in user_tables(&pool).await {
        let columns = table_columns(&pool, &table).await;
        for col in &columns {
            if col.name.ends_with("_cents") {
                assert_eq!(
                    col.col_type.to_uppercase(),
                    "INTEGER",
                    "Money column {}.{} must be INTEGER (cents), found {}",
                    table,
                    col.name,
                    col.col_type
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Convention: timestamp/date columns (_at suffix) must be TEXT (ISO 8601)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn timestamp_columns_are_text() {
    let (pool, _dir) = migrated_pool().await;

    // Auto-discover: any column ending in _at is a timestamp and must be TEXT.
    // This avoids a static allowlist that drifts as tables grow.
    for table in user_tables(&pool).await {
        let columns = table_columns(&pool, &table).await;
        for col in &columns {
            if col.name.ends_with("_at") {
                assert_eq!(
                    col.col_type.to_uppercase(),
                    "TEXT",
                    "Timestamp column {}.{} must be TEXT (ISO 8601), found {}",
                    table,
                    col.name,
                    col.col_type
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Convention: every mutable table with updated_at gets an updated_at trigger
// ---------------------------------------------------------------------------

/// Tables explicitly exempt from the updated_at trigger requirement.
/// Each exemption must have a documented reason.
const TRIGGER_EXEMPT_TABLES: &[&str] = &[
    "settings",         // simple KV store, no updated_at column
    "number_sequences", // infrastructure counter, not a domain entity
    "activity_log",     // append-only audit log, never updated
];

#[tokio::test]
async fn mutable_tables_have_updated_at_trigger() {
    let (pool, _dir) = migrated_pool().await;

    for table in user_tables(&pool).await {
        let columns = table_columns(&pool, &table).await;
        let has_updated_at = columns.iter().any(|c| c.name == "updated_at");

        if TRIGGER_EXEMPT_TABLES.contains(&table.as_str()) {
            // Catch stale exemptions: if someone adds updated_at to an exempt table,
            // the exemption should be removed so the trigger requirement kicks in.
            assert!(
                !has_updated_at,
                "Table '{}' is in TRIGGER_EXEMPT_TABLES but has an updated_at column. \
                 Remove it from the exemption list so the trigger requirement applies.",
                table
            );
            continue;
        }

        if has_updated_at {
            let triggers = trigger_names(&pool, &table).await;
            let trigger_name = format!("{}_updated_at", table);
            assert!(
                triggers.contains(&trigger_name),
                "Table '{}' has updated_at column but no '{}' trigger. \
                 Every mutable table must have an updated_at trigger. \
                 If this table is exempt, add it to TRIGGER_EXEMPT_TABLES with a reason.",
                table,
                trigger_name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Convention: every FK has an explicit ON DELETE clause (not default NO ACTION)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn foreign_keys_have_explicit_on_delete() {
    let (pool, _dir) = migrated_pool().await;

    for table in user_tables(&pool).await {
        let fks = foreign_keys(&pool, &table).await;
        for fk in &fks {
            assert_ne!(
                fk.on_delete.to_uppercase(),
                "NO ACTION",
                "FK {}.{} -> {}.{} uses default NO ACTION. \
                 Every FK must have an explicit ON DELETE clause \
                 (CASCADE, SET NULL, or RESTRICT).",
                table,
                fk.from,
                fk.table,
                fk.to
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Convention: every deleted_at column has a partial index (WHERE deleted_at IS NULL)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn deleted_at_columns_have_partial_index() {
    let (pool, _dir) = migrated_pool().await;

    for table in user_tables(&pool).await {
        let columns = table_columns(&pool, &table).await;
        let has_deleted_at = columns.iter().any(|c| c.name == "deleted_at");

        if !has_deleted_at {
            continue;
        }

        // deleted_at must be nullable — NOT NULL would break soft-delete
        let deleted_at_col = columns.iter().find(|c| c.name == "deleted_at").unwrap();
        assert!(
            !deleted_at_col.notnull,
            "Table '{}' column deleted_at must be nullable (no NOT NULL). \
             Soft-delete requires NULL to mean 'active'.",
            table
        );

        let indexes = index_list(&pool, &table).await;
        let mut has_partial_deleted_at_index = false;
        for idx in &indexes {
            if idx.partial
                && let Some(sql) = index_sql(&pool, &idx.name).await
            {
                let sql_upper = sql.to_uppercase();
                if sql_upper.contains("DELETED_AT") && sql_upper.contains("IS NULL") {
                    has_partial_deleted_at_index = true;
                    break;
                }
            }
        }

        assert!(
            has_partial_deleted_at_index,
            "Table '{}' has a deleted_at column but no partial index \
             with WHERE deleted_at IS NULL. Soft-delete queries need this \
             index for performance.",
            table
        );
    }
}

// ---------------------------------------------------------------------------
// Convention: created_at and updated_at columns have ISO 8601 defaults
// ---------------------------------------------------------------------------

async fn assert_timestamp_has_default(pool: &sqlx::SqlitePool, column_name: &str) {
    for table in user_tables(pool).await {
        let columns = table_columns(pool, &table).await;
        for col in &columns {
            if col.name == column_name {
                assert!(
                    col.dflt_value.is_some(),
                    "Table '{}' column {} must have a DEFAULT value",
                    table,
                    column_name
                );
                let default = col.dflt_value.as_ref().unwrap().to_uppercase();
                assert!(
                    default.contains("STRFTIME") || default.contains("CURRENT_TIMESTAMP"),
                    "Table '{}' column {} default must use strftime or CURRENT_TIMESTAMP, \
                     found: {}",
                    table,
                    column_name,
                    col.dflt_value.as_ref().unwrap()
                );
            }
        }
    }
}

#[tokio::test]
async fn created_at_has_timestamp_default() {
    let (pool, _dir) = migrated_pool().await;
    assert_timestamp_has_default(&pool, "created_at").await;
}

#[tokio::test]
async fn updated_at_has_timestamp_default() {
    let (pool, _dir) = migrated_pool().await;
    assert_timestamp_has_default(&pool, "updated_at").await;
}
