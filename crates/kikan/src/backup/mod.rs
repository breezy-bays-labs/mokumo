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

    // Open a raw rusqlite connection to query the current schema version.
    // Check table existence explicitly to avoid swallowing real errors.
    let version = {
        let conn = rusqlite::Connection::open(db_path)?;
        let table_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='seaql_migrations'",
            [],
            |row| row.get(0),
        )?;
        if !table_exists {
            tracing::info!("No seaql_migrations table found, skipping backup");
            return Ok(None);
        }
        let v: String = conn.query_row("SELECT MAX(version) FROM seaql_migrations", [], |row| {
            row.get(0)
        })?;
        v
        // conn dropped here
    };

    let backup_path =
        build_backup_path(db_path, &version).ok_or("Invalid or non-UTF8 database path")?;

    // Use SQLite's backup API for WAL-safe copies
    let backup_path_clone = backup_path.clone();
    let db_path_owned = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), rusqlite::Error> {
        let src = rusqlite::Connection::open(&db_path_owned)?;
        let mut dst = rusqlite::Connection::open(&backup_path_clone)?;
        let backup = rusqlite::backup::Backup::new(&src, &mut dst)?;
        backup.run_to_completion(5, std::time::Duration::from_millis(250), None)?;
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
mod tests {
    use super::*;
    use crate::db::initialize_database;

    async fn test_db() -> (sea_orm::DatabaseConnection, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
        let db = initialize_database(&url).await.unwrap();
        // Create a seaql_migrations table so pre_migration_backup has something to snapshot.
        let pool = db.get_sqlite_connection_pool();
        sqlx::query(
            "CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL)",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO seaql_migrations VALUES ('m20260321_000000_init', 0)")
            .execute(pool)
            .await
            .unwrap();
        (db, tmp)
    }

    #[tokio::test]
    async fn pre_migration_backup_skips_nonexistent_path() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonexistent.db");
        pre_migration_backup(&path).await.unwrap();
        let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
        assert!(
            entries.next_entry().await.unwrap().is_none(),
            "no files should exist after backup of missing path"
        );
    }

    #[tokio::test]
    async fn pre_migration_backup_skips_when_no_migration_table() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bare.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE foo (id INTEGER)").unwrap();
        }
        pre_migration_backup(&path).await.unwrap();
        let mut count = 0i32;
        let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
        while entries.next_entry().await.unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, 1, "only the original DB should exist — no backup");
    }

    #[tokio::test]
    async fn pre_migration_backup_creates_backup_file() {
        let (db, tmp) = test_db().await;
        let path = tmp.path().join("test.db");
        drop(db);

        pre_migration_backup(&path).await.unwrap();

        let mut backups = Vec::new();
        let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains("backup-v") {
                backups.push(name);
            }
        }
        assert_eq!(
            backups.len(),
            1,
            "exactly one backup should have been created"
        );
        assert!(
            backups[0].starts_with("test.db.backup-v"),
            "backup file should be named test.db.backup-v{{version}}"
        );
    }

    #[tokio::test]
    async fn pre_migration_backup_rotates_old_backups() {
        let (db, tmp) = test_db().await;
        let path = tmp.path().join("test.db");
        drop(db);

        // Create 3 fake older backups (sort before real migration names lexicographically)
        for i in 1..=3 {
            let fake = tmp.path().join(format!("test.db.backup-va_old{i}"));
            tokio::fs::write(&fake, b"fake").await.unwrap();
        }

        pre_migration_backup(&path).await.unwrap();

        let mut backups = Vec::new();
        let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains("backup-v") {
                backups.push(name);
            }
        }
        assert_eq!(backups.len(), 3, "rotation should keep only 3 backups");
        assert!(
            !backups.iter().any(|n| n.contains("a_old1")),
            "oldest backup should have been removed"
        );
        assert!(
            backups
                .iter()
                .any(|n| n.starts_with("test.db.backup-v") && !n.contains("a_old")),
            "real backup should be retained"
        );
    }

    // ── build_backup_path ──────────────────────────────────────────────────

    #[test]
    fn build_backup_path_appends_version_suffix() {
        let path = Path::new("/tmp/mokumo.db");
        let result = build_backup_path(path, "m20260326_000000_customers").unwrap();
        assert_eq!(
            result,
            PathBuf::from("/tmp/mokumo.db.backup-vm20260326_000000_customers")
        );
    }

    #[test]
    fn build_backup_path_preserves_parent_directory() {
        let path = Path::new("/home/user/data/shop.db");
        let result = build_backup_path(path, "m20260101_000000_init").unwrap();
        assert_eq!(
            result.file_name().unwrap().to_str().unwrap(),
            "shop.db.backup-vm20260101_000000_init"
        );
        assert_eq!(
            result.parent().unwrap().to_str().unwrap(),
            "/home/user/data"
        );
    }

    // ── collect_existing_backups ───────────────────────────────────────────

    #[tokio::test]
    async fn collect_existing_backups_empty_when_none_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("fresh.db");
        tokio::fs::write(&db_path, b"dummy").await.unwrap();
        let backups = collect_existing_backups(&db_path).await.unwrap();
        assert!(backups.is_empty(), "no backup files should be found");
    }

    #[tokio::test]
    async fn collect_existing_backups_finds_matching_files_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("mokumo.db");
        tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260326_z"), b"b3")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260322_a"), b"b1")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260324_m"), b"b2")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("other.db.backup-vm20260322_a"), b"ignore")
            .await
            .unwrap();
        let backups = collect_existing_backups(&db_path).await.unwrap();
        assert_eq!(backups.len(), 3);
        let names: Vec<String> = backups
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert!(names[0].contains("20260322_a"), "oldest first: {names:?}");
        assert!(names[1].contains("20260324_m"), "middle: {names:?}");
        assert!(names[2].contains("20260326_z"), "newest last: {names:?}");
    }

    // ── rotate_backups ────────────────────────────────────────────────────

    #[tokio::test]
    async fn rotate_backups_keeps_all_when_within_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let files: Vec<_> = (1..=3)
            .map(|i| tmp.path().join(format!("backup_{i}")))
            .collect();
        for f in &files {
            tokio::fs::write(f, b"x").await.unwrap();
        }
        rotate_backups(files, 3).await;
        let mut count = 0i32;
        let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
        while entries.next_entry().await.unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, 3, "all backups should be retained when within limit");
    }

    #[tokio::test]
    async fn rotate_backups_deletes_oldest_when_over_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let files: Vec<_> = ["backup_a", "backup_b", "backup_c", "backup_d"]
            .iter()
            .map(|name| tmp.path().join(name))
            .collect();
        for f in &files {
            tokio::fs::write(f, b"x").await.unwrap();
        }
        rotate_backups(files, 3).await;
        assert!(
            !tmp.path().join("backup_a").exists(),
            "oldest backup should be deleted"
        );
        assert!(tmp.path().join("backup_b").exists());
        assert!(tmp.path().join("backup_c").exists());
        assert!(tmp.path().join("backup_d").exists());
    }

    // ── collect_existing_backups over-match guard ─────────────────────────

    #[tokio::test]
    async fn collect_existing_backups_excludes_over_matched_names() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("mokumo.db");
        tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260322_a"), b"ok")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("mokumo.db.foo.backup-vm20260322"), b"no")
            .await
            .unwrap();
        let backups = collect_existing_backups(&db_path).await.unwrap();
        assert_eq!(
            backups.len(),
            1,
            "only exact-prefix backup should match: {backups:?}"
        );
        assert!(
            backups[0]
                .0
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("mokumo.db.backup-v"),
        );
    }
}
