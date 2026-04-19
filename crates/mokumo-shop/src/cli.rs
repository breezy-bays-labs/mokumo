//! Direct-path CLI helpers — database reset, backup, restore, migrate
//! status, password reset — that operate without a running daemon.
//!
//! Lifted from `mokumo-api` in PR 4b (#512). These helpers open the
//! database file directly via `rusqlite` (no pool, no migrations,
//! no HTTP) so CLI subcommands work whether or not a server is running.
//! They live in `mokumo-shop` because they touch shop-specific tables
//! (`users`, `shop_settings`) and call `mokumo_shop::db::known_migration_names`.

use std::path::{Path, PathBuf};

/// SQLite sidecar suffixes deleted alongside the main database file.
pub const DB_SIDECAR_SUFFIXES: &[&str] = &["", "-wal", "-shm", "-journal"];

/// Report from a database reset operation.
#[derive(Debug, Default)]
pub struct ResetReport {
    pub deleted: Vec<PathBuf>,
    pub not_found: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, std::io::Error)>,
    pub recovery_dir_error: Option<(PathBuf, std::io::Error)>,
    /// Non-fatal: backup directory could not be scanned (only set when `include_backups` is true).
    pub backup_dir_error: Option<(PathBuf, std::io::Error)>,
}

/// Fatal errors during database reset (not partial file failures).
#[derive(Debug, thiserror::Error)]
pub enum ResetError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A single migration record from `seaql_migrations`, with computed status.
#[derive(Debug)]
pub struct MigrationRecord {
    pub name: String,
    pub applied_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Output of `mokumo migrate status`.
#[derive(Debug)]
pub struct MigrateStatusReport {
    pub current_version: Option<String>,
    pub applied: Vec<MigrationRecord>,
    pub pending: Vec<String>,
    /// Migrations recorded in the DB but not known to this binary.
    /// Non-empty only on binary downgrade — the schema is ahead of the binary.
    pub unknown: Vec<String>,
}

/// Reset a user's password directly via SQLite (no server required).
pub fn cli_reset_password(db_path: &Path, email: &str, new_password: &str) -> Result<(), String> {
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| format!("Cannot open database at {}: {e}", db_path.display()))?;

    let hash = password_auth::generate_hash(new_password);

    let rows = conn
        .execute(
            "UPDATE users SET password_hash = ?1 WHERE email = ?2 AND deleted_at IS NULL",
            rusqlite::params![hash, email],
        )
        .map_err(|e| format!("Failed to update password: {e}"))?;

    if rows == 0 {
        return Err(format!("No active user found with email '{email}'"));
    }

    Ok(())
}

/// Delete database files, sidecars, and optionally backups + recovery files.
///
/// `profile_dir` is the directory containing `mokumo.db` for the target profile
/// (e.g. `data_dir/demo` or `data_dir/production`). The caller resolves this
/// from the `--production` flag before calling.
pub fn cli_reset_db(
    profile_dir: &Path,
    recovery_dir: &Path,
    include_backups: bool,
) -> Result<ResetReport, ResetError> {
    let mut report = ResetReport::default();

    for suffix in DB_SIDECAR_SUFFIXES {
        let path = profile_dir.join(format!("mokumo.db{suffix}"));
        delete_file(&path, &mut report);
    }

    if include_backups {
        match std::fs::read_dir(profile_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if let Some(name_str) = name.to_str()
                        && name_str.starts_with("mokumo.db.backup-v")
                    {
                        delete_file(&entry.path(), &mut report);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                report.backup_dir_error = Some((profile_dir.to_path_buf(), e));
            }
        }
    }

    match std::fs::read_dir(recovery_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if let Some(name_str) = name.to_str()
                    && name_str.starts_with("mokumo-recovery-")
                    && name_str.ends_with(".html")
                {
                    delete_file(&entry.path(), &mut report);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            report.recovery_dir_error = Some((recovery_dir.to_path_buf(), e));
        }
    }

    Ok(report)
}

/// Create a manual backup of the database using the SQLite Online Backup API.
///
/// This is safe to run while the server is running — the Online Backup API
/// handles WAL mode and concurrent access correctly.
pub fn cli_backup(
    db_path: &Path,
    output: Option<&Path>,
) -> Result<kikan::backup::BackupResult, String> {
    let output_path = match output {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = db_path.parent().unwrap_or(Path::new("."));
            dir.join(kikan::backup::build_timestamped_name())
        }
    };

    let result = kikan::backup::create_backup(db_path, &output_path).map_err(|e| format!("{e}"))?;

    kikan::backup::verify_integrity(&output_path)
        .map_err(|e| format!("Backup created but integrity check failed: {e}"))?;

    // Bundle the shop logo as a sibling file alongside the backup DB.
    // Failure is non-fatal — log a warning and continue.
    let production_dir = db_path.parent().unwrap_or(Path::new("."));
    if let Ok(conn) = rusqlite::Connection::open(&output_path)
        && let Ok(ext) = conn.query_row(
            "SELECT logo_extension FROM shop_settings WHERE id = 1 AND logo_extension IS NOT NULL",
            [],
            |row| row.get::<_, String>(0),
        )
    {
        let logo_src = production_dir.join(format!("logo.{ext}"));
        let logo_dst = output_path.with_extension(format!("logo.{ext}"));
        if let Err(e) = std::fs::copy(&logo_src, &logo_dst) {
            tracing::warn!(
                "cli_backup: could not copy logo file {:?} → {:?}: {e}",
                logo_src,
                logo_dst
            );
        }
    }

    Ok(result)
}

