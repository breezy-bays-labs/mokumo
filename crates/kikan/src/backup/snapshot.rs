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
/// Format: `mokumo-backup-{YYYYMMDD-HHMMSS}.db`. The `mokumo-` prefix is
/// part of the operator-facing filename contract — renaming it would
/// break backup-listing code that keys on the prefix.
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
#[path = "snapshot_tests.rs"]
mod tests;
