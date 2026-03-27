use crate::db_err;
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

/// Parse a SQLite timestamp string (e.g. `2026-03-27T12:00:00Z`) into `DateTime<Utc>`.
fn parse_sqlite_timestamp(s: &str) -> Result<DateTime<Utc>, DomainError> {
    // Try ISO 8601 with timezone suffix first
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Fallback: parse as naive datetime (no timezone) and assume UTC
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
/// Called from entity repo adapters (customer, garment, quote, etc.)
/// during mutation transactions. The caller owns the transaction —
/// this function only executes the INSERT.
pub(crate) async fn insert_activity_log_raw(
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
    .map_err(crate::sea_err)?;

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
