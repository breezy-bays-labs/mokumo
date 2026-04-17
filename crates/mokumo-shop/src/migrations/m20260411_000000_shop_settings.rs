use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

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

        // Backfill shop_name from existing key-value store if present.
        conn.execute_unprepared(
            "INSERT OR IGNORE INTO shop_settings (id, shop_name)
             SELECT 1, COALESCE(value, '') FROM settings WHERE key = 'shop_name'",
        )
        .await?;

        // Ensure singleton row exists even if shop_name was never set.
        conn.execute_unprepared(
            "INSERT OR IGNORE INTO shop_settings (id, shop_name) VALUES (1, '')",
        )
        .await?;

        // Trigger to auto-update updated_at on any mutation.
        // Guard: WHEN NEW.updated_at = OLD.updated_at prevents infinite recursion.
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

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared("DROP TRIGGER IF EXISTS shop_settings_updated_at")
            .await?;
        conn.execute_unprepared("DROP TABLE IF EXISTS shop_settings")
            .await?;
        conn.execute_unprepared("PRAGMA user_version = 7").await?;
        Ok(())
    }

    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }
}
