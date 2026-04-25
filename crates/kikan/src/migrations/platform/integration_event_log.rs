//! M00 platform migration: `integration_event_log` — per-integration event record.
//!
//! Named `integration_event_log`, not `integration_sync_history`, because
//! integrations have three dispatch patterns (sync-driven, webhook-driven,
//! scheduled); the event log records all three uniformly. See
//! `adr-mokumo-integrations` §ADR-3 — CAO F4 caught the `_sync_history`
//! framing as a leftover from the pre-webhook shape.
//!
//! Payload is stored `_redacted` — credentials and PII are scrubbed before
//! the event is persisted. The trait's `test_connection` and sync calls
//! handle the redaction; this table trusts the scrubbed form.
//!
//! Append-only: rows are inserted and never updated. There is no
//! `updated_at` column or AFTER UPDATE trigger by design; event records
//! are immutable history. A future refactor that introduces row mutation
//! here would violate the audit-trail contract.

use crate::migrations::conn::MigrationConn;
use crate::migrations::{GraftId, Migration, MigrationRef, MigrationTarget};

use super::PlatformMigrations;

pub(crate) struct IntegrationEventLog;

#[async_trait::async_trait]
impl Migration for IntegrationEventLog {
    fn name(&self) -> &'static str {
        "m20260424_000003_integration_event_log"
    }

    fn graft_id(&self) -> GraftId {
        PlatformMigrations::graft_id()
    }

    fn target(&self) -> MigrationTarget {
        MigrationTarget::PerProfile
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        // FK to active_integrations — must run second.
        vec![MigrationRef {
            graft: PlatformMigrations::graft_id(),
            name: "m20260424_000002_active_integrations",
        }]
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), sea_orm::DbErr> {
        conn.execute_unprepared(
            "CREATE TABLE integration_event_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                integration_id TEXT NOT NULL REFERENCES active_integrations(integration_id) ON DELETE RESTRICT,
                at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                event_type TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('ok', 'error')),
                error TEXT,
                payload_redacted TEXT,
                CHECK ((status = 'ok' AND error IS NULL) OR (status = 'error' AND error IS NOT NULL))
            )",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE INDEX idx_integration_event_log_integration_at
                ON integration_event_log(integration_id, at DESC)",
        )
        .await?;

        Ok(())
    }
}
