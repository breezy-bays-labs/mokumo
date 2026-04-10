/// Integration tests for startup safety guards (#308 + #309 + PRAGMA scope).
///
/// Tests cover:
/// - check_application_id: valid (0), valid (MKMO), invalid (wrong non-zero)
/// - check_schema_compatibility: fresh DB, known migrations, unknown migration,
///   empty seaql_migrations
/// - initialize_database: DbErr::Custom interception (defense-in-depth regression)
/// - PRAGMA user_version stamped correctly after full migration run
/// - PRAGMA application_id stamped correctly after full migration run
/// - All migrations return use_transaction() == Some(true)
use mokumo_db::{
    check_application_id, check_schema_compatibility, ensure_auto_vacuum, initialize_database,
};
use sea_orm_migration::MigratorTrait as _;

// ─── check_application_id ───────────────────────���────────────────────────────

#[test]
fn check_application_id_passes_for_zero() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // Create a plain SQLite file — application_id defaults to 0
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute("CREATE TABLE dummy (id INTEGER PRIMARY KEY)", [])
        .unwrap();
    drop(conn);

    assert!(
        check_application_id(&db_path).is_ok(),
        "application_id = 0 should be valid (not-yet-stamped)"
    );
}

#[test]
fn check_application_id_passes_for_mkmo() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    // 0x4D4B4D4F = 1296780623 ("MKMO" in big-endian ASCII)
    conn.execute_batch("PRAGMA application_id = 1296780623")
        .unwrap();
    drop(conn);

    assert!(
        check_application_id(&db_path).is_ok(),
        "application_id = 0x4D4B4D4F should be valid"
    );
}

#[test]
fn check_application_id_fails_for_wrong_id() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch("PRAGMA application_id = 999999")
        .unwrap();
    drop(conn);

    let err = check_application_id(&db_path).unwrap_err();
    assert!(
        matches!(err, mokumo_db::DatabaseSetupError::NotMokumoDatabase { .. }),
        "Expected NotMokumoDatabase, got: {err:?}"
    );
}

// ─── check_schema_compatibility ──────────────────────────────────────────────

#[test]
fn check_schema_compatibility_passes_fresh_db() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("fresh.db");

    // DB doesn't exist yet — should pass immediately
    assert!(
        check_schema_compatibility(&db_path).is_ok(),
        "Non-existent DB should pass compatibility check"
    );
}

#[test]
fn check_schema_compatibility_passes_no_migrations_table() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute("CREATE TABLE dummy (id INTEGER PRIMARY KEY)", [])
        .unwrap();
    drop(conn);

    assert!(
        check_schema_compatibility(&db_path).is_ok(),
        "DB with no seaql_migrations table should pass"
    );
}

#[tokio::test]
async fn check_schema_compatibility_passes_known_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // Initialize fully — all migrations applied
    let db = initialize_database(&url).await.unwrap();
    drop(db);

    assert!(
        check_schema_compatibility(&db_path).is_ok(),
        "Fully migrated DB should pass compatibility check"
    );
}

#[tokio::test]
async fn check_schema_compatibility_fails_unknown_migration() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // Initialize fully, then inject a fake future migration
    let db = initialize_database(&url).await.unwrap();
    drop(db);

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "INSERT INTO seaql_migrations (version, applied_at) VALUES (?1, ?2)",
        rusqlite::params!["m20991231_000000_future_feature", 9_999_999_999_i64],
    )
    .unwrap();
    drop(conn);

    let err = check_schema_compatibility(&db_path).unwrap_err();
    match err {
        mokumo_db::DatabaseSetupError::SchemaIncompatible {
            unknown_migrations, ..
        } => {
            assert!(
                unknown_migrations.contains(&"m20991231_000000_future_feature".to_string()),
                "Expected unknown migration in list, got: {unknown_migrations:?}"
            );
        }
        other => panic!("Expected SchemaIncompatible, got: {other:?}"),
    }
}

#[test]
fn check_schema_compatibility_passes_empty_migrations_table() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // Create seaql_migrations table but leave it empty
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE seaql_migrations (version TEXT PRIMARY KEY, applied_at INTEGER NOT NULL)",
    )
    .unwrap();
    drop(conn);

    assert!(
        check_schema_compatibility(&db_path).is_ok(),
        "Empty seaql_migrations should pass (no unknown migrations)"
    );
}

// ─── DbErr::Custom interception (defense-in-depth regression) ────────────────

/// SeaORM emits DbErr::Custom("Migration file of version '...' is missing...")
/// when the DB has migrations the binary doesn't know. initialize_database must
/// intercept this and return SchemaIncompatible (not Migration).
///
/// We simulate this by inserting a fake applied migration into seaql_migrations
/// before calling initialize_database (bypassing check_schema_compatibility).
#[tokio::test]
async fn initialize_database_intercepts_dberr_custom_for_downgrade() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // Initialize the DB first so seaql_migrations exists with all known migrations
    let db = initialize_database(&url).await.unwrap();
    drop(db);

    // Inject a fake future migration directly into seaql_migrations (bypassing the guard)
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO seaql_migrations (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params!["m20991231_000000_regression_test", 9_999_999_999_i64],
        )
        .unwrap();
        drop(conn);
    }

    // Now call initialize_database directly (no guard) — SeaORM should emit DbErr::Custom
    // and we should intercept it as SchemaIncompatible
    let err = initialize_database(&url).await.unwrap_err();
    assert!(
        matches!(
            err,
            mokumo_db::DatabaseSetupError::SchemaIncompatible { .. }
        ),
        "Expected SchemaIncompatible from DbErr::Custom interception, got: {err:?}"
    );
}

