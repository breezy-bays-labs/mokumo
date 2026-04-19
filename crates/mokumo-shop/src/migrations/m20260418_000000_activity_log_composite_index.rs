use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared(
            "CREATE INDEX idx_activity_log_created_at_id \
             ON activity_log(created_at DESC, id DESC)",
        )
        .await?;
        // Diagnostic schema stamp (user_version is secondary to seaql_migrations).
        conn.execute_unprepared("PRAGMA user_version = 10").await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared("DROP INDEX IF EXISTS idx_activity_log_created_at_id")
            .await?;
        conn.execute_unprepared("PRAGMA user_version = 9").await?;
        Ok(())
    }

    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }
}
