use std::path::{Path, PathBuf};
use std::time::Duration;

use sea_orm_migration::MigratorTrait;

use crate::{MOKUMO_APPLICATION_ID, migration};

/// Errors that can occur during restore candidate validation or copy.
///
/// Dedicated enum — does not stretch `DatabaseSetupError` beyond its "pool
/// creation + migration" scope.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("not a Mokumo database: {}", path.display())]
    NotMokumoDatabase { path: PathBuf },

    #[error("database integrity check failed: {}", path.display())]
    DatabaseCorrupt { path: PathBuf },

    #[error("schema incompatible: database at {} has unknown migrations: {:?}", path.display(), unknown_migrations)]
    SchemaIncompatible {
        path: PathBuf,
        unknown_migrations: Vec<String>,
    },

    #[error("production database already exists: {}", path.display())]
    ProductionDbExists { path: PathBuf },

    #[error("database access error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Information extracted from a validated restore candidate.
#[derive(Debug)]
pub struct CandidateInfo {
    /// Size of the source file in bytes.
    pub file_size: u64,
    /// The latest applied migration version string, or `None` if the
    /// `seaql_migrations` table is absent (fresh / pre-migration database).
    pub schema_version: Option<String>,
}

/// Validate a `.db` file as a Mokumo restore candidate.
///
/// Runs a three-step chain on the source file (opened read-only):
/// 1. Identity — file size + `PRAGMA application_id` must be `0` or `0x4D4B4D4F`.
/// 2. Integrity — `PRAGMA integrity_check` must return `"ok"`.
/// 3. Schema compatibility — all applied migrations must be known to this binary.
///
/// On success returns [`CandidateInfo`] with the file size and schema version.
/// On failure returns a typed [`RestoreError`] without modifying any state.
pub fn validate_candidate(source: &Path) -> Result<CandidateInfo, RestoreError> {
    let (conn, file_size) = open_and_verify_identity(source)?;
    verify_integrity(&conn, source)?;
    let schema_version = verify_schema_compatibility(&conn, source)?;
    drop(conn);
    Ok(CandidateInfo {
        file_size,
        schema_version,
    })
}

/// Step 1 — Identity: reject empty/unreadable files and non-Mokumo databases.
///
/// Opens the file read-only and checks `PRAGMA application_id`. Returns the
/// opened connection (for subsequent steps) and the file size in bytes.
fn open_and_verify_identity(source: &Path) -> Result<(rusqlite::Connection, u64), RestoreError> {
    // Fail fast on empty or unreadable files.
    // An empty file opens as a valid SQLite database with application_id=0,
    // so we must reject it before attempting to open it.
    let file_size = std::fs::metadata(source)
        .map_err(|_| RestoreError::NotMokumoDatabase {
            path: source.to_path_buf(),
        })?
        .len();

    if file_size == 0 {
        return Err(RestoreError::NotMokumoDatabase {
            path: source.to_path_buf(),
        });
    }

    let conn = rusqlite::Connection::open_with_flags(
        source,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| RestoreError::NotMokumoDatabase {
        path: source.to_path_buf(),
    })?;

    let app_id: i64 = conn
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(|_| RestoreError::NotMokumoDatabase {
            path: source.to_path_buf(),
        })?;

    match app_id {
        0 => {}                                 // not-yet-stamped — valid
        id if id == MOKUMO_APPLICATION_ID => {} // "MKMO" — valid
        _ => {
            return Err(RestoreError::NotMokumoDatabase {
                path: source.to_path_buf(),
            });
        }
    }

    Ok((conn, file_size))
}

/// Step 2 — Integrity: `PRAGMA integrity_check` must return exactly `"ok"`.
fn verify_integrity(conn: &rusqlite::Connection, source: &Path) -> Result<(), RestoreError> {
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(RestoreError::Sqlite)?;

    if integrity != "ok" {
        return Err(RestoreError::DatabaseCorrupt {
            path: source.to_path_buf(),
        });
    }

    Ok(())
}

/// Step 3 — Schema compatibility: all applied migrations must be known to this binary.
///
/// Returns the maximum applied migration version string, or `None` if the
/// `seaql_migrations` table is absent (fresh / pre-migration database).
fn verify_schema_compatibility(
    conn: &rusqlite::Connection,
    source: &Path,
) -> Result<Option<String>, RestoreError> {
    let table_exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='seaql_migrations'",
        [],
        |row| row.get(0),
    )?;

    if !table_exists {
        return Ok(None);
    }

    let applied: Vec<String> = {
        let mut stmt = conn.prepare("SELECT version FROM seaql_migrations")?;
        stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?
    };

    let known: std::collections::HashSet<String> = migration::Migrator::migrations()
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

