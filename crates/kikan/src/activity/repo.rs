//! Reader half of the activity log — `SqliteActivityLogRepo`.
//!
//! The writer half (`SqliteActivityWriter`) lives in `crate::activity::sqlite`.
//! Together they own the platform's activity-log persistence contract; verticals
//! call `insert_activity_log_raw` from within their mutation transactions
//! (see the `pub` re-export at the module root).

use chrono::{DateTime, NaiveDateTime, Utc};
use mokumo_core::activity::traits::ActivityLogRepository;
use mokumo_core::activity::{ActivityAction, ActivityEntry};
use mokumo_core::error::DomainError;
use mokumo_core::pagination::PageParams;
use sqlx::SqlitePool;

#[derive(sqlx::FromRow)]
struct ActivityRow {
    id: i64,
    entity_type: String,
    entity_id: String,
    action: String,
    actor_id: String,
    actor_type: String,
    payload: String,
    created_at: String,
}

fn db_err(e: sqlx::Error) -> DomainError {
    DomainError::Internal {
        message: e.to_string(),
    }
}

fn sea_err(e: sea_orm::DbErr) -> DomainError {
    DomainError::Internal {
        message: e.to_string(),
    }
}

/// Parse a SQLite timestamp string (e.g. `2026-03-27T12:00:00Z`) into `DateTime<Utc>`.
fn parse_sqlite_timestamp(s: &str) -> Result<DateTime<Utc>, DomainError> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .map(|naive| naive.and_utc())
        .map_err(|e| DomainError::Internal {
            message: format!("invalid timestamp in activity log: {e}"),
        })
}

fn row_to_entry(row: ActivityRow) -> Result<ActivityEntry, DomainError> {
    let payload: serde_json::Value =
        serde_json::from_str(&row.payload).map_err(|e| DomainError::Internal {
            message: format!("invalid JSON in activity payload: {e}"),
        })?;
    let created_at = parse_sqlite_timestamp(&row.created_at)?;
    Ok(ActivityEntry {
        id: row.id,
        entity_type: row.entity_type,
        entity_id: row.entity_id,
        action: row.action,
        actor_id: row.actor_id,
        actor_type: row.actor_type,
        payload,
        created_at,
    })
}

/// Insert an activity log entry using the provided connection.
///
/// Called from entity repo adapters during mutation transactions. The caller
/// owns the transaction — this function only executes the INSERT. `entity_type`
/// and `actor_type` are opaque strings owned by the calling vertical; kikan
/// passes them through without interpretation.
pub async fn insert_activity_log_raw(
    conn: &impl sea_orm::ConnectionTrait,
    entity_type: &str,
    entity_id: &str,
    action: ActivityAction,
    actor_id: &str,
    actor_type: &str,
    payload: &serde_json::Value,
) -> Result<(), DomainError> {
    let payload_str = serde_json::to_string(payload).map_err(|e| DomainError::Internal {
        message: format!("failed to serialize activity payload: {e}"),
    })?;

    conn.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Sqlite,
        "INSERT INTO activity_log (entity_type, entity_id, action, actor_id, actor_type, payload) VALUES (?, ?, ?, ?, ?, ?)",
        vec![
            sea_orm::Value::from(entity_type.to_string()),
            sea_orm::Value::from(entity_id.to_string()),
            sea_orm::Value::from(action.to_string()),
            sea_orm::Value::from(actor_id.to_string()),
            sea_orm::Value::from(actor_type.to_string()),
            sea_orm::Value::from(payload_str),
        ],
    ))
    .await
    .map_err(sea_err)?;

    Ok(())
}

pub struct SqliteActivityLogRepo {
    pool: SqlitePool,
}

impl SqliteActivityLogRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

