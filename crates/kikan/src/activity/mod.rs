pub mod repo;
pub mod sqlite;

pub use repo::{SqliteActivityLogRepo, insert_activity_log_raw};
pub use sqlite::SqliteActivityWriter;

/// Activity log entry written by verticals during mutation transactions.
///
/// The writer is deliberately domain-agnostic: `entity_kind`, `action`, and
/// `actor_type` are free-form strings owned by the caller. kikan guarantees
/// persistence semantics; the vertical guarantees the wire-format contract
/// (R13 action-string continuity).
pub struct ActivityLogEntry {
    /// Opaque UUID string. `None` for system-initiated actions.
    pub actor_id: Option<String>,
    pub actor_type: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub action: String,
    pub payload: serde_json::Value,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

/// Persist activity log entries as part of a mutation transaction.
///
/// Implementors MUST use the caller-supplied `tx` so the insert is atomic
/// with the entity mutation. `async-trait` is used here (rather than the
/// `trait_variant::make(Send)` convention elsewhere in kikan) because
/// `EngineContext` stores `Arc<dyn ActivityWriter>` — and `trait_variant`
/// does not produce object-safe traits.
#[async_trait::async_trait]
pub trait ActivityWriter: Send + Sync + 'static {
    async fn log(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        entry: ActivityLogEntry,
    ) -> Result<(), crate::error::ActivityWriteError>;
}
