//! Database backup + restore primitives.
//!
//! - [`snapshot`] — CLI backup/restore via SQLite Online Backup API.
//! - [`restore`] — Restore-candidate validation and atomic copy into a
//!   production slot.
//! - [`pre_migration_backup`] and friends — automatic pre-migration
//!   snapshots written to `{db}.backup-v{version}` with rotation.

pub mod restore;
pub mod snapshot;

use std::path::{Path, PathBuf};

pub use restore::{CandidateInfo, RestoreError, copy_to_production, validate_candidate};
pub use snapshot::{
    BackupError, BackupResult, RestoreResult, build_timestamped_name, create_backup,
    restore_from_backup, verify_integrity,
};

/// Build the backup file path for a database backup.
///
/// Returns `{db_dir}/{db_filename}.backup-v{version}`, or `None` if the path
/// has no filename or is not valid UTF-8.
fn build_backup_path(db_path: &Path, version: &str) -> Option<PathBuf> {
    let file_name = db_path.file_name()?.to_str()?;
    Some(db_path.with_file_name(format!("{file_name}.backup-v{version}")))
}

/// Collect existing backup files for a database, sorted oldest-first by
/// version suffix.
///
/// Scans the parent directory for files matching `{db_filename}.backup-v*`.
/// Returns `(path, mtime)` pairs; mtime falls back to `UNIX_EPOCH` on
/// metadata errors.
pub async fn collect_existing_backups(
    db_path: &Path,
) -> Result<Vec<(PathBuf, std::time::SystemTime)>, Box<dyn std::error::Error + Send + Sync>> {
    let parent = db_path.parent().ok_or("Invalid database path")?;
    let file_name = db_path
        .file_name()
        .ok_or("Invalid database path")?
        .to_str()
        .ok_or("Non-UTF8 database path")?;
    let backup_prefix = format!("{}.backup-v", file_name);

    let mut backups: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    let mut entries = tokio::fs::read_dir(parent).await?;
    while let Some(entry) = entries.next_entry().await? {
        let entry_name = entry.file_name();
        match entry_name.to_str() {
            Some(name) if name.starts_with(&backup_prefix) => {
                let mtime = entry
                    .metadata()
                    .await
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                backups.push((entry.path(), mtime));
            }
            None => tracing::warn!(
                "Skipping backup candidate with non-UTF8 filename: {:?}",
                entry.path()
            ),
            _ => {}
        }
    }

    // Sort lexicographically by version suffix — migration names are
    // timestamp-prefixed (e.g. "m20260326_...") so lexicographic = chronological.
    backups.sort_by_key(|(p, _)| {
        p.file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.rsplit_once("backup-v").map(|(_, ver)| ver))
            .unwrap_or("")
            .to_string()
    });

    Ok(backups)
}

/// Delete the oldest backups, keeping only the `keep` most recent.
///
/// `backups` must be sorted oldest-first (as returned by
/// [`collect_existing_backups`]). Deletion failures are logged as warnings
/// and do not propagate as errors. Returns the number of files that failed
/// to delete.
async fn rotate_backups(backups: Vec<PathBuf>, keep: usize) -> usize {
    let to_delete = backups.len().saturating_sub(keep);
    let mut failed = 0usize;
    for path in backups.into_iter().take(to_delete) {
        match tokio::fs::remove_file(&path).await {
            Ok(()) => tracing::info!("Removed old backup {:?}", path),
            Err(e) => {
                tracing::warn!(
                    "Failed to remove old backup {:?}: {}. Manual cleanup may be needed.",
                    path,
                    e
                );
                failed += 1;
            }
        }
    }
    failed
}

/// Create a backup of the database file before running migrations.
///
/// The backup is named `{db_path}.backup-v{version}` where `version` is
/// the current schema version from the `seaql_migrations` table. Only
/// the last 3 backups are kept; older ones are deleted.
///
/// Skips silently when:
/// - The database file does not exist (first run)
/// - The `seaql_migrations` table does not exist
///
/// # Important
/// Call this BEFORE opening any SQLx pool to the same database.
pub async fn pre_migration_backup(
    db_path: &Path,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error + Send + Sync>> {
    match tokio::fs::metadata(db_path).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!("No existing database at {:?}, skipping backup", db_path);
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    }

    // Query schema version + run the backup entirely on the blocking pool.
    // rusqlite is synchronous — stalls the async executor if called directly.
    let db_path_owned = db_path.to_path_buf();
    let version: Option<String> = tokio::task::spawn_blocking(
        move || -> Result<Option<String>, rusqlite::Error> {
            let conn = rusqlite::Connection::open(&db_path_owned)?;
            let table_exists: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='seaql_migrations'",
                [],
                |row| row.get(0),
            )?;
            if !table_exists {
                return Ok(None);
            }
            // MAX(version) returns NULL for an empty table — handle as Option.
            let v: Option<String> = conn
                .query_row("SELECT MAX(version) FROM seaql_migrations", [], |row| {
                    row.get(0)
                })?;
            Ok(v)
        },
    )
    .await
    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })??;

    let Some(version) = version.filter(|s| !s.is_empty()) else {
        tracing::info!("No migrations recorded yet, skipping backup");
        return Ok(None);
    };

    let backup_path =
        build_backup_path(db_path, &version).ok_or("Invalid or non-UTF8 database path")?;

    // Use SQLite's backup API for WAL-safe copies. Copy in large batches with no
    // sleep — safe inside spawn_blocking and avoids the ~20-minute stall that a
    // small page count + 250ms sleep would cause on moderate databases.
    let backup_path_clone = backup_path.clone();
    let db_path_owned = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), rusqlite::Error> {
        let src = rusqlite::Connection::open(&db_path_owned)?;
        let mut dst = rusqlite::Connection::open(&backup_path_clone)?;
        let backup = rusqlite::backup::Backup::new(&src, &mut dst)?;
        backup.run_to_completion(1024, std::time::Duration::from_millis(0), None)?;
        Ok(())
    })
    .await
    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })??;
    tracing::info!("Created database backup at {:?}", backup_path);

    // Rotation is best-effort: a scan or deletion failure must not obscure
    // the successful backup above.
    match collect_existing_backups(db_path).await {
        Ok(backups) => {
            let paths: Vec<PathBuf> = backups.into_iter().map(|(p, _)| p).collect();
            let failed = rotate_backups(paths, 3).await;
            if failed > 0 {
                tracing::warn!(
                    "{failed} old backup(s) could not be removed from {:?}. Manual cleanup may be needed.",
                    db_path.parent().unwrap_or(db_path)
                );
            }
        }
        Err(e) => tracing::warn!(
            "Could not scan for old backups in {:?}: {}. Backup at {:?} was created successfully.",
            db_path.parent().unwrap_or(db_path),
            e,
            backup_path
        ),
    }

    Ok(Some(backup_path))
}

#[cfg(test)]
#[path = "backup_tests.rs"]
mod tests;
