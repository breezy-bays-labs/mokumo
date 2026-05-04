//! Integration tests for the `mokumo migrate status` CLI subcommand.

use tempfile::tempdir;

fn create_seaql_table(conn: &rusqlite::Connection) {
    conn.execute_batch(
        "CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);",
    )
    .unwrap();
}

fn insert_migration(conn: &rusqlite::Connection, version: &str, applied_at: i64) {
    conn.execute(
        "INSERT INTO seaql_migrations (version, applied_at) VALUES (?1, ?2)",
        rusqlite::params![version, applied_at],
    )
    .unwrap();
}

#[test]
fn migrate_status_fresh_db_all_pending() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");

    // Fresh DB: seaql_migrations table does not exist.
    rusqlite::Connection::open(&db_path).unwrap();

    let report = mokumo_shop::cli::cli_migrate_status(&db_path).unwrap();

    assert!(report.current_version.is_none(), "no version on fresh db");
    assert!(
        report.applied.is_empty(),
        "no applied migrations on fresh db"
    );
    assert!(
        !report.pending.is_empty(),
        "all known migrations should be pending"
    );
    let known = mokumo_shop::db::known_migration_names();
    assert_eq!(
        report.pending, known,
        "pending should equal all known migrations"
    );
}

#[test]
fn migrate_status_fully_migrated_no_pending() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    create_seaql_table(&conn);

    let known = mokumo_shop::db::known_migration_names();
    for name in &known {
        insert_migration(&conn, name, 1_700_000_000);
    }

    let report = mokumo_shop::cli::cli_migrate_status(&db_path).unwrap();

    assert_eq!(
        report.current_version.as_deref(),
        known.last().map(std::string::String::as_str),
        "current version should be the last applied migration"
    );
    assert_eq!(report.applied.len(), known.len(), "all migrations applied");
    assert!(report.pending.is_empty(), "no pending migrations");
}

#[test]
fn migrate_status_partial_pending() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    create_seaql_table(&conn);

    let known = mokumo_shop::db::known_migration_names();
    // Apply only the first migration.
    insert_migration(&conn, &known[0], 1_700_000_000);

    let report = mokumo_shop::cli::cli_migrate_status(&db_path).unwrap();

    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.applied[0].name, known[0]);
    assert_eq!(report.pending.len(), known.len() - 1);
    assert_eq!(
        report.pending,
        known[1..],
        "pending should be all after the first"
    );
}

#[test]
fn migrate_status_applied_at_parses_timestamp() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    create_seaql_table(&conn);

    let known = mokumo_shop::db::known_migration_names();
    // 2024-01-15 12:00:00 UTC = 1705320000
    insert_migration(&conn, &known[0], 1_705_320_000);

    let report = mokumo_shop::cli::cli_migrate_status(&db_path).unwrap();

    let applied_at = report.applied[0]
        .applied_at
        .expect("should parse timestamp");
    assert_eq!(applied_at.format("%Y-%m-%d").to_string(), "2024-01-15");
}

#[test]
fn migrate_status_invalid_path_returns_error() {
    // SQLite creates files on open, so testing a truly nonexistent path requires
    // a path whose parent directory doesn't exist (SQLite can't create parent dirs).
    let db_path = std::path::Path::new("/nonexistent/parent/dir/mokumo.db");

    let err = mokumo_shop::cli::cli_migrate_status(db_path).unwrap_err();
    assert!(
        !err.is_empty(),
        "should return a non-empty error for invalid path"
    );
}
