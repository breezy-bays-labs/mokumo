use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS number_sequences (
                name         TEXT    PRIMARY KEY NOT NULL,
                prefix       TEXT    NOT NULL,
                current_value INTEGER NOT NULL DEFAULT 0,
                padding      INTEGER NOT NULL DEFAULT 4
            )",
        )
        .await?;
        conn.execute_unprepared(
            "INSERT INTO number_sequences (name, prefix, current_value, padding)
             VALUES ('customer', 'C', 0, 4)",
        )
        .await?;
        // Diagnostic schema stamp (user_version is secondary to seaql_migrations).
        conn.execute_unprepared("PRAGMA user_version = 3").await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared("DROP TABLE IF EXISTS number_sequences")
            .await?;
        Ok(())
    }

    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }
}
