//! Integration tests for the `mokumo-server restore` CLI subcommand.

use std::path::Path;
use tempfile::tempdir;

/// Helper: create a minimal valid SQLite database at the given path.
fn create_test_db(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT);
         INSERT INTO test (name) VALUES ('alice');
         INSERT INTO test (name) VALUES ('bob');",
    )
    .unwrap();
}

/// Helper: count rows in the test table.
fn count_rows(path: &Path) -> i64 {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    conn.query_row("SELECT COUNT(*) FROM test", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn restore_replaces_database() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    mokumo_shop::startup::ensure_data_dirs(data_dir).unwrap();

    let db_path = data_dir.join("demo").join("mokumo.db");
    let backup_path = tmp.path().join("backup.db");

    // Original DB with 2 rows
    create_test_db(&db_path);
    assert_eq!(count_rows(&db_path), 2);

    // Backup with 1 row
    {
        let conn = rusqlite::Connection::open(&backup_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO test (name) VALUES ('restored');",
        )
        .unwrap();
    }

    let result = mokumo_shop::cli::cli_restore(&db_path, &backup_path).unwrap();
    assert_eq!(count_rows(&db_path), 1);
    let safety_path = result
        .safety_backup_path
        .expect("safety backup should exist");
    assert!(safety_path.exists());
    assert_eq!(count_rows(&safety_path), 2);
}

#[test]
fn restore_fails_for_nonexistent_backup() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let backup_path = tmp.path().join("nonexistent.db");

    let err = mokumo_shop::cli::cli_restore(&db_path, &backup_path).unwrap_err();
    assert!(
        err.contains("not found"),
        "expected 'not found', got: {err}"
    );
}
