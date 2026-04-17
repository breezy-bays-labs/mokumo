//! BDD step definitions for `migration_safety.feature`.
//!
//! These steps wire the 5 `@allow.skipped` scenarios:
//!   1. Database is backed up before schema upgrade
//!   2. Only the last three backups are kept
//!   3. No backup on first run
//!   4. A failed schema upgrade leaves the database unchanged
//!   5. Every migration runs inside a transaction

use cucumber::{given, then, when};
use sea_orm_migration::MigratorTrait as _;

use super::DbWorld;

// ── Scenario 1 & 2: Given — set up DB at version N ────────────────────────────────────────────

#[given(expr = "an existing database at schema version {int}")]
async fn given_database_at_schema_version(w: &mut DbWorld, version: u32) {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = sea_orm::Database::connect(&url).await.unwrap();
    mokumo_shop::migrations::Migrator::up(&db, Some(version))
        .await
        .unwrap();
    drop(db);

    // Record seaql_migrations count for the "same row counts" assertion
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM seaql_migrations", [], |r| r.get(0))
        .unwrap();

    w.ms_source_seaql_count = Some(count);
    w.ms_db_path = Some(db_path);
    w.ms_tmp = Some(tmp);
}

#[given(expr = "the database is at schema version {int}")]
async fn given_db_at_schema_version(w: &mut DbWorld, version: u32) {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = sea_orm::Database::connect(&url).await.unwrap();
    mokumo_shop::migrations::Migrator::up(&db, Some(version))
        .await
        .unwrap();
    drop(db);

    w.ms_db_path = Some(db_path);
    w.ms_tmp = Some(tmp);
}

/// Create fake backup files for the rotation scenario.
///
/// Uses synthetic version strings that sort lexicographically BEFORE any real migration version
/// (real migrations start with "m20260321..."), ensuring the oldest fake backup is removed by
/// rotation when the new real backup is created.
#[given(expr = "backups exist from previous upgrades to versions {int}, {int}, and {int}")]
async fn given_existing_backups(w: &mut DbWorld, v1: u32, v2: u32, v3: u32) {
    let db_path = w.ms_db_path.as_ref().unwrap().clone();
    let dir = db_path.parent().unwrap().to_path_buf();
    let db_name = db_path.file_name().unwrap().to_str().unwrap().to_string();

    // Synthetic version strings that sort before real migration names (m20260321...)
    let fake_versions = [
        format!("m20260100_000000_fake_v{v1}"),
        format!("m20260200_000000_fake_v{v2}"),
        format!("m20260300_000000_fake_v{v3}"),
    ];

    // Oldest = first in lexicographic sort order
    let oldest_name = format!("{}.backup-v{}", db_name, fake_versions[0]);
    w.ms_oldest_backup = Some(dir.join(&oldest_name));

    for fv in &fake_versions {
        let backup_name = format!("{}.backup-v{}", db_name, fv);
        tokio::fs::write(dir.join(&backup_name), b"fake backup content")
            .await
            .unwrap();
    }
}

// ── Scenario 3: Given — no database file ──────────────────────────────────────────────────────

#[given("no database file exists")]
async fn given_no_database_file(w: &mut DbWorld) {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    // db_path intentionally NOT created
    w.ms_db_path = Some(db_path);
    w.ms_tmp = Some(tmp);
}

// ── Scenario 4: Given — fully migrated DB ─────────────────────────────────────────────────────

#[given("a database with all current migrations applied")]
async fn given_fully_migrated_db(w: &mut DbWorld) {
    // DbWorld::new() already creates a fully-migrated temp DB — record table count.
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(&w.pool)
    .await
    .unwrap();
    w.ms_table_count_before = Some(count.0);
}

// ── Scenario 5: Given — migration registry ────────────────────────────────────────────────────

#[given("the migration registry")]
async fn given_migration_registry(_w: &mut DbWorld) {
    // No setup required — the migration registry is static.
}

// ── Shared When: "a schema upgrade to version N runs" ─────────────────────────────────────────

