use sea_orm::{ConnectionTrait, DatabaseTransaction, Statement, Value};

use crate::activity::{ActivityLogEntry, ActivityWriter};
use crate::error::ActivityWriteError;

/// SQLite implementation of [`ActivityWriter`].
///
/// Writes rows into `activity_log` with a second-precision RFC3339
/// timestamp computed in Rust. This is byte-identical to the migration's
/// `DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))`, preserving R13
/// wire-format continuity when the writer takes over from the DEFAULT.
pub struct SqliteActivityWriter;

impl SqliteActivityWriter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SqliteActivityWriter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ActivityWriter for SqliteActivityWriter {
    async fn log(
        &self,
        tx: &DatabaseTransaction,
        entry: ActivityLogEntry,
    ) -> Result<(), ActivityWriteError> {
        let payload_str = serde_json::to_string(&entry.payload)?;
        let created_at = entry.occurred_at.format("%Y-%m-%dT%H:%M:%SZ").to_string();

        tx.execute_raw(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "INSERT INTO activity_log \
             (entity_type, entity_id, action, actor_id, actor_type, payload, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            vec![
                Value::from(entry.entity_kind),
                Value::from(entry.entity_id),
                Value::from(entry.action),
                Value::from(entry.actor_id.unwrap_or_else(|| "system".to_string())),
                Value::from(entry.actor_type),
                Value::from(payload_str),
                Value::from(created_at),
            ],
        ))
        .await?;
        Ok(())
    }
}
