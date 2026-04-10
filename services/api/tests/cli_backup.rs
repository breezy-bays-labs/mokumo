//! Integration tests for the `mokumo-api backup` CLI subcommand.

use std::path::Path;
use tempfile::tempdir;

/// Helper: create a minimal valid SQLite database at the given path.
fn create_test_db(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT);
         INSERT INTO test (name) VALUES ('alice');",
    )
    .unwrap();
}

#[test]
fn backup_creates_timestamped_file() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    mokumo_api::ensure_data_dirs(data_dir).unwrap();

    let db_path = data_dir.join("demo").join("mokumo.db");
    create_test_db(&db_path);

    let result = mokumo_api::cli_backup(&db_path, None).unwrap();
    assert!(result.path.exists());
    assert!(result.size > 0);

    let name = result.path.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("mokumo-backup-"));
    assert!(name.ends_with(".db"));
}

#[test]
fn backup_with_custom_output_path() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    mokumo_api::ensure_data_dirs(data_dir).unwrap();

    let db_path = data_dir.join("demo").join("mokumo.db");
    let output_path = tmp.path().join("my-backup.db");
    create_test_db(&db_path);

    let result = mokumo_api::cli_backup(&db_path, Some(&output_path)).unwrap();
    assert_eq!(result.path, output_path);
    assert!(output_path.exists());
}

#[test]
fn backup_fails_for_nonexistent_db() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("nonexistent.db");

    let err = mokumo_api::cli_backup(&db_path, None).unwrap_err();
    assert!(
        err.contains("not found"),
        "expected 'not found', got: {err}"
    );
}
