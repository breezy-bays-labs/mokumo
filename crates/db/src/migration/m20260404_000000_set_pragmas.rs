use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// PRAGMA application_id value that identifies a Mokumo database.
/// "MKMO" encoded as a big-endian 32-bit integer (0x4D4B4D4F).
const MOKUMO_APPLICATION_ID: i64 = 0x4D4B4D4F; // = 1296780623

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        // Stamp the database as a Mokumo database. Any SQLite file with a different
        // non-zero application_id will be rejected at startup by check_application_id().
        //
        // Note: PRAGMA application_id and PRAGMA user_version write directly to the
        // SQLite file header. These writes are NOT transactional — they persist even
        // if the surrounding transaction rolls back. This is acceptable: a failed
        // migration aborts startup regardless, and the stamp is purely advisory.
        conn.execute_unprepared(&format!("PRAGMA application_id = {MOKUMO_APPLICATION_ID}"))
            .await?;

        // Stamp the schema version. user_version is diagnostic only — seaql_migrations
        // is the authoritative source of truth for migration state.
        conn.execute_unprepared("PRAGMA user_version = 7").await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared("PRAGMA application_id = 0").await?;
        conn.execute_unprepared("PRAGMA user_version = 6").await?;
        Ok(())
    }

    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }
}
