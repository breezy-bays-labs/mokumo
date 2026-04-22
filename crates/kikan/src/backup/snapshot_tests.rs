use super::*;
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

// ── build_timestamped_name ────────────────────────────────────────────

#[test]
fn timestamped_name_has_expected_format() {
    let name = build_timestamped_name();
    assert!(name.starts_with("mokumo-backup-"));
    assert!(name.ends_with(".db"));
    // Format: mokumo-backup-YYYYMMDD-HHMMSS.db
    assert_eq!(name.len(), "mokumo-backup-20260409-143022.db".len());
}

// ── create_backup ─────────────────────────────────────────────────────

#[test]
fn create_backup_produces_valid_copy() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let backup_path = tmp.path().join("backup.db");

    create_test_db(&db_path);

    let result = create_backup(&db_path, &backup_path).unwrap();
    assert_eq!(result.path, backup_path);
    assert!(result.size > 0);

    // Verify the backup has the same data
    assert_eq!(count_rows(&backup_path), 2);
}

#[test]
fn create_backup_fails_for_nonexistent_db() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("nonexistent.db");
    let backup_path = tmp.path().join("backup.db");

    let err = create_backup(&db_path, &backup_path).unwrap_err();
    assert!(matches!(err, BackupError::DatabaseNotFound { .. }));
}

// ── verify_integrity ──────────────────────────────────────────────────

#[test]
fn verify_integrity_passes_for_valid_db() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("valid.db");
    create_test_db(&db_path);

    verify_integrity(&db_path).unwrap();
}

#[test]
fn verify_integrity_fails_for_nonexistent_file() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("nonexistent.db");

    let err = verify_integrity(&path).unwrap_err();
    assert!(matches!(err, BackupError::DatabaseNotFound { .. }));
}

#[test]
fn verify_integrity_fails_for_corrupt_file() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("corrupt.db");
    // Write garbage that starts with SQLite magic but is otherwise corrupt
    let mut data = b"SQLite format 3\0".to_vec();
    data.extend_from_slice(&[0u8; 100]);
    std::fs::write(&path, &data).unwrap();

    let err = verify_integrity(&path).unwrap_err();
    // Either IntegrityCheckFailed or Sqlite error depending on how corrupt
    assert!(
        matches!(
            err,
            BackupError::IntegrityCheckFailed { .. } | BackupError::Sqlite(_)
        ),
        "expected integrity or sqlite error, got: {err}"
    );
}

// ── restore_from_backup ───────────────────────────────────────────────

#[test]
fn restore_replaces_database_with_backup_contents() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let backup_path = tmp.path().join("backup.db");

    // Create original DB with 2 rows
    create_test_db(&db_path);
    assert_eq!(count_rows(&db_path), 2);

    // Create a different backup with 3 rows
    {
        let conn = rusqlite::Connection::open(&backup_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO test (name) VALUES ('x');
             INSERT INTO test (name) VALUES ('y');
             INSERT INTO test (name) VALUES ('z');",
        )
        .unwrap();
    }

    let suffixes: &[&str] = &["", "-wal", "-shm", "-journal"];
    let result = restore_from_backup(&db_path, &backup_path, suffixes).unwrap();

    assert_eq!(result.restored_from, backup_path);
    let safety_path = result
        .safety_backup_path
        .expect("safety backup should exist");
    assert!(safety_path.exists());

    // DB should now have 3 rows (from backup)
    assert_eq!(count_rows(&db_path), 3);

    // Safety backup should have original 2 rows
    assert_eq!(count_rows(&safety_path), 2);
}

#[test]
fn restore_removes_wal_sidecars() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let wal_path = tmp.path().join("mokumo.db-wal");
    let shm_path = tmp.path().join("mokumo.db-shm");
    let backup_path = tmp.path().join("backup.db");

    create_test_db(&db_path);
    create_test_db(&backup_path);

    // Create fake sidecar files
    std::fs::write(&wal_path, b"fake-wal").unwrap();
    std::fs::write(&shm_path, b"fake-shm").unwrap();

    let suffixes: &[&str] = &["", "-wal", "-shm", "-journal"];
    restore_from_backup(&db_path, &backup_path, suffixes).unwrap();

    assert!(!wal_path.exists(), "WAL sidecar should be removed");
    assert!(!shm_path.exists(), "SHM sidecar should be removed");
}

#[test]
fn restore_fails_for_nonexistent_backup() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let backup_path = tmp.path().join("nonexistent.db");

    let suffixes: &[&str] = &["", "-wal", "-shm", "-journal"];
    let err = restore_from_backup(&db_path, &backup_path, suffixes).unwrap_err();
    assert!(matches!(err, BackupError::BackupFileNotFound { .. }));
}

#[test]
fn restore_to_empty_directory_skips_safety_backup() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    let backup_path = tmp.path().join("backup.db");

    // No existing DB — just a backup to restore from
    create_test_db(&backup_path);

    let suffixes: &[&str] = &["", "-wal", "-shm", "-journal"];
    let result = restore_from_backup(&db_path, &backup_path, suffixes).unwrap();

    assert_eq!(count_rows(&db_path), 2);
    // No existing DB means no safety backup was created
    assert!(result.safety_backup_path.is_none());
}
