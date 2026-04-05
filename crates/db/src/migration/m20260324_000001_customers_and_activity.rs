use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        conn.execute_unprepared(
            "CREATE TABLE customers (
                id TEXT PRIMARY KEY,
                company_name TEXT,
                display_name TEXT NOT NULL,
                email TEXT,
                phone TEXT,
                address_line1 TEXT,
                address_line2 TEXT,
                city TEXT,
                state TEXT,
                postal_code TEXT,
                country TEXT DEFAULT 'US',
                notes TEXT,
                portal_enabled BOOLEAN NOT NULL DEFAULT FALSE,
                portal_user_id TEXT,
                tax_exempt BOOLEAN NOT NULL DEFAULT FALSE,
                tax_exemption_certificate_path TEXT,
                tax_exemption_expires_at TEXT,
                payment_terms TEXT DEFAULT 'due_on_receipt',
                credit_limit_cents INTEGER,
                stripe_customer_id TEXT,
                quickbooks_customer_id TEXT,
                lead_source TEXT,
                tags TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                deleted_at TEXT
            )",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE TRIGGER customers_updated_at AFTER UPDATE ON customers
             FOR EACH ROW BEGIN
                 UPDATE customers SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = OLD.id;
             END",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE TABLE activity_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                action TEXT NOT NULL,
                actor_id TEXT NOT NULL DEFAULT 'system',
                actor_type TEXT NOT NULL DEFAULT 'system',
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            )",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE INDEX idx_activity_log_entity ON activity_log(entity_type, entity_id)",
        )
        .await?;

        conn.execute_unprepared("CREATE INDEX idx_activity_log_type ON activity_log(entity_type)")
            .await?;

        // Diagnostic schema stamp (user_version is secondary to seaql_migrations).
        conn.execute_unprepared("PRAGMA user_version = 4").await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared("DROP INDEX IF EXISTS idx_activity_log_type")
            .await?;
        conn.execute_unprepared("DROP INDEX IF EXISTS idx_activity_log_entity")
            .await?;
        conn.execute_unprepared("DROP TABLE IF EXISTS activity_log")
            .await?;
        conn.execute_unprepared("DROP TRIGGER IF EXISTS customers_updated_at")
            .await?;
        conn.execute_unprepared("DROP TABLE IF EXISTS customers")
            .await?;
        Ok(())
    }

    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }
}
