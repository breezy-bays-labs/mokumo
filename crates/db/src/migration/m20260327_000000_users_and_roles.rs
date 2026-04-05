use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        conn.execute_unprepared(
            "CREATE TABLE roles (
                id INTEGER PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                description TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            )",
        )
        .await?;

        conn.execute_unprepared(
            "INSERT INTO roles (id, name, description) VALUES
                (1, 'Admin', 'Full access to all features'),
                (2, 'Staff', 'Standard staff access'),
                (3, 'Guest', 'Read-only guest access')",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE TABLE users (
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

        // Diagnostic schema stamp (user_version is secondary to seaql_migrations).
        conn.execute_unprepared("PRAGMA user_version = 6").await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared("DROP TRIGGER IF EXISTS users_updated_at")
            .await?;
        conn.execute_unprepared("DROP INDEX IF EXISTS idx_users_deleted_at")
            .await?;
        conn.execute_unprepared("DROP TABLE IF EXISTS users")
            .await?;
        conn.execute_unprepared("DROP TABLE IF EXISTS roles")
            .await?;
        Ok(())
    }

    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }
}
