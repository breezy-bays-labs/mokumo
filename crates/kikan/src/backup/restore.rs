//! Restore-candidate validation and atomic copy into a production slot.
//!
//! Generic over the vertical's `MigratorTrait` so kikan does not depend on
//! any specific migrator — callers pass their migrator as the type parameter.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sea_orm_migration::MigratorTrait;

use crate::db::KIKAN_APPLICATION_ID;

/// Errors that can occur during restore candidate validation or copy.
///
/// Dedicated enum — does not stretch `DatabaseSetupError` beyond its
/// "pool creation + migration" scope.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("not a kikan database: {}", path.display())]
    NotKikanDatabase { path: PathBuf },

    #[error("database integrity check failed: {}", path.display())]
    DatabaseCorrupt { path: PathBuf },

    #[error("schema incompatible: database at {} has unknown migrations: {:?}", path.display(), unknown_migrations)]
    SchemaIncompatible {
        path: PathBuf,
        unknown_migrations: Vec<String>,
    },

    #[error("primary profile database already exists: {}", path.display())]
    ProductionDbExists { path: PathBuf },

    #[error("database access error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Information extracted from a validated restore candidate.
#[derive(Debug)]
pub struct CandidateInfo {
    /// Size of the source file in bytes. Always non-zero (empty files are
    /// rejected during identity validation).
    pub file_size: std::num::NonZeroU64,
    /// The latest applied migration version string, or `None` if the
    /// `seaql_migrations` table is absent (fresh / pre-migration database).
    pub schema_version: Option<String>,
}

/// Validate a `.db` file as a kikan restore candidate.
///
/// Runs a three-step chain on the source file (opened read-only):
/// 1. Identity — file size + `PRAGMA application_id` must be `0` or
///    [`KIKAN_APPLICATION_ID`].
/// 2. Integrity — `PRAGMA integrity_check` must return `"ok"`.
/// 3. Schema compatibility — all applied migrations must be known to
///    the supplied migrator type `M`.
///
/// On success returns [`CandidateInfo`] with the file size and schema
/// version. On failure returns a typed [`RestoreError`] without
/// modifying any state.
pub fn validate_candidate<M: MigratorTrait>(source: &Path) -> Result<CandidateInfo, RestoreError> {
    let (conn, file_size) = open_and_verify_identity(source)?;
    verify_integrity(&conn, source)?;
    let schema_version = verify_schema_compatibility::<M>(&conn, source)?;
    drop(conn);
    Ok(CandidateInfo {
        file_size,
        schema_version,
    })
}

/// Step 1 — Identity: reject empty/unreadable files and non-kikan databases.
fn open_and_verify_identity(
    source: &Path,
) -> Result<(rusqlite::Connection, std::num::NonZeroU64), RestoreError> {
    // Fail fast on empty or unreadable files.
    // An empty file opens as a valid SQLite database with application_id=0,
    // so we must reject it before attempting to open it.
    let file_size = std::fs::metadata(source)
        .map_err(|_| RestoreError::NotKikanDatabase {
            path: source.to_path_buf(),
        })?
        .len();

    if file_size == 0 {
        return Err(RestoreError::NotKikanDatabase {
            path: source.to_path_buf(),
        });
    }
    // SAFETY: guarded by the zero-check above.
    let file_size = std::num::NonZeroU64::new(file_size).expect("file_size is non-zero");

    let conn = rusqlite::Connection::open_with_flags(
        source,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| RestoreError::NotKikanDatabase {
        path: source.to_path_buf(),
    })?;

    let app_id: i64 = conn
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(|_| RestoreError::NotKikanDatabase {
            path: source.to_path_buf(),
        })?;

    match app_id {
        0 => {}                                // not-yet-stamped — valid
        id if id == KIKAN_APPLICATION_ID => {} // "MKMO" — valid
        _ => {
            return Err(RestoreError::NotKikanDatabase {
                path: source.to_path_buf(),
            });
        }
    }

    Ok((conn, file_size))
}

