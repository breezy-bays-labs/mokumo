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
        // SQLite ALTER TABLE DROP COLUMN is unavailable on older versions, and
        // `CREATE TABLE AS SELECT` would lose the primary key, UNIQUE, FK, and
        // DEFAULT constraints. Recreate the table with the original schema from
        // m20260327_000000_users_and_roles, copy data, then restore the
        // partial index and updated_at trigger.
        let conn = manager.get_connection();

        conn.execute_unprepared("DROP TRIGGER IF EXISTS users_updated_at")
            .await?;
        conn.execute_unprepared("DROP INDEX IF EXISTS idx_users_deleted_at")
            .await?;

        conn.execute_unprepared(
            "CREATE TABLE users_new (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                role_id INTEGER NOT NULL DEFAULT 1 REFERENCES roles(id) ON DELETE RESTRICT,
                is_active BOOLEAN NOT NULL DEFAULT 1,
                last_login_at TEXT,
                recovery_code_hash TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                deleted_at TEXT
            )",
        )
        .await?;

        conn.execute_unprepared(
            "INSERT INTO users_new (
                id, email, name, password_hash, role_id, is_active,
                last_login_at, recovery_code_hash, created_at, updated_at, deleted_at
             )
             SELECT
                id, email, name, password_hash, role_id, is_active,
                last_login_at, recovery_code_hash, created_at, updated_at, deleted_at
             FROM users",
        )
        .await?;

        conn.execute_unprepared("DROP TABLE users").await?;

        conn.execute_unprepared("ALTER TABLE users_new RENAME TO users")
            .await?;

        conn.execute_unprepared(
            "CREATE INDEX idx_users_deleted_at ON users(id) WHERE deleted_at IS NULL",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE TRIGGER users_updated_at AFTER UPDATE ON users
             FOR EACH ROW BEGIN
                 UPDATE users SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = OLD.id;
             END",
        )
        .await?;

        conn.execute_unprepared("PRAGMA user_version = 8").await?;

        Ok(())
    }

    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }
}