impl ActivityLogRepository for SqliteActivityLogRepo {
    async fn list(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        params: PageParams,
    ) -> Result<(Vec<ActivityEntry>, i64), DomainError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM activity_log \
             WHERE (?1 IS NULL OR entity_type = ?1) \
             AND (?2 IS NULL OR entity_id = ?2)",
        )
        .bind(entity_type)
        .bind(entity_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;

        let rows: Vec<ActivityRow> = sqlx::query_as(
            "SELECT * FROM activity_log \
             WHERE (?1 IS NULL OR entity_type = ?1) \
             AND (?2 IS NULL OR entity_id = ?2) \
             ORDER BY created_at DESC, id DESC \
             LIMIT ?3 OFFSET ?4",
        )
        .bind(entity_type)
        .bind(entity_id)
        .bind(params.per_page() as i64)
        .bind(params.offset() as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let entries: Vec<ActivityEntry> = rows
            .into_iter()
            .map(row_to_entry)
            .collect::<Result<_, _>>()?;
        Ok((entries, count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mokumo_core::activity::traits::ActivityLogRepository;
    use mokumo_core::pagination::PageParams;

    /// Create a pool with the activity_log table initialized. Kikan's test
    /// harness can't run mokumo's SeaORM migrator (DAG would flow the wrong
    /// way), so the table is created inline. Schema is byte-identical to
    /// `m20260324_000001_customers_and_activity.rs`.
    async fn test_pool() -> (SqlitePool, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
        let db = crate::db::initialize_database(&url).await.unwrap();
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
        (pool, tmp)
    }

    async fn insert_activity(pool: &SqlitePool, entity_type: &str, entity_id: &str, action: &str) {
        sqlx::query(
            "INSERT INTO activity_log \
             (entity_type, entity_id, action, actor_id, actor_type, payload) \
             VALUES (?, ?, ?, 'system', 'system', '{}')",
        )
        .bind(entity_type)
        .bind(entity_id)
        .bind(action)
        .execute(pool)
        .await
        .unwrap();
    }

    #[test]
    fn row_to_entry_valid_rfc3339_timestamp() {
        let row = ActivityRow {
            id: 1,
            entity_type: "thing".to_string(),
            entity_id: "abc".to_string(),
            action: "created".to_string(),
            actor_id: "user-1".to_string(),
            actor_type: "user".to_string(),
            payload: r#"{"key":"value"}"#.to_string(),
            created_at: "2026-03-27T12:00:00Z".to_string(),
        };
        let entry = row_to_entry(row).unwrap();
        assert_eq!(entry.id, 1);
        assert_eq!(entry.entity_type, "thing");
        assert_eq!(entry.payload["key"], "value");
    }

    #[test]
    fn row_to_entry_naive_datetime_without_timezone() {
        let row = ActivityRow {
            id: 2,
            entity_type: "thing".to_string(),
            entity_id: "def".to_string(),
            action: "updated".to_string(),
            actor_id: "system".to_string(),
            actor_type: "system".to_string(),
            payload: "{}".to_string(),
            created_at: "2026-03-27T12:00:00".to_string(),
        };
        let entry = row_to_entry(row).unwrap();
        assert_eq!(entry.id, 2);
    }

    #[test]
    fn row_to_entry_naive_space_separated_timestamp() {
        let row = ActivityRow {
            id: 3,
            entity_type: "thing".to_string(),
            entity_id: "ghi".to_string(),
            action: "deleted".to_string(),
            actor_id: "system".to_string(),
            actor_type: "system".to_string(),
            payload: "{}".to_string(),
            created_at: "2026-03-27 12:00:00".to_string(),
        };
        let entry = row_to_entry(row).unwrap();
        assert_eq!(entry.id, 3);
    }

    #[test]
    fn row_to_entry_invalid_json_returns_error() {
        let row = ActivityRow {
            id: 4,
            entity_type: "thing".to_string(),
            entity_id: "jkl".to_string(),
            action: "created".to_string(),
            actor_id: "system".to_string(),
            actor_type: "system".to_string(),
            payload: "not-json{{".to_string(),
            created_at: "2026-03-27T12:00:00Z".to_string(),
        };
        assert!(row_to_entry(row).is_err());
    }

    #[test]
    fn row_to_entry_invalid_timestamp_returns_error() {
        let row = ActivityRow {
            id: 5,
            entity_type: "thing".to_string(),
            entity_id: "mno".to_string(),
            action: "created".to_string(),
            actor_id: "system".to_string(),
            actor_type: "system".to_string(),
            payload: "{}".to_string(),
            created_at: "not-a-date".to_string(),
        };
        assert!(row_to_entry(row).is_err());
    }

    #[tokio::test]
    async fn list_empty_returns_zero_count() {
        let (pool, _tmp) = test_pool().await;
        let repo = SqliteActivityLogRepo::new(pool);
        let (entries, count) = repo
            .list(None, None, PageParams::new(None, None))
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn list_all_returns_all_entries() {
        let (pool, _tmp) = test_pool().await;
        insert_activity(&pool, "thing", "t1", "created").await;
        insert_activity(&pool, "thing", "t2", "created").await;
        let repo = SqliteActivityLogRepo::new(pool);
        let (entries, count) = repo
            .list(None, None, PageParams::new(None, None))
            .await
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn list_filter_by_entity_type() {
        let (pool, _tmp) = test_pool().await;
        insert_activity(&pool, "thing", "t1", "created").await;
        insert_activity(&pool, "widget", "w1", "created").await;
        let repo = SqliteActivityLogRepo::new(pool);
        let (entries, count) = repo
            .list(Some("thing"), None, PageParams::new(None, None))
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(entries[0].entity_type, "thing");
    }

    #[tokio::test]
    async fn list_filter_by_entity_id() {
        let (pool, _tmp) = test_pool().await;
        insert_activity(&pool, "thing", "t1", "created").await;
        insert_activity(&pool, "thing", "t2", "created").await;
        let repo = SqliteActivityLogRepo::new(pool);
        let (entries, count) = repo
            .list(None, Some("t1"), PageParams::new(None, None))
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(entries[0].entity_id, "t1");
    }

    #[tokio::test]
    async fn list_pagination_limits_results() {
        let (pool, _tmp) = test_pool().await;
        for i in 0..5 {
            insert_activity(&pool, "thing", &format!("t{i}"), "created").await;
        }
        let repo = SqliteActivityLogRepo::new(pool);
        let (entries, count) = repo
            .list(None, None, PageParams::new(Some(1), Some(2)))
            .await
            .unwrap();
        assert_eq!(count, 5, "total count should be all entries");
        assert_eq!(entries.len(), 2, "page size should limit results to 2");
    }
}
