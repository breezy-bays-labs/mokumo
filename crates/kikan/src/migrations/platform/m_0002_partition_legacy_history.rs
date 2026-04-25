//! Partition legacy `kikan_migrations` history rows so engine-platform
//! migrations carry the new `kikan::engine` graft id.
//!
//! Per `adr-kikan-upgrade-migration-strategy.md` §"Existing Mokumo
//! migrations partition": engine-owned migrations moved from the
//! catch-all `kikan` graft id to `kikan::engine`, and existing history
//! rows must be relabeled in place so the runner does not try to re-apply
//! them.
//!
//! Target is `Meta`; this migration runs against `meta.db` only. On a
//! fresh install `meta.db.kikan_migrations` is empty when this migration
//! runs, so the UPDATE matches zero rows. The per-profile side of the
//! partition (relabeling rows in legacy per-profile DBs) belongs to the
//! legacy-install upgrade handler in `kikan::meta`, alongside moving the
//! engine-platform tables off the per-profile pools.

use crate::migrations::conn::MigrationConn;
use crate::migrations::{GraftId, Migration, MigrationRef, MigrationTarget};

use super::PlatformMigrations;

pub(crate) struct PartitionLegacyHistory;

#[async_trait::async_trait]
impl Migration for PartitionLegacyHistory {
    fn name(&self) -> &'static str {
        "m20260425_000001_partition_legacy_history"
    }

    fn graft_id(&self) -> GraftId {
        PlatformMigrations::graft_id()
    }

    fn target(&self) -> MigrationTarget {
        MigrationTarget::Meta
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        // No DAG dependency — the rows being relabeled belong to migrations
        // that may or may not have applied to meta.db (depending on legacy
        // vs fresh install). The UPDATE is idempotent and safe in both
        // directions.
        Vec::new()
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), sea_orm::DbErr> {
        // Listed names are the engine-platform migrations being relabeled.
        // No-op on fresh installs (zero matching rows).
        conn.execute_unprepared(
            "UPDATE kikan_migrations
                SET graft_id = 'kikan::engine'
              WHERE graft_id = 'kikan'
                AND name IN (
                    'm20260327_000000_users_and_roles',
                    'm20260424_000000_profile_user_roles',
                    'm20260424_000001_prevent_last_admin_deactivation',
                    'm20260424_000002_active_integrations',
                    'm20260424_000003_integration_event_log'
                )",
        )
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, FromQueryResult, Statement};

    #[derive(Debug, FromQueryResult)]
    struct Row {
        graft_id: String,
        name: String,
    }

    async fn seeded_pool() -> sea_orm::DatabaseConnection {
        let pool = crate::db::initialize_database("sqlite::memory:")
            .await
            .unwrap();
        // mirror the runner's bootstrap so kikan_migrations exists, then
        // seed the legacy graft_id rows the partition is supposed to relabel
        // plus a decoy that must NOT be relabeled.
        pool.execute_unprepared(crate::migrations::bootstrap::KIKAN_MIGRATIONS_SQL)
            .await
            .unwrap();
        for stmt in [
            "INSERT INTO kikan_migrations (graft_id, name, applied_at) VALUES \
             ('kikan', 'm20260327_000000_users_and_roles', 1)",
            "INSERT INTO kikan_migrations (graft_id, name, applied_at) VALUES \
             ('kikan', 'm20260424_000000_profile_user_roles', 1)",
            "INSERT INTO kikan_migrations (graft_id, name, applied_at) VALUES \
             ('kikan', 'm20260424_000001_prevent_last_admin_deactivation', 1)",
            "INSERT INTO kikan_migrations (graft_id, name, applied_at) VALUES \
             ('kikan', 'm20260424_000002_active_integrations', 1)",
            "INSERT INTO kikan_migrations (graft_id, name, applied_at) VALUES \
             ('kikan', 'm20260424_000003_integration_event_log', 1)",
            "INSERT INTO kikan_migrations (graft_id, name, applied_at) VALUES \
             ('kikan', 'create_kikan_migrations', 1)",
        ] {
            pool.execute_unprepared(stmt).await.unwrap();
        }
        pool
    }

    async fn fetch_rows(pool: &sea_orm::DatabaseConnection) -> Vec<Row> {
        Row::find_by_statement(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT graft_id, name FROM kikan_migrations ORDER BY name".to_string(),
        ))
        .all(pool)
        .await
        .unwrap()
    }

    async fn run_partition(pool: &sea_orm::DatabaseConnection) {
        let txn = sea_orm::TransactionTrait::begin(pool).await.unwrap();
        let conn = MigrationConn::new(txn);
        PartitionLegacyHistory.up(&conn).await.unwrap();
        conn.into_inner().commit().await.unwrap();
    }

    #[tokio::test]
    async fn relabels_listed_engine_platform_rows_only() {
        let pool = seeded_pool().await;
        run_partition(&pool).await;
        let rows = fetch_rows(&pool).await;
        let by_name: std::collections::HashMap<&str, &str> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.graft_id.as_str()))
            .collect();

        for name in [
            "m20260327_000000_users_and_roles",
            "m20260424_000000_profile_user_roles",
            "m20260424_000001_prevent_last_admin_deactivation",
            "m20260424_000002_active_integrations",
            "m20260424_000003_integration_event_log",
        ] {
            assert_eq!(
                by_name.get(name),
                Some(&"kikan::engine"),
                "expected `{name}` to be relabeled"
            );
        }

        assert_eq!(
            by_name.get("create_kikan_migrations"),
            Some(&"kikan"),
            "decoy bootstrap row must remain on `kikan` graft_id"
        );
    }

    #[tokio::test]
    async fn is_idempotent() {
        let pool = seeded_pool().await;
        run_partition(&pool).await;
        run_partition(&pool).await;
        let rows = fetch_rows(&pool).await;
        assert!(
            rows.iter()
                .filter(|r| r.name == "m20260327_000000_users_and_roles")
                .all(|r| r.graft_id == "kikan::engine")
        );
    }
}