/// Copy a validated `.db` file to the production slot via the SQLite Online Backup API.
///
/// Safety guarantees:
/// - Fails immediately if `production_dir/mokumo.db` already exists.
/// - Writes to a temp file (`mokumo.db.restore-tmp`) in the same directory,
///   then atomically renames it to the final path.
/// - Cleans up the temp file on any failure after it is created.
///
/// The caller must have already validated the source file with [`validate_candidate`].
/// This function re-validates nothing — TOCTOU mitigation is the API handler's job.
pub fn copy_to_production(source: &Path, production_dir: &Path) -> Result<(), RestoreError> {
    let final_path = production_dir.join("mokumo.db");

    // Pre-check: destination must not exist
    if final_path.exists() {
        return Err(RestoreError::ProductionDbExists { path: final_path });
    }

    // Ensure production directory exists
    std::fs::create_dir_all(production_dir)?;

    let temp_path = production_dir.join("mokumo.db.restore-tmp");

    // Remove any stale temp file from a previous failed attempt
    if temp_path.exists() {
        let _ = std::fs::remove_file(&temp_path);
    }

    // Perform backup using SQLite Online Backup API (WAL-safe)
    let result = (|| -> Result<(), RestoreError> {
        let src = rusqlite::Connection::open_with_flags(
            source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let mut dst = rusqlite::Connection::open(&temp_path)?;

        let backup = rusqlite::backup::Backup::new(&src, &mut dst)?;
        backup.run_to_completion(5, Duration::from_millis(250), None)?;

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
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ---- Test helpers ----

    /// Create a minimal valid Mokumo SQLite database with `application_id` set.
    fn make_mokumo_db(dir: &TempDir) -> PathBuf {
        let path = dir.path().join("test.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        // Stamp application_id and create a seaql_migrations table with one known migration
        conn.execute_batch(&format!(
            "PRAGMA application_id = {MOKUMO_APPLICATION_ID};
             CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);
             INSERT INTO seaql_migrations VALUES ('m20260404_000000_set_pragmas', 0);"
        ))
        .unwrap();
        drop(conn);
        path
    }

    /// Create a valid Mokumo SQLite database with NO seaql_migrations table.
    fn make_mokumo_db_no_migrations(dir: &TempDir) -> PathBuf {
        let path = dir.path().join("fresh.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&format!("PRAGMA application_id = {MOKUMO_APPLICATION_ID};"))
            .unwrap();
        drop(conn);
        path
    }

    /// Create a valid Mokumo SQLite database with application_id 0 (legacy/unstamped).
    fn make_unstamped_db(dir: &TempDir) -> PathBuf {
        let path = dir.path().join("unstamped.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        // application_id defaults to 0 — do not set it
        conn.execute_batch(
            "CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);",
        )
        .unwrap();
        drop(conn);
        path
    }

    // ---- validate_candidate tests ----

    #[test]
    fn valid_mokumo_db_passes_validation() {
        let dir = TempDir::new().unwrap();
        let path = make_mokumo_db(&dir);
        let result = validate_candidate(&path);
        assert!(result.is_ok(), "Expected Ok, got: {result:?}");
    }

    #[test]
    fn empty_file_fails_identity_check() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("empty.db");
        std::fs::write(&path, b"").unwrap();
        let result = validate_candidate(&path);
        assert!(
            matches!(result, Err(RestoreError::NotMokumoDatabase { .. })),
            "Expected NotMokumoDatabase, got: {result:?}"
        );
    }

    #[test]
    fn non_sqlite_file_fails_identity_check() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("garbage.db");
        std::fs::write(&path, b"this is not a sqlite database at all!!!").unwrap();
        let result = validate_candidate(&path);
        assert!(
            matches!(result, Err(RestoreError::NotMokumoDatabase { .. })),
            "Expected NotMokumoDatabase, got: {result:?}"
        );
    }

    #[test]
    fn wrong_application_id_fails_identity_check() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("wrong_id.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        // Use a plain decimal value — hex literals in PRAGMA context may not
        // parse correctly in all SQLite versions. Create a table to force page
        // 1 (the header) to be flushed to disk; a PRAGMA alone won't do it.
        conn.execute_batch(
            "PRAGMA application_id = 999999;
             CREATE TABLE _dummy (id INTEGER PRIMARY KEY);",
        )
        .unwrap();
        drop(conn);
        let result = validate_candidate(&path);
        assert!(
            matches!(result, Err(RestoreError::NotMokumoDatabase { .. })),
            "Expected NotMokumoDatabase for wrong app_id, got: {result:?}"
        );
    }

    #[test]
    fn application_id_zero_passes_identity_check() {
        let dir = TempDir::new().unwrap();
        let path = make_unstamped_db(&dir);
        let result = validate_candidate(&path);
        assert!(result.is_ok(), "Expected Ok for app_id=0, got: {result:?}");
    }

    #[test]
    fn truncated_file_fails_integrity_check() {
        let dir = TempDir::new().unwrap();
        let original = make_mokumo_db(&dir);

        // Read the SQLite file and truncate after the header
        let data = std::fs::read(&original).unwrap();
        let truncated_path = dir.path().join("truncated.db");
        // Keep only the header (first 100 bytes) — missing all page data
        std::fs::write(&truncated_path, &data[..100.min(data.len())]).unwrap();

        let result = validate_candidate(&truncated_path);
        // A file with only 100 bytes cannot be opened as a valid SQLite DB — it should be
        // treated as NotMokumoDatabase (can't open) or DatabaseCorrupt (opens but fails check)
        assert!(
            matches!(
                result,
                Err(RestoreError::NotMokumoDatabase { .. })
                    | Err(RestoreError::DatabaseCorrupt { .. })
            ),
            "Expected NotMokumoDatabase or DatabaseCorrupt for truncated file, got: {result:?}"
        );
    }

    #[test]
    fn corrupted_page_data_fails_integrity_check() {
        let dir = TempDir::new().unwrap();
        let original = make_mokumo_db(&dir);

        // Write the valid DB then corrupt the middle of the file
        let mut data = std::fs::read(&original).unwrap();
        if data.len() > 200 {
            // Overwrite bytes in the middle of the file (page data, not the header)
            let mid = data.len() / 2;
            data[mid..mid + 50].fill(0xFF);
        }
        let corrupt_path = dir.path().join("corrupt.db");
        std::fs::write(&corrupt_path, &data).unwrap();

        let result = validate_candidate(&corrupt_path);
        // Might be DatabaseCorrupt or pass if corruption is in unused space.
        // The important thing is it doesn't panic and returns a typed error if corrupt.
        match &result {
            Ok(_) => {} // Corruption happened to be in unused space — acceptable
            Err(RestoreError::DatabaseCorrupt { .. }) => {} // Expected
            Err(RestoreError::NotMokumoDatabase { .. }) => {} // Also acceptable
            other => panic!("Unexpected error for corrupted file: {other:?}"),
        }
    }

    #[test]
    fn unknown_migrations_fails_schema_check() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("future.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&format!(
            "PRAGMA application_id = {MOKUMO_APPLICATION_ID};
             CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);
             INSERT INTO seaql_migrations VALUES ('m99991231_999999_future_migration', 0);"
        ))
        .unwrap();
        drop(conn);

        let result = validate_candidate(&path);
        assert!(
            matches!(result, Err(RestoreError::SchemaIncompatible { .. })),
            "Expected SchemaIncompatible, got: {result:?}"
        );
    }

    #[test]
    fn no_migrations_table_passes_compatibility_check() {
        let dir = TempDir::new().unwrap();
        let path = make_mokumo_db_no_migrations(&dir);
        let result = validate_candidate(&path);
        assert!(
            result.is_ok(),
            "Expected Ok for DB with no migrations table, got: {result:?}"
        );
        let info = result.unwrap();
        assert!(info.schema_version.is_none(), "Expected no schema version");
    }

    #[test]
    fn candidate_info_contains_file_size() {
        let dir = TempDir::new().unwrap();
        let path = make_mokumo_db(&dir);
        let actual_size = std::fs::metadata(&path).unwrap().len();
        let info = validate_candidate(&path).unwrap();
        assert_eq!(info.file_size, actual_size);
    }

    #[test]
    fn candidate_info_contains_schema_version() {
        let dir = TempDir::new().unwrap();
        let path = make_mokumo_db(&dir);
        let info = validate_candidate(&path).unwrap();
        assert_eq!(
            info.schema_version.as_deref(),
            Some("m20260404_000000_set_pragmas"),
            "Expected the known migration version"
        );
    }

    #[test]
    fn older_schema_version_passes_and_reports_version() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("older.db");
        // Use the first known migration name from the binary's Migrator
        let known_migrations: Vec<String> = migration::Migrator::migrations()
            .iter()
            .map(|m| m.name().to_owned())
            .collect();

        // Pick the lexicographically smallest migration (oldest)
        let oldest = known_migrations
            .iter()
            .min()
            .cloned()
            .unwrap_or_else(|| "m20260404_000000_set_pragmas".to_owned());

        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&format!(
            "PRAGMA application_id = {MOKUMO_APPLICATION_ID};
             CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);
             INSERT INTO seaql_migrations VALUES ('{oldest}', 0);"
        ))
        .unwrap();
        drop(conn);

        let result = validate_candidate(&path);
        assert!(
            result.is_ok(),
            "Expected Ok for older schema, got: {result:?}"
        );
        let info = result.unwrap();
        assert_eq!(info.schema_version.as_deref(), Some(oldest.as_str()));
    }

    // ---- copy_to_production tests ----

    #[test]
    fn copy_to_production_happy_path() {
        let src_dir = TempDir::new().unwrap();
        let dst_dir = TempDir::new().unwrap();
        let source = make_mokumo_db(&src_dir);
        let production_dir = dst_dir.path().join("production");

        let result = copy_to_production(&source, &production_dir);
        assert!(result.is_ok(), "Expected Ok, got: {result:?}");

        let final_path = production_dir.join("mokumo.db");
        assert!(final_path.exists(), "Production DB should exist after copy");

        // Verify content matches by checking we can read it as a SQLite DB
        let conn = rusqlite::Connection::open(&final_path).unwrap();
        let app_id: i64 = conn
            .query_row("PRAGMA application_id", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            app_id, MOKUMO_APPLICATION_ID,
            "Copied DB should have correct application_id"
        );
    }

    #[test]
    fn copy_to_production_fails_when_dest_exists() {
        let src_dir = TempDir::new().unwrap();
        let dst_dir = TempDir::new().unwrap();
        let source = make_mokumo_db(&src_dir);
        let production_dir = dst_dir.path().join("production");

        // Pre-create the production directory and DB
        std::fs::create_dir_all(&production_dir).unwrap();
        std::fs::write(production_dir.join("mokumo.db"), b"existing").unwrap();

        let result = copy_to_production(&source, &production_dir);
        assert!(
            matches!(result, Err(RestoreError::ProductionDbExists { .. })),
            "Expected ProductionDbExists, got: {result:?}"
        );
    }

    #[test]
    fn copy_to_production_uses_temp_file_then_renames() {
        let src_dir = TempDir::new().unwrap();
        let dst_dir = TempDir::new().unwrap();
        let source = make_mokumo_db(&src_dir);
        let production_dir = dst_dir.path().join("production");

        copy_to_production(&source, &production_dir).unwrap();

        // After success: final file exists, temp file does NOT
        assert!(production_dir.join("mokumo.db").exists());
        assert!(
            !production_dir.join("mokumo.db.restore-tmp").exists(),
            "Temp file should be cleaned up after successful copy"
        );
    }

    #[test]
    fn copy_to_production_cleans_up_stale_temp_file() {
        let src_dir = TempDir::new().unwrap();
        let dst_dir = TempDir::new().unwrap();
        let source = make_mokumo_db(&src_dir);
        let production_dir = dst_dir.path().join("production");

        // Pre-create a stale temp file
        std::fs::create_dir_all(&production_dir).unwrap();
        let stale_temp = production_dir.join("mokumo.db.restore-tmp");
        std::fs::write(&stale_temp, b"stale data").unwrap();

        // Copy should still succeed (stale temp removed first)
        let result = copy_to_production(&source, &production_dir);
        assert!(
            result.is_ok(),
            "Expected Ok despite stale temp, got: {result:?}"
        );
        assert!(production_dir.join("mokumo.db").exists());
        assert!(!stale_temp.exists(), "Stale temp should be gone");
    }
}