/// Shared by Scenario 1 ("upgrade to version 2") and Scenario 2 ("upgrade to version 5").
///
/// Steps:
/// 1. Run `pre_migration_backup` — this is the behaviour under test
/// 2. Locate the newest backup file in the temp dir and store it
/// 3. Apply the Nth migration via `Migrator::up(&db, Some(N))`
#[when(expr = "a schema upgrade to version {int} runs")]
async fn when_schema_upgrade_to_version(w: &mut DbWorld, version: u32) {
    let db_path = w.ms_db_path.as_ref().unwrap().clone();
    let tmp_dir = w.ms_tmp.as_ref().unwrap().path().to_path_buf();
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // Guard 2: backup before migration
    let _backup = mokumo_db::pre_migration_backup(&db_path)
        .await
        .expect("pre_migration_backup should succeed");

    // Locate the newest backup (largest version string = most recent)
    let mut backups: Vec<std::path::PathBuf> = Vec::new();
    let mut entries = tokio::fs::read_dir(&tmp_dir).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_str().unwrap_or("").to_string();
        if name.contains("backup-v") {
            backups.push(entry.path());
        }
    }
    backups.sort();
    w.ms_backup_path = backups.last().cloned();

    // Apply up-to-version-N migrations (only unapplied ones run)
    let db = sea_orm::Database::connect(&url).await.unwrap();
    mokumo_shop::migrations::Migrator::up(&db, Some(version))
        .await
        .unwrap();
    drop(db);
}

// ── Scenario 3: When — first initialization ───────────────────────────────────────────────────

#[when("the database is initialized for the first time")]
async fn when_initialized_for_first_time(w: &mut DbWorld) {
    let db_path = w.ms_db_path.as_ref().unwrap().clone();
    // pre_migration_backup returns Ok(None) silently when no file exists
    let result = mokumo_db::pre_migration_backup(&db_path)
        .await
        .expect("pre_migration_backup should succeed (skip) when DB does not exist");
    assert!(
        result.is_none(),
        "Expected Ok(None) when database does not exist"
    );
}

// ── Scenario 4: When — bad migration ──────────────────────────────────────────────────────────

#[when("a migration containing invalid SQL is applied")]
async fn when_bad_migration_applied(w: &mut DbWorld) {
    use sea_orm_migration::prelude::*;

    struct BadMigration;

    impl MigrationName for BadMigration {
        fn name(&self) -> &str {
            "m99999999_000000_bdd_intentional_failure"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for BadMigration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            let conn = manager.get_connection();
            // Create a table first — should be rolled back if transaction works
            conn.execute_unprepared("CREATE TABLE bdd_fail_table (id INTEGER PRIMARY KEY)")
                .await?;
            // Then execute intentionally invalid SQL to cause failure
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

    struct MigratorWithBad;

    impl MigratorTrait for MigratorWithBad {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            let mut migrations = mokumo_shop::migrations::Migrator::migrations();
            migrations.push(Box::new(BadMigration));
            migrations
        }
    }

    // Attempt the bad migration against the DbWorld's fully-migrated DB.
    // The good migrations (1-7) are already applied so only BadMigration runs.
    let result = MigratorWithBad::up(&w.db, None).await;
    w.ms_migration_failed = result.is_err();
}

// ── Scenario 1 Then: backup created ───────────────────────────────────────────────────────────

/// The `expected_name` argument (e.g. "mokumo.db.backup-v1") is the abstract BDD label.
/// The actual backup filename encodes the full migration version string. We verify that
/// a backup file with the standard naming prefix exists, regardless of the exact suffix.
#[then(expr = "a backup file {string} is created")]
async fn then_backup_file_created(w: &mut DbWorld, _expected_name: String) {
    let backup = w
        .ms_backup_path
        .as_ref()
        .expect("Expected ms_backup_path to be set by the When step");
    assert!(
        backup.exists(),
        "Backup file should exist on disk: {:?}",
        backup
    );
}

#[then("the backup is a valid Mokumo database that passes the startup guard chain")]
async fn then_backup_passes_guard_chain(w: &mut DbWorld) {
    let backup_path = w.ms_backup_path.as_ref().unwrap().clone();
    let backup_url = format!("sqlite:{}?mode=rwc", backup_path.display());

    // Guard 1: valid Mokumo database (PRAGMA application_id)
    mokumo_db::check_application_id(&backup_path).expect("Backup should pass check_application_id");

    // Guard 2 intentionally omitted — see upgrade_path.rs for the rationale.
    // Backing up a backup would be noise and does not test the guard chain.

    // Guard 3: schema compatible with this binary
    mokumo_shop::db::check_schema_compatibility(&backup_path)
        .expect("Backup should pass check_schema_compatibility");

    // Record seaql_migrations count from the backup BEFORE initialize_database applies
    // remaining migrations. This snapshot is used by the "same row counts" Then step to
    // verify the backup is a faithful copy of the source at the time of backup.
    {
        let conn = rusqlite::Connection::open(&backup_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM seaql_migrations", [], |r| r.get(0))
            .unwrap();
        w.ms_backup_seaql_before_upgrade = Some(count);
    }

    // Guard 4: initialize pool + apply any pending migrations
    // Note: initialize_database may apply remaining migrations to the backup — this is
    // correct behavior (it proves the backup is a valid starting point for migration).
    let db = mokumo_shop::db::initialize_database(&backup_url)
        .await
        .expect("Backup should boot through initialize_database");
    drop(db);
}

#[then("the backup contains the same row counts as the source database")]
async fn then_backup_has_same_row_counts(w: &mut DbWorld) {
    // Compare the backup's seaql_migrations count as captured BEFORE initialize_database
    // ran in the previous Then step. initialize_database applies remaining migrations which
    // would inflate the count — the pre-upgrade snapshot is the faithful comparison point.
    let source_count = w
        .ms_source_seaql_count
        .expect("ms_source_seaql_count should be set by the Given step");
    let backup_count = w
        .ms_backup_seaql_before_upgrade
        .expect("ms_backup_seaql_before_upgrade should be set by the guard-chain Then step");

    assert_eq!(
        backup_count, source_count,
        "Backup seaql_migrations count at backup time ({backup_count}) should match source ({source_count})"
    );
}

// ── Scenario 2 Then: rotation ─────────────────────────────────────────────────────────────────

#[then(expr = "a backup of version {int} is created before upgrading")]
async fn then_backup_of_version_created(w: &mut DbWorld, _version: u32) {
    let backup = w
        .ms_backup_path
        .as_ref()
        .expect("Expected ms_backup_path to be set by the When step");
    assert!(
        backup.exists(),
        "A new backup should have been created by pre_migration_backup"
    );
}

#[then("the oldest backup is removed")]
async fn then_oldest_backup_removed(w: &mut DbWorld) {
    let oldest = w
        .ms_oldest_backup
        .as_ref()
        .expect("ms_oldest_backup should be set by the 'backups exist' Given step");
    assert!(
        !oldest.exists(),
        "Oldest backup should have been removed by rotation: {:?}",
        oldest
    );
}

#[then("three backup files remain")]
async fn then_three_backups_remain(w: &mut DbWorld) {
    let db_path = w.ms_db_path.as_ref().unwrap();
    let dir = db_path.parent().unwrap();
    let db_name = db_path.file_name().unwrap().to_str().unwrap();
    let prefix = format!("{}.backup-v", db_name);

    let mut count = 0u32;
    let mut entries = tokio::fs::read_dir(dir).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_str().unwrap_or("").to_string();
        if name.starts_with(&prefix) {
            count += 1;
        }
    }
    assert_eq!(count, 3, "Expected exactly 3 backup files, found {count}");
}

// ── Scenario 3 Then: no backup on first run ───────────────────────────────────────────────────

#[then("no backup file is created")]
async fn then_no_backup_file(w: &mut DbWorld) {
    let dir = w.ms_tmp.as_ref().unwrap().path();
    let mut count = 0u32;
    let mut entries = tokio::fs::read_dir(dir).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_str().unwrap_or("").to_string();
        if name.contains("backup-v") {
            count += 1;
        }
    }
    assert_eq!(
        count, 0,
        "No backup files should be created when no database file exists"
    );
}

