//! Upgrade-path and roundtrip-backup integration tests for the mokumo migrator.
//!
//! Covers two acceptance criteria:
//!
//! - **`roundtrip_backup_restores_intact`** (#307): verifies that a backup created from a
//!   fully-migrated, populated database is a valid, independently-bootable Mokumo database
//!   that passes the startup guard chain and contains the original data.
//!
//! - **`upgrade_path_preserves_data`** (#310): verifies that shop data survives a
//!   vN → vN+1 schema upgrade — specifically that `pre_migration_backup` runs cleanly
//!   and that `initialize_database` applies remaining migrations without data loss.

use kikan::backup::pre_migration_backup;
use kikan::db::check_application_id;
use mokumo_shop::db::{check_schema_compatibility, initialize_database};

// ── Shared seed helpers ────────────────────────────────────────────────────────────────────────

/// Seed approximately 10 rows into an already-migrated database via raw rusqlite.
///
/// All seeded tables (customers, users, roles, activity_log, settings) are created by
/// migrations 1–6 and therefore exist whether this is called before or after migration 7.
/// Roles (3 rows) are pre-seeded by migration 6; this function does not insert roles.
///
/// Uses rusqlite directly to avoid SeaORM connection conflicts and to keep the test
/// independent of ORM-layer evolution.
fn seed_data(db_path: &std::path::Path) {
    let conn = rusqlite::Connection::open(db_path).unwrap();

    // 3 customers (id = TEXT PRIMARY KEY, display_name = TEXT NOT NULL)
    conn.execute_batch(
        "INSERT INTO customers (id, display_name) VALUES
            ('cust-1', 'Acme Apparel'),
            ('cust-2', 'Blue Shirt Co'),
            ('cust-3', 'Delta Threads');",
    )
    .unwrap();

    // 1 user (email + name + password_hash required; role_id defaults to 1 = Admin)
    conn.execute_batch(
        "INSERT INTO users (email, name, password_hash) VALUES
            ('owner@shop.example', 'Shop Owner', '$argon2id$dummy_hash_for_test');",
    )
    .unwrap();

    // 1 activity_log entry (actor_id/actor_type default to 'system')
    conn.execute_batch(
        "INSERT INTO activity_log (entity_type, entity_id, action, payload) VALUES
            ('customer', 'cust-1', 'created', '{}');",
    )
    .unwrap();

    // 2 settings rows
    conn.execute_batch(
        "INSERT INTO settings (key, value) VALUES
            ('setup_mode', 'demo'),
            ('setup_complete', 'false');",
    )
    .unwrap();
}

/// Assert that seed rows are present and counts match expectations.
///
/// Also verifies that the 3 pre-seeded role rows from migration 6 survive.
fn assert_row_counts(db_path: &std::path::Path) {
    let conn = rusqlite::Connection::open(db_path).unwrap();

    let customers: i64 = conn
        .query_row("SELECT COUNT(*) FROM customers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(customers, 3, "Expected 3 customer rows");

    let users: i64 = conn
        .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
        .unwrap();
    assert_eq!(users, 1, "Expected 1 user row");

    // Roles are pre-seeded by migration 6 with Admin, Staff, Guest.
    let roles: i64 = conn
        .query_row("SELECT COUNT(*) FROM roles", [], |r| r.get(0))
        .unwrap();
    assert_eq!(roles, 3, "Expected 3 role rows (pre-seeded by migration 6)");

    let activity: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_log", [], |r| r.get(0))
        .unwrap();
    assert_eq!(activity, 1, "Expected 1 activity_log row");

    let settings: i64 = conn
        .query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(settings, 2, "Expected 2 settings rows");
}

// ── Tests ─────────────────────────────────────────────────────────────────────────────────────

/// Verify that a backup created from a populated database is a valid, independently-bootable
/// Mokumo database that passes the full startup guard chain and retains the original data.
///
/// Guard chain applied to the backup:
///   check_application_id → (pre_migration_backup omitted — see note) → check_schema_compatibility → initialize_database
///
/// Guard 2 (pre_migration_backup) is intentionally omitted from the chain run against the
/// backup file. The backup was created from an already-migrated database; backing it up again
/// would produce a backup-of-a-backup with no diagnostic value. The goal is to prove the
/// backup file is independently openable, not to produce another backup.
#[tokio::test]
async fn roundtrip_backup_restores_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // Step 1: Initialize with all 7 migrations.
    let db = initialize_database(&url).await.unwrap();
    drop(db);

    // Step 2: Seed data via rusqlite (after dropping the SeaORM connection).
    seed_data(&db_path);

    // Step 3: Create backup.
    let _backup_path = pre_migration_backup(&db_path)
        .await
        .expect("pre_migration_backup should succeed on a fully-migrated database");

    // Step 4: Locate the backup file.
    let backup_path = {
        let mut found = None;
        let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let name = entry.file_name().to_str().unwrap_or("").to_string();
            if name.starts_with("mokumo.db.backup-v") {
                found = Some(entry.path());
                break;
            }
        }
        found.expect("A backup file should exist after pre_migration_backup")
    };

    let backup_url = format!("sqlite:{}?mode=rwc", backup_path.display());

    // Step 5: Run the startup guard chain against the backup file.

    // Guard 1: file must be a Mokumo database (PRAGMA application_id check)
    check_application_id(&backup_path).expect("Backup should pass check_application_id");

    // Guard 2 intentionally omitted — see module-level doc comment above.

    // Guard 3: schema must be compatible with the running binary
    check_schema_compatibility(&backup_path)
        .expect("Backup should pass check_schema_compatibility");

    // Guard 4: open pool + run migrations (no-op since all migrations already applied)
    let backup_db = initialize_database(&backup_url)
        .await
        .expect("Backup should boot through initialize_database without error");
    drop(backup_db);

    // Step 6: Assert data integrity on the backup file.
    assert_row_counts(&backup_path);

    let conn = rusqlite::Connection::open(&backup_path).unwrap();
    let app_id: i64 = conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        app_id, 0x4D4B_4D4F,
        "Backup PRAGMA application_id should be 0x4D4B_4D4F (MKMO), got {app_id:#x}"
    );
}