// ─── PRAGMA stamps after migration ───────────────────────────────────────────

#[tokio::test]
async fn user_version_is_7_after_full_migration() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = initialize_database(&url).await.unwrap();
    drop(db);

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    drop(conn);

    assert_eq!(
        user_version, 7,
        "user_version should be 7 after all migrations run"
    );
}

#[tokio::test]
async fn application_id_is_mkmo_after_full_migration() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = initialize_database(&url).await.unwrap();
    drop(db);

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let app_id: i64 = conn
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .unwrap();
    drop(conn);

    assert_eq!(
        app_id, 0x4D4B4D4F,
        "application_id should be 0x4D4B4D4F (1296780623) after set_pragmas migration"
    );
}

// ─── Space-safe path handling (#134) ─────────────────────────────────────────

#[tokio::test]
async fn initialize_database_works_with_spaces_in_path() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("Application Support").join("mokumo.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let result = initialize_database(&url).await;
    assert!(
        result.is_ok(),
        "initialize_database must succeed when path contains spaces: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn migrations_run_successfully_with_spaces_in_path() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("Application Support").join("mokumo.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = initialize_database(&url).await.unwrap();
    // If initialize_database succeeded, migrations ran. Confirm by querying the migrations table.
    use sea_orm::ConnectionTrait as _;
    let result = db
        .execute_unprepared("SELECT COUNT(*) FROM seaql_migrations")
        .await;
    assert!(
        result.is_ok(),
        "seaql_migrations table must exist after migrations with spaces in path: {:?}",
        result.err()
    );
}

// ─── Migration quality assertions ──────────────────────────────��─────────────

// ─── ensure_auto_vacuum ──────────────────────────────────────────────────��──

#[test]
fn ensure_auto_vacuum_creates_new_db_with_incremental() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("new.db");

    ensure_auto_vacuum(&db_path).unwrap();

    assert!(db_path.exists(), "file should be created for new database");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let av: i32 = conn
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();
    assert_eq!(av, 2, "new database should have auto_vacuum=INCREMENTAL");
}

#[test]
fn ensure_auto_vacuum_enables_on_existing_db() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // Create a database with auto_vacuum = NONE (default) and insert data
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE dummy (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        conn.execute(
            "INSERT INTO dummy (id, name) VALUES (1, 'survive_vacuum')",
            [],
        )
        .unwrap();
        let av: i32 = conn
            .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
            .unwrap();
        assert_eq!(av, 0, "precondition: auto_vacuum should be NONE");
    }

    ensure_auto_vacuum(&db_path).unwrap();

    // Verify auto_vacuum is now INCREMENTAL and data survived the VACUUM
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let av: i32 = conn
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();
    assert_eq!(av, 2, "auto_vacuum should be INCREMENTAL after guard");

    let name: String = conn
        .query_row("SELECT name FROM dummy WHERE id = 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        name, "survive_vacuum",
        "data must survive VACUUM during auto_vacuum upgrade"
    );
}

#[test]
fn ensure_auto_vacuum_noop_when_already_incremental() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // Create a database with auto_vacuum = INCREMENTAL from the start
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL")
            .unwrap();
        conn.execute_batch("CREATE TABLE dummy (id INTEGER PRIMARY KEY)")
            .unwrap();
    }

    assert!(
        ensure_auto_vacuum(&db_path).is_ok(),
        "Should succeed without error when already INCREMENTAL"
    );

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let av: i32 = conn
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();
    assert_eq!(av, 2, "auto_vacuum should remain INCREMENTAL");
}

#[test]
fn ensure_auto_vacuum_noop_when_full() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // Create a database with auto_vacuum = FULL
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("PRAGMA auto_vacuum = FULL").unwrap();
        conn.execute_batch("CREATE TABLE dummy (id INTEGER PRIMARY KEY)")
            .unwrap();
    }

    assert!(
        ensure_auto_vacuum(&db_path).is_ok(),
        "Should succeed without error when FULL"
    );

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let av: i32 = conn
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();
    assert_eq!(av, 1, "auto_vacuum should remain FULL (no VACUUM needed)");
}

#[test]
fn ensure_auto_vacuum_fails_on_corrupt_db() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("corrupt.db");

    // Write garbage to simulate a corrupt database file
    std::fs::write(&db_path, b"this is not a sqlite database").unwrap();

    let result = ensure_auto_vacuum(&db_path);
    assert!(result.is_err(), "Corrupt database should return an error");
}

#[cfg(unix)]
#[test]
fn ensure_auto_vacuum_fails_on_read_only_db() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("readonly.db");

    // Create a valid database, then make it read-only
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE dummy (id INTEGER PRIMARY KEY)")
            .unwrap();
    }
    std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o444)).unwrap();

    let result = ensure_auto_vacuum(&db_path);
    assert!(
        result.is_err(),
        "Read-only database should return an error (cannot open for write)"
    );

    // Restore permissions for cleanup
    std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

// ─── Migration quality assertions ────────────────────────────────────────────

#[test]
fn all_migrations_use_transaction_returns_some_true() {
    for migration in mokumo_db::migration::Migrator::migrations() {
        assert_eq!(
            migration.use_transaction(),
            Some(true),
            "Migration '{}' must return Some(true) from use_transaction() (non-transactional \
             migrations are prohibited — see CLAUDE.md §15)",
            migration.name()
        );
    }
}