// ── Scenario 4 Then: failed migration atomicity ───────────────────────────────────────────────

#[then("the migration should fail")]
async fn then_migration_failed(w: &mut DbWorld) {
    assert!(
        w.ms_migration_failed,
        "Expected the bad migration to fail with an error"
    );
}

#[then("the database schema should be identical to before the attempt")]
async fn then_schema_identical(w: &mut DbWorld) {
    let expected = w
        .ms_table_count_before
        .expect("ms_table_count_before should be set by the Given step");
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(&w.pool)
    .await
    .unwrap();
    assert_eq!(
        count.0, expected,
        "Table count should be unchanged after failed migration (expected {expected}, got {})",
        count.0
    );
}

#[then("no partial changes should be visible")]
async fn then_no_partial_changes(w: &mut DbWorld) {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE name = 'bdd_fail_table'")
            .fetch_one(&w.pool)
            .await
            .unwrap();
    assert_eq!(
        count.0, 0,
        "Partial DDL (bdd_fail_table) should have been rolled back by the transaction"
    );
}

// ── Scenario 5 Then: all migrations transactional ─────────────────────────────────────────────

#[then("every registered migration should be marked as transactional")]
async fn then_all_migrations_transactional(_w: &mut DbWorld) {
    use mokumo_shop::migrations::Migrator;

    let all_ok = Migrator::migrations()
        .iter()
        .all(|m| m.use_transaction() == Some(true));

    assert!(
        all_ok,
        "Every migration must return use_transaction() == Some(true); \
         found at least one that does not"
    );
}