/// Verify that shop data survives a schema upgrade: database at migration N-1 is backed up,
/// then `initialize_database` applies the remaining migration, and all rows survive intact.
///
/// Migration breakdown (7 total):
///   1. m20260321_000000_init
///   2. m20260322_000000_settings
///   3. m20260324_000000_number_sequences
///   4. m20260324_000001_customers_and_activity  ← customers + activity_log tables
///   5. m20260326_000000_customers_deleted_at_index
///   6. m20260327_000000_users_and_roles  ← roles (pre-seeded) + users tables
///   7. m20260404_000000_set_pragmas  ← sets PRAGMA application_id + user_version only
///
/// This test simulates a shop running all-but-one migrations, then upgrading to a build
/// that adds one more migration. All business data must survive the upgrade unchanged.
#[tokio::test]
async fn upgrade_path_preserves_data() {
    use mokumo_shop::migrations::Migrator;
    use sea_orm_migration::MigratorTrait;

    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // Step 1: Initialize database at schema version N-1 (all-but-one migration).
    // This simulates a database created by a previous Mokumo build. Using
    // len()-1 keeps the test correct as new migrations are added.
    // Platform migrations (users, roles, shop_settings) run first — they are
    // now owned by kikan and required before vertical migrations that ALTER
    // TABLE users (login_lockout).
    let total_migrations = Migrator::migrations().len();
    let db = sea_orm::Database::connect(&url).await.unwrap();
    kikan::migrations::platform::run_platform_migrations(&db)
        .await
        .expect("platform migrations must succeed");
    Migrator::up(&db, Some(u32::try_from(total_migrations - 1).unwrap()))
        .await
        .unwrap();
    drop(db);

    // Step 2: Seed shop data via rusqlite.
    // All tables (customers, users, roles, activity_log, settings) are created by migrations
    // 1–6, so they are present at this point. Roles are already pre-seeded by migration 6.
    seed_data(&db_path);

    // Step 3: Create pre-upgrade backup.
    let _backup_path = pre_migration_backup(&db_path)
        .await
        .expect("pre_migration_backup should succeed on a database with seaql_migrations table");

    // Step 4: Apply remaining migration (migration 7) via initialize_database.
    // initialize_database runs the full migrator — migration 7 is a no-op schema change
    // (sets PRAGMA application_id and user_version only; no data modification).
    let db = initialize_database(&url)
        .await
        .expect("initialize_database should apply migration 7 without error");
    drop(db);

    // Step 5: Assert all seed data survived the schema upgrade.
    assert_row_counts(&db_path);

    // Step 6: Assert all migration versions are recorded in seaql_migrations.
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let migration_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM seaql_migrations", [], |r| r.get(0))
        .unwrap();
    let expected = i64::try_from(total_migrations).unwrap();
    assert_eq!(
        migration_count, expected,
        "All {expected} migration versions should be recorded in seaql_migrations after upgrade"
    );
}