/// Step 2 — Integrity: `PRAGMA integrity_check` must return exactly `"ok"`.
fn verify_integrity(conn: &rusqlite::Connection, source: &Path) -> Result<(), RestoreError> {
    let corrupt = || RestoreError::DatabaseCorrupt {
        path: source.to_path_buf(),
    };
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| corrupt())?;

    if integrity != "ok" {
        return Err(RestoreError::DatabaseCorrupt {
            path: source.to_path_buf(),
        });
    }

    Ok(())
}

/// Step 3 — Schema compatibility: all applied migrations must be known to
/// the supplied migrator.
fn verify_schema_compatibility<M: MigratorTrait>(
    conn: &rusqlite::Connection,
    source: &Path,
) -> Result<Option<String>, RestoreError> {
    let corrupt = || RestoreError::DatabaseCorrupt {
        path: source.to_path_buf(),
    };

    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='seaql_migrations'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| corrupt())?;

    if !table_exists {
        return Ok(None);
    }

    let applied: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT version FROM seaql_migrations")
            .map_err(|_| corrupt())?;
        stmt.query_map([], |row| row.get(0))
            .map_err(|_| corrupt())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| corrupt())?
    };

    let known: std::collections::HashSet<String> = M::migrations()
        .iter()
        .map(|m| m.name().to_owned())
        .collect();

    let unknown: Vec<String> = applied
        .iter()
        .filter(|v| !known.contains(*v))
        .cloned()
        .collect();

    if !unknown.is_empty() {
        return Err(RestoreError::SchemaIncompatible {
            path: source.to_path_buf(),
            unknown_migrations: unknown,
        });
    }

    // MAX version string (lexicographic, matches SeaORM's timestamp format)
    Ok(applied.into_iter().max())
}

/// Copy a validated `.db` file to the production slot via the SQLite
/// Online Backup API.
///
/// `production_filename` is the final basename written under
/// `production_dir` (the graft's `db_filename`). The temp file is named
/// `{production_filename}.restore-tmp`.
///
/// Safety guarantees:
/// - Fails immediately if `production_dir/{production_filename}` already
///   exists.
/// - Writes to a temp file in the same directory, then atomically renames
///   it to the final path.
/// - Cleans up the temp file on any failure after it is created.
///
/// The caller must have already validated the source file with
/// [`validate_candidate`]. This function re-validates nothing — TOCTOU
/// mitigation is the API handler's job.
pub fn copy_to_production(
    source: &Path,
    production_dir: &Path,
    production_filename: &str,
) -> Result<(), RestoreError> {
    let final_path = production_dir.join(production_filename);

    // Pre-check: destination must not exist
    if final_path.exists() {
        return Err(RestoreError::ProductionDbExists { path: final_path });
    }

    // Ensure production directory exists
    std::fs::create_dir_all(production_dir)?;

    let temp_path = production_dir.join(format!("{production_filename}.restore-tmp"));

    // Remove any stale temp file from a previous failed attempt
    if temp_path.exists() {
        std::fs::remove_file(&temp_path)?;
    }

    // Perform backup using SQLite Online Backup API (WAL-safe)
    let result = (|| -> Result<(), RestoreError> {
        let src = rusqlite::Connection::open_with_flags(
            source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let mut dst = rusqlite::Connection::open(&temp_path)?;

        let backup = rusqlite::backup::Backup::new(&src, &mut dst)?;
        // Use i32::MAX pages per step so the backup completes in a single step
        // with no inter-step sleep. This prevents large databases (hundreds of
        // MB) from timing out at 5 pages / 250 ms (which would take hours).
        backup.run_to_completion(i32::MAX, Duration::from_millis(250), None)?;

        drop(backup);
        drop(dst);
        drop(src);

        Ok(())
    })();

    // Clean up temp file on backup failure
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
        return result;
    }

    // Atomic rename: temp → final path (same directory guarantees same filesystem)
    std::fs::rename(&temp_path, &final_path).map_err(|e| {
        let _ = std::fs::remove_file(&temp_path);
        RestoreError::Io(e)
    })?;

    Ok(())
}

#[cfg(test)]
#[path = "restore_tests.rs"]
mod tests;
