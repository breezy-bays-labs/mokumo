//! M00 platform migration: install-level `meta.activity_log` table.
//!
//! Mirrors the per-profile `activity_log` schema (created by mokumo-shop's
//! `m20260324_000001_customers_and_activity`) so writers can use the same
//! `kikan::activity::insert_activity_log_raw` adapter against either pool
//! without branching on shape.
//!
//! Install-level events live here: profile lifecycle (create/archive/
//! reactivate/hard-delete), legacy install upgrade, demo self-repair, and
//! anything else that crosses the per-profile boundary. Per-profile
//! mutations (customers, quotes, invoices) keep writing to their own
//! profile DB's `activity_log`. The audit reader's UX picks the relevant
//! table by event scope; both share the same row shape so the same
//! `ActivityRow` deserializer works on either.
//!
//! Required by `kikan::meta::upgrade::run_legacy_upgrade` (M00 Seam 2)
//! and by PR B's `profiles::repo::hard_delete` (M00 Seam 3 — the
//! per-profile DB is gone post-delete, so the audit entry has no other
//! place to live).

use crate::migrations::conn::MigrationConn;
use crate::migrations::{GraftId, Migration, MigrationRef, MigrationTarget};

use super::PlatformMigrations;

pub(crate) struct CreateMetaActivityLog;

#[async_trait::async_trait]
impl Migration for CreateMetaActivityLog {
    fn name(&self) -> &'static str {
        "m20260425_000003_create_meta_activity_log"
    }

    fn graft_id(&self) -> GraftId {
        PlatformMigrations::graft_id()
    }

    fn target(&self) -> MigrationTarget {
        MigrationTarget::Meta
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        Vec::new()
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), sea_orm::DbErr> {
        // `IF NOT EXISTS` keeps the migration safe in single-pool dev/test
        // paths (`mokumo_shop::db::initialize_database`) where the
        // per-profile vertical migration `m20260324_000001_customers_and_activity`
        // also creates an `activity_log` on the same pool. Both schemas are
        // byte-identical, so the second creation is a logical no-op. In
        // production (proper meta.db pool) the table never pre-exists.
        conn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS activity_log (
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
        .await?;

        conn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_activity_log_entity \
             ON activity_log(entity_type, entity_id)",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_activity_log_type ON activity_log(entity_type)",
        )
        .await?;

        Ok(())
    }
}
