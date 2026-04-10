//! CLI backup and restore operations using the SQLite Online Backup API.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Result of a successful backup operation.
#[derive(Debug)]
pub struct BackupResult {
    /// Path to the created backup file.
    pub path: PathBuf,
    /// Size of the backup file in bytes.
    pub size: u64,
}

/// Result of a successful restore operation.
#[derive(Debug)]
pub struct RestoreResult {
    /// Path of the backup file that was restored from.
    pub restored_from: PathBuf,
    /// Path to the safety backup created before overwriting, if one was made.
    /// `None` when restoring into a directory with no existing database.
    pub safety_backup_path: Option<PathBuf>,
}

/// Errors that can occur during backup or restore operations.
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("database not found: {}", path.display())]
    DatabaseNotFound { path: PathBuf },

    #[error("backup file not found: {}", path.display())]
    BackupFileNotFound { path: PathBuf },

    #[error("integrity check failed: {details}")]
    IntegrityCheckFailed { details: String },

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Build a timestamped backup filename.
///
/// Format: `mokumo-backup-{YYYYMMDD-HHMMSS}.db`
pub fn build_timestamped_name() -> String {
    let now = chrono::Local::now();
    format!("mokumo-backup-{}.db", now.format("%Y%m%d-%H%M%S"))
}

/// Create a backup of the SQLite database using the Online Backup API.
///
/// The backup API is safe to use while the database is open and in WAL mode —
/// it produces a single self-contained `.db` file with no sidecars.
pub fn create_backup(db_path: &Path, output_path: &Path) -> Result<BackupResult, BackupError> {
    if !db_path.exists() {
        return Err(BackupError::DatabaseNotFound {
            path: db_path.to_path_buf(),
        });
    }

    let src = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut dst = rusqlite::Connection::open(output_path)?;

    let backup = rusqlite::backup::Backup::new(&src, &mut dst)?;
    backup.run_to_completion(100, Duration::from_millis(50), None)?;
    drop(backup);
    drop(dst);
    drop(src);

    let size = std::fs::metadata(output_path)?.len();

    Ok(BackupResult {
        path: output_path.to_path_buf(),
        size,
    })
}

/// Verify the integrity of a SQLite database file.
///
/// Runs `PRAGMA integrity_check` and returns `Ok(())` if the database passes,
/// or `Err(BackupError::IntegrityCheckFailed)` with the error details.
pub fn verify_integrity(path: &Path) -> Result<(), BackupError> {
    if !path.exists() {
        return Err(BackupError::DatabaseNotFound {
            path: path.to_path_buf(),
        });
    }

    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    let mut stmt = conn.prepare("PRAGMA integrity_check")?;
    let rows: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;

    if rows.len() == 1 && rows[0] == "ok" {
        Ok(())
    } else {
        Err(BackupError::IntegrityCheckFailed {
            details: rows.join("; "),
        })
    }
}

/// Create a safety backup of the current database, choosing a path that
/// won't collide with the restore source.
fn create_safety_backup(
    db_path: &Path,
    restore_source: &Path,
) -> Result<Option<PathBuf>, BackupError> {
    if !db_path.exists() {
        return Ok(None);
    }

    let mut path = db_path.with_extension("db.pre-restore-backup");
    if path == restore_source {
        let now = chrono::Local::now();
        path = db_path.with_extension(format!(
            "db.pre-restore-backup.{}",
            now.format("%Y%m%d-%H%M%S")
        ));
    }
    create_backup(db_path, &path)?;
    Ok(Some(path))
}

/// Remove WAL/SHM/journal sidecars that are incompatible with the backup's
/// page state. Skips empty suffixes (the main file).
fn remove_sidecars(db_path: &Path, suffixes: &[&str]) -> Result<(), BackupError> {
    let base = db_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    for suffix in suffixes {
        if suffix.is_empty() {
            continue;
        }
        let sidecar = db_path.with_file_name(format!("{base}{suffix}"));
        if sidecar.exists() {
            std::fs::remove_file(&sidecar)?;
        }
    }
    Ok(())
}

/// Restore a database from a backup file.
///
/// 1. Verifies the backup file's integrity.
/// 2. Creates a safety backup of the current database (if it exists).
/// 3. Removes WAL/SHM sidecars left by the previous database.
/// 4. Uses the SQLite Online Backup API to copy from backup → db_path.
///
/// The caller must ensure the server is not running (process lock acquired).
pub fn restore_from_backup(
    db_path: &Path,
    backup_path: &Path,
    sidecar_suffixes: &[&str],
) -> Result<RestoreResult, BackupError> {
    if !backup_path.exists() {
        return Err(BackupError::BackupFileNotFound {
            path: backup_path.to_path_buf(),
        });
    }

    verify_integrity(backup_path)?;
    let safety_backup_path = create_safety_backup(db_path, backup_path)?;
    remove_sidecars(db_path, sidecar_suffixes)?;

    let src = rusqlite::Connection::open_with_flags(
        backup_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut dst = rusqlite::Connection::open(db_path)?;

    let backup = rusqlite::backup::Backup::new(&src, &mut dst)?;
    backup.run_to_completion(100, Duration::ZERO, None)?;
    drop(backup);
    drop(dst);
    drop(src);

    Ok(RestoreResult {
        restored_from: backup_path.to_path_buf(),
        safety_backup_path,
    })
}

#[cfg(test)]
mod tests {
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
}
