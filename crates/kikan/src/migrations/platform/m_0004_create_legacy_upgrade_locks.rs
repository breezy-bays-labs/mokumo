//! M00 platform migration: install-level `meta.legacy_upgrade_locks` table.
//!
//! Single-row table whose sole purpose is to serve as a guaranteed write
//! target for the legacy upgrade's TOCTOU lock-upgrade dance. Pre-PR-A
//! per-profile DBs hold `users` + `roles` tables that the post-PR-A
//! schema places on `meta.db`; the legacy upgrade migrates them across
//! and must atomically observe meta state during the move.
//!
//! Why a dedicated table instead of writing to an existing one (e.g.,
//! `profiles`)? Two reasons:
//!
//! 1. **Optimizer-proof.** SQLite's query planner can fold a constant-
//!    false `WHERE 1=0` predicate before emitting the `OP_OpenWrite`
//!    opcode that acquires the RESERVED lock. A targeted INSERT that
//!    actually writes a row sidesteps that risk entirely. The
//!    `legacy_upgrade_locks` row is the write that forces the lock
//!    upgrade.
//!
//! 2. **Audit trail.** `locked_at` records the most recent upgrade
//!    attempt timestamp. Useful for diagnosing crashed-mid-upgrade
//!    installs at support time without grovelling through journals.
//!
//! See `kikan::meta::upgrade::run_legacy_upgrade` for the lock-upgrade
//! INSERT. The table is intentionally meta-only because the upgrade
//! orchestration runs against `meta.db`.

use crate::migrations::conn::MigrationConn;
use crate::migrations::{GraftId, Migration, MigrationRef, MigrationTarget};

use super::PlatformMigrations;

pub(crate) struct CreateLegacyUpgradeLocks;

#[async_trait::async_trait]
impl Migration for CreateLegacyUpgradeLocks {
    fn name(&self) -> &'static str {
        "m20260427_000000_create_legacy_upgrade_locks"
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
        // Singleton row enforced by `CHECK (id = 1)` — the lock table
        // never grows beyond one row regardless of upgrade attempt count
        // (the INSERT-OR-UPDATE pattern in `run_legacy_upgrade` refreshes
        // `locked_at` on each attempt rather than appending).
        conn.execute_unprepared(
            "CREATE TABLE legacy_upgrade_locks (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                locked_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            )",
        )
        .await?;

        Ok(())
    }
}
