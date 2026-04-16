use sea_orm_migration::prelude::*;

/// Adds login rate-limiting columns to the users table.
///
/// - `failed_login_attempts`: consecutive failed login counter; reset to 0 on
///   successful authentication.
/// - `locked_until`: ISO-8601 UTC timestamp after which the account is
///   automatically unlocked; NULL when not locked.
///
/// Both columns default to their "not locked" state so existing rows are
/// valid immediately after migration without a backfill.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        conn.execute_unprepared(
            "ALTER TABLE users ADD COLUMN failed_login_attempts INTEGER NOT NULL DEFAULT 0",
        )
        .await?;

        conn.execute_unprepared("ALTER TABLE users ADD COLUMN locked_until TEXT NULL")
            .await?;

        // Keep the existing updated_at trigger in sync with the new columns.
        // The users table already has an AFTER UPDATE trigger from the initial
        // migration; no new trigger is required — it fires on any column update.

        // Diagnostic schema stamp (user_version is secondary to seaql_migrations).
        conn.execute_unprepared("PRAGMA user_version = 9").await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite does not support DROP COLUMN for columns added via ALTER TABLE
        // on older SQLite versions. Recreate the table without the new columns.
        let conn = manager.get_connection();

        conn.execute_unprepared(
            "CREATE TABLE users_backup AS SELECT
                id, email, name, password_hash, role_id, is_active,
                last_login_at, recovery_code_hash, created_at, updated_at, deleted_at
             FROM users",
        )
        .await?;

        conn.execute_unprepared("DROP TABLE users").await?;

        conn.execute_unprepared("ALTER TABLE users_backup RENAME TO users")
            .await?;

        conn.execute_unprepared("PRAGMA user_version = 8").await?;

        Ok(())
    }

    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }
}