/// Restore the database from a backup file.
///
/// Verifies the backup's integrity, creates a safety backup of the current
/// database, then overwrites it with the backup contents. The caller must
/// hold the process lock (server must not be running).
pub fn cli_restore(
    db_path: &Path,
    backup_path: &Path,
) -> Result<kikan::backup::RestoreResult, String> {
    let result = kikan::backup::restore_from_backup(db_path, backup_path, DB_SIDECAR_SUFFIXES)
        .map_err(|e| format!("{e}"))?;

    // Sweep stale logo.* files so a changed extension doesn't leave orphans.
    let production_dir = db_path.parent().unwrap_or(Path::new("."));
    for candidate_ext in &["png", "jpeg", "webp"] {
        let stale = production_dir.join(format!("logo.{candidate_ext}"));
        if stale.exists()
            && let Err(e) = std::fs::remove_file(&stale)
        {
            tracing::warn!("cli_restore: could not remove stale logo {:?}: {e}", stale);
        }
    }
    if let Ok(conn) = rusqlite::Connection::open(backup_path)
        && let Ok(ext) = conn.query_row(
            "SELECT logo_extension FROM shop_settings WHERE id = 1 AND logo_extension IS NOT NULL",
            [],
            |row| row.get::<_, String>(0),
        )
    {
        let sibling = backup_path.with_extension(format!("logo.{ext}"));
        if sibling.exists() {
            let logo_dst = production_dir.join(format!("logo.{ext}"));
            if let Err(e) = std::fs::copy(&sibling, &logo_dst) {
                tracing::warn!(
                    "cli_restore: could not restore logo file {:?} → {:?}: {e}",
                    sibling,
                    logo_dst
                );
            }
        }
    }

    Ok(result)
}

/// Query the migration state of a database file.
///
/// Opens the database with a raw rusqlite connection (no pool, no migrations).
pub fn cli_migrate_status(db_path: &Path) -> Result<MigrateStatusReport, String> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| format!("Cannot open database at {}: {e}", db_path.display()))?;

    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='seaql_migrations'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to query sqlite_master: {e}"))?;

    if !table_exists {
        let known = crate::db::known_migration_names();
        return Ok(MigrateStatusReport {
            current_version: None,
            applied: vec![],
            pending: known,
            unknown: vec![],
        });
    }

    let mut stmt = conn
        .prepare("SELECT version, applied_at FROM seaql_migrations ORDER BY version")
        .map_err(|e| format!("Failed to prepare migration query: {e}"))?;

    let applied: Vec<MigrationRecord> = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let ts: i64 = row.get(1)?;
            Ok((name, ts))
        })
        .map_err(|e| format!("Failed to query seaql_migrations: {e}"))?
        .map(|r| {
            r.map(|(name, ts)| MigrationRecord {
                applied_at: chrono::DateTime::from_timestamp(ts, 0),
                name,
            })
        })
        .collect::<Result<_, _>>()
        .map_err(|e: rusqlite::Error| format!("Failed to read migration row: {e}"))?;

    let known = crate::db::known_migration_names();
    let known_set: std::collections::HashSet<&str> = known.iter().map(|n| n.as_str()).collect();

    let unknown: Vec<String> = applied
        .iter()
        .filter(|r| !known_set.contains(r.name.as_str()))
        .map(|r| r.name.clone())
        .collect();

    let applied_names: std::collections::HashSet<&str> =
        applied.iter().map(|r| r.name.as_str()).collect();

    let pending: Vec<String> = known
        .into_iter()
        .filter(|n| !applied_names.contains(n.as_str()))
        .collect();

    let current_version = applied.last().map(|r| r.name.clone());

    Ok(MigrateStatusReport {
        current_version,
        applied,
        pending,
        unknown,
    })
}

fn delete_file(path: &Path, report: &mut ResetReport) {
    match std::fs::remove_file(path) {
        Ok(()) => report.deleted.push(path.to_path_buf()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            report.not_found.push(path.to_path_buf());
        }
        Err(e) => report.failed.push((path.to_path_buf(), e)),
    }
}
