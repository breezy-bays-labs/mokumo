// TODO(Invariant I1): Rename to profile_settings / display_name when the
// table rename lands (separate issue). The `shop_` prefix is platform
// infrastructure named in vertical vocabulary; it does not indicate that
// shop-domain logic belongs in kikan.

use crate::migrations::conn::MigrationConn;
use crate::migrations::{GraftId, Migration, MigrationRef, MigrationTarget};

use super::PlatformMigrations;

pub(crate) struct ShopSettings;

#[async_trait::async_trait]
impl Migration for ShopSettings {
    fn name(&self) -> &'static str {
        "m20260411_000000_shop_settings"
    }

    fn graft_id(&self) -> GraftId {
        PlatformMigrations::graft_id()
    }

    fn target(&self) -> MigrationTarget {
        MigrationTarget::PerProfile
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        vec![MigrationRef {
            graft: PlatformMigrations::graft_id(),
            name: "m20260327_000000_users_and_roles",
        }]
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), sea_orm::DbErr> {
        conn.execute_unprepared(
            "CREATE TABLE shop_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                shop_name TEXT NOT NULL DEFAULT '',
                logo_extension TEXT NULL,
                logo_epoch INTEGER NULL,
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            )",
        )
        .await?;

        // Ensure singleton row exists with empty defaults.
        // Legacy backfill from `settings` table removed — dev DB wipe means
        // no existing data to migrate (pre-alpha, #565).
        conn.execute_unprepared(
            "INSERT OR IGNORE INTO shop_settings (id, shop_name) VALUES (1, '')",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE TRIGGER shop_settings_updated_at
                AFTER UPDATE ON shop_settings
                FOR EACH ROW
                WHEN NEW.updated_at = OLD.updated_at
                BEGIN
                    UPDATE shop_settings
                       SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                     WHERE rowid = NEW.rowid;
                END",
        )
        .await?;

        conn.execute_unprepared("PRAGMA user_version = 8").await?;

        Ok(())
    }
}
