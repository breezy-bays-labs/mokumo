use sea_orm_migration::prelude::*;
use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

/// Create an in-memory DatabaseConnection + raw pool via pool-first wrapping (Decision D1).
/// Returns both: DatabaseConnection for SeaORM, SqlitePool for raw query helpers.
async fn test_db() -> (sea_orm::DatabaseConnection, SqlitePool) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone());
    (db, pool)
}

/// Get user-defined table names (excludes sqlite_* and seaql_* internal tables).
async fn get_user_tables(pool: &SqlitePool) -> Vec<String> {
    sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'seaql_%' ORDER BY name")
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect()
}

/// Get migration version strings from seaql_migrations.
async fn get_migration_versions(pool: &SqlitePool) -> Vec<String> {
    sqlx::query("SELECT version FROM seaql_migrations ORDER BY version")
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>("version"))
        .collect()
}

#[tokio::test]
async fn bad_migration_rolls_back_atomically() {
    let (db, pool) = test_db().await;

    // Run all good migrations
    mokumo_db::migration::Migrator::up(&db, None).await.unwrap();

    // Record current table set
    let tables_before = get_user_tables(&pool).await;
    assert!(
        !tables_before.is_empty(),
        "Good migrations should create tables"
    );

    // --- Define a bad migration that creates a table then fails ---
    struct BadMigration;

    impl MigrationName for BadMigration {
        fn name(&self) -> &str {
            "m20260399_000000_intentional_failure"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for BadMigration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            let conn = manager.get_connection();
            // Create a table — should be rolled back if transaction works
            conn.execute_unprepared("CREATE TABLE bad_test_table (id INTEGER PRIMARY KEY)")
                .await?;
            // Then fail with invalid SQL
            conn.execute_unprepared("THIS IS INTENTIONALLY INVALID SQL")
                .await?;
            Ok(())
        }

        async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
            Ok(())
        }

        fn use_transaction(&self) -> Option<bool> {
            Some(true)
        }
    }

    // Migrator that includes good migrations + the bad one
    struct TestMigratorWithBad;

    impl MigratorTrait for TestMigratorWithBad {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            let mut m = mokumo_db::migration::Migrator::migrations();
            m.push(Box::new(BadMigration));
            m
        }
    }

    // Attempt the bad migration — should fail
    let result = TestMigratorWithBad::up(&db, None).await;
    assert!(result.is_err(), "Bad migration should fail");

    // (a) Table set identical to before the attempt
    let tables_after = get_user_tables(&pool).await;
    assert_eq!(
        tables_before, tables_after,
        "Schema should be unchanged after failed migration"
    );

    // (b) No partial DDL from multi-statement migration
    assert!(
        !tables_after.contains(&"bad_test_table".to_string()),
        "Partial DDL (bad_test_table) should be rolled back"
    );

    // (c) seaql_migrations does NOT contain the failed migration's version
    let versions = get_migration_versions(&pool).await;
    assert!(
        !versions.iter().any(|v| v.contains("intentional_failure")),
        "Failed migration should not appear in seaql_migrations"
    );

    // (d) Schema unchanged — verify original objects still exist
    let schema_objects: Vec<String> =
        sqlx::query("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY sql")
            .fetch_all(&pool)
            .await
            .unwrap()
            .iter()
            .map(|row| row.get::<String, _>("sql"))
            .collect();
    assert!(
        !schema_objects.is_empty(),
        "Schema should still have the original objects"
    );
}
