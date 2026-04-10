pub mod activity;
pub mod backup;
pub mod customer;
pub mod migration;
pub mod restore;
pub mod role;
pub mod sequence;
pub mod user;

use std::future::Future;
use std::pin::Pin;

use mokumo_core::error::DomainError;
use sqlx::sqlite::{SqliteConnection, SqlitePoolOptions};

pub use sea_orm::DatabaseConnection;

/// Standard PRAGMAs applied to every SQLite connection pool in Mokumo.
///
/// WAL mode, normal synchronous, 5s busy timeout, foreign keys enforced, 64MB cache.
fn configure_sqlite_connection(
    conn: &mut SqliteConnection,
) -> Pin<Box<dyn Future<Output = Result<(), sqlx::Error>> + Send + '_>> {
    Box::pin(async move {
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&mut *conn)
            .await?;
        sqlx::query("PRAGMA synchronous=NORMAL")
            .execute(&mut *conn)
            .await?;
        sqlx::query("PRAGMA busy_timeout=5000")
            .execute(&mut *conn)
            .await?;
        sqlx::query("PRAGMA foreign_keys=ON")
            .execute(&mut *conn)
            .await?;
        sqlx::query("PRAGMA cache_size=-64000")
            .execute(&mut *conn)
            .await?;
        // 256 MB memory-mapped I/O for read performance. Per-connection PRAGMA.
        // Caveat: on Windows, mmap prevents file truncation which makes
        // incremental_vacuum unable to shrink the file. See #457.
        sqlx::query("PRAGMA mmap_size=268435456")
            .execute(&mut *conn)
            .await?;
        Ok(())
    })
}

/// Error type for database initialization (pool creation + migration).
#[derive(Debug, thiserror::Error)]
pub enum DatabaseSetupError {
    #[error("pool creation failed: {0}")]
    Pool(#[from] sqlx::Error),

    #[error("migration failed: {0}")]
    Migration(#[from] sea_orm::DbErr),

    #[error("database query failed: {0}")]
    Query(sqlx::Error),

    /// Returned when the database file does not appear to be a Mokumo database
    /// (PRAGMA application_id is non-zero and not 0x4D4B4D4F "MKMO").
    #[error("not a Mokumo database: {}", path.display())]
    NotMokumoDatabase { path: std::path::PathBuf },

    /// Returned when the database contains applied migrations not known to this
    /// binary — indicating the database was created by a newer version of Mokumo.
    #[error("schema incompatible: database at {} has unknown migrations: {:?}", path.display(), unknown_migrations)]
    SchemaIncompatible {
        path: std::path::PathBuf,
        unknown_migrations: Vec<String>,
    },

    /// Underlying rusqlite error from pre-pool guard checks.
    #[error("database access error: {0}")]
    Rusqlite(#[from] rusqlite::Error),
}

impl DatabaseSetupError {
    /// Construct a [`SchemaIncompatible`][Self::SchemaIncompatible] error.
    ///
    /// # Panics (debug builds only)
    /// Panics if `unknown_migrations` is empty — an incompatibility without any
    /// unknown migrations is a bug in the caller.
    pub(crate) fn schema_incompatible(
        path: std::path::PathBuf,
        unknown_migrations: Vec<String>,
    ) -> Self {
        debug_assert!(
            !unknown_migrations.is_empty(),
            "SchemaIncompatible requires at least one unknown migration"
        );
        Self::SchemaIncompatible {
            path,
            unknown_migrations,
        }
    }
}

/// Convert a sqlx error into a DomainError::Internal.
/// Shared across all repository implementations.
pub(crate) fn db_err(e: sqlx::Error) -> DomainError {
    DomainError::Internal {
        message: e.to_string(),
    }
}

/// Convert a SeaORM error into a DomainError::Internal.
/// Analogous to `db_err()` for sqlx errors. Used via `map_err(sea_err)`.
pub(crate) fn sea_err(e: sea_orm::DbErr) -> DomainError {
    DomainError::Internal {
        message: e.to_string(),
    }
}

/// PRAGMA application_id value that identifies a Mokumo database.
/// `"MKMO"` encoded as a big-endian 32-bit integer (0x4D4B4D4F = 1296780623).
///
/// Valid states at startup: `0` (not-yet-stamped, legacy installs before
/// `m20260404_000000_set_pragmas` ran) or this value.
/// Any other non-zero value → `check_application_id` returns `NotMokumoDatabase`.
pub const MOKUMO_APPLICATION_ID: i64 = 0x4D4B4D4F;

/// SeaORM emits this message when a DB has migrations the binary doesn't know about
/// (downgrade scenario). Intercepted as defense-in-depth after `check_schema_compatibility`.
///
/// Validated against `sea-orm-migration = "=2.0.0-rc.37"`. The test
/// `initialize_database_intercepts_dberr_custom_for_downgrade` in
/// `crates/db/tests/startup_guards.rs` covers this sentinel — if SeaORM changes
/// this message format in a future version, that test will catch it.
const DBERRCOMPAT_PATTERN: &str = "Migration file of version";

/// Create a SQLite connection pool with WAL mode and run SeaORM migrations.
///
/// Pool-first wrapping: create `SqlitePool` with PRAGMA hooks, then wrap
/// via `SqlxSqliteConnector::from_sqlx_sqlite_pool` for `DatabaseConnection`.
///
/// The `database_url` should include `?mode=rwc` if the file may not exist yet.
pub async fn initialize_database(
    database_url: &str,
) -> Result<DatabaseConnection, DatabaseSetupError> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .after_connect(|conn, _meta| configure_sqlite_connection(conn))
        .connect(database_url)
        .await?;

    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);

    use sea_orm_migration::MigratorTrait;
    match migration::Migrator::up(&db, None).await {
        Ok(()) => {}
        Err(sea_orm::DbErr::Custom(ref msg)) if msg.contains(DBERRCOMPAT_PATTERN) => {
            // SeaORM detected a downgrade: the DB has a migration the binary doesn't know.
            // Re-surface as SchemaIncompatible so callers can produce a human-readable message.
            // Strip "sqlite:" prefix and "?..." query suffix to recover the actual file path.
            let path = {
                let stripped = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
                let path_str = stripped.split('?').next().unwrap_or(stripped);
                std::path::PathBuf::from(path_str)
            };
            return Err(DatabaseSetupError::schema_incompatible(
                path,
                vec![msg.clone()],
            ));
        }
        Err(e) => return Err(DatabaseSetupError::Migration(e)),
    }

    // Log user_version for diagnostic visibility (set by migrations; never used for decisions).
    {
        use sqlx::Row;
        let pool = db.get_sqlite_connection_pool();
        match sqlx::query("PRAGMA user_version").fetch_one(pool).await {
            Ok(row) => match row.try_get::<i64, _>(0) {
                Ok(uv) => tracing::info!("DB schema stamp: user_version={uv}"),
                Err(e) => tracing::warn!("Could not decode user_version: {e}"),
            },
            Err(e) => tracing::warn!("Could not read user_version: {e}"),
        }
    }

    Ok(db)
}

/// Run a health check against the database.
///
/// Thin wrapper so `services/api/` doesn't need a direct `sea-orm` dependency.
pub async fn health_check(db: &DatabaseConnection) -> Result<(), DomainError> {
    use sea_orm::ConnectionTrait;
    db.execute_unprepared("SELECT 1")
        .await
        .map(|_| ())
        .map_err(sea_err)
}

/// Lightweight runtime diagnostics for a single profile database connection.
///
/// Reads `PRAGMA user_version` and `PRAGMA journal_mode` via the underlying
/// sqlx pool. Keeps sqlx out of `services/api/` per the crate boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbRuntimeDiagnostics {
    pub schema_version: i64,
    pub wal_mode: bool,
}

pub async fn read_db_runtime_diagnostics(
    db: &DatabaseConnection,
) -> Result<DbRuntimeDiagnostics, DomainError> {
    use sqlx::Row;
    let pool = db.get_sqlite_connection_pool();

    let schema_version = sqlx::query("PRAGMA user_version")
        .fetch_one(pool)
        .await
        .and_then(|row| row.try_get::<i64, _>(0))
        .map_err(|e| DomainError::Internal {
            message: format!("read user_version: {e}"),
        })?;

    let journal_mode = sqlx::query("PRAGMA journal_mode")
        .fetch_one(pool)
        .await
        .and_then(|row| row.try_get::<String, _>(0))
        .map_err(|e| DomainError::Internal {
            message: format!("read journal_mode: {e}"),
        })?;

    Ok(DbRuntimeDiagnostics {
        schema_version,
        wal_mode: journal_mode.eq_ignore_ascii_case("wal"),
    })
}

/// Build the backup file path for a database backup.
///
/// Returns `{db_dir}/{db_filename}.backup-v{version}`, or `None` if the path
/// has no filename or is not valid UTF-8.
fn build_backup_path(db_path: &std::path::Path, version: &str) -> Option<std::path::PathBuf> {
    let file_name = db_path.file_name()?.to_str()?;
    Some(db_path.with_file_name(format!("{file_name}.backup-v{version}")))
}

/// Collect existing backup files for a database, sorted oldest-first by version suffix.
///
/// Scans the parent directory for files matching `{db_filename}.backup-v*`.
/// Returns `(path, mtime)` pairs; mtime falls back to `UNIX_EPOCH` on metadata errors.
pub async fn collect_existing_backups(
    db_path: &std::path::Path,
) -> Result<
    Vec<(std::path::PathBuf, std::time::SystemTime)>,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let parent = db_path.parent().ok_or("Invalid database path")?;
    let file_name = db_path
        .file_name()
        .ok_or("Invalid database path")?
        .to_str()
        .ok_or("Non-UTF8 database path")?;
    let backup_prefix = format!("{}.backup-v", file_name);

    let mut backups: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
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
/// `backups` must be sorted oldest-first (as returned by `collect_existing_backups`).
/// Deletion failures are logged as warnings and do not propagate as errors.
/// Returns the number of files that failed to delete.
async fn rotate_backups(backups: Vec<std::path::PathBuf>, keep: usize) -> usize {
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
/// The backup is named `{db_path}.backup-v{version}` where `version` is the
/// current schema version from the `seaql_migrations` table. Only the last 3
/// backups are kept; older ones are deleted.
///
/// Skips silently when:
/// - The database file does not exist (first run)
/// - The `seaql_migrations` table does not exist
///
/// # Important
/// Call this BEFORE opening any SQLx pool to the same database.
pub async fn pre_migration_backup(
    db_path: &std::path::Path,
) -> Result<Option<std::path::PathBuf>, Box<dyn std::error::Error + Send + Sync>> {
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
            let paths: Vec<std::path::PathBuf> = backups.into_iter().map(|(p, _)| p).collect();
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

/// Check whether the database file belongs to Mokumo by reading PRAGMA application_id.
///
/// Valid states:
/// - `0` — not yet stamped (existing installs before `m20260404_000000_set_pragmas` runs); valid.
/// - `0x4D4B4D4F` (1296780623, "MKMO") — stamped correctly; valid.
/// - any other non-zero — not a Mokumo database; returns `DatabaseSetupError::NotMokumoDatabase`.
///
/// Uses a raw `rusqlite::Connection` (pre-pool) so pool resources are never allocated
/// against an incompatible file.
///
/// # Important
/// Call this BEFORE opening any SQLx pool to the same database.
pub fn check_application_id(db_path: &std::path::Path) -> Result<(), DatabaseSetupError> {
    let conn = rusqlite::Connection::open(db_path)?;
    let app_id: i64 = conn.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    drop(conn);

    match app_id {
        0 => Ok(()),                                 // not-yet-stamped — valid
        id if id == MOKUMO_APPLICATION_ID => Ok(()), // "MKMO" — valid
        _ => Err(DatabaseSetupError::NotMokumoDatabase {
            path: db_path.to_path_buf(),
        }),
    }
}

/// Check whether the database schema is compatible with this binary by comparing
/// applied migrations in `seaql_migrations` against the binary's `Migrator::migrations()`.
///
/// Returns `Err(SchemaIncompatible)` if the database has any migrations the binary
/// does not know about — indicating the database was created by a newer version of Mokumo.
///
/// Silently succeeds when:
/// - The database file does not exist yet (fresh install).
/// - The `seaql_migrations` table does not exist (fresh database with no migrations run).
/// - All applied migrations are known to the binary.
///
/// Uses a raw `rusqlite::Connection` (pre-pool) so pool resources are never allocated
/// against an incompatible schema.
///
/// # Important
/// Call this BEFORE opening any SQLx pool to the same database.
pub fn check_schema_compatibility(db_path: &std::path::Path) -> Result<(), DatabaseSetupError> {
    use sea_orm_migration::MigratorTrait;

    if !db_path.exists() {
        return Ok(()); // Fresh install — nothing to check
    }

    let conn = rusqlite::Connection::open(db_path)?;

    // Check if seaql_migrations table exists (not present on a fresh SQLite file)
    let table_exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='seaql_migrations'",
        [],
        |row| row.get(0),
    )?;

    if !table_exists {
        drop(conn);
        return Ok(()); // No migrations applied yet — compatible
    }

    // Collect all applied migration version strings.
    // stmt borrows conn, so scope it to allow conn to drop afterward.
    let applied: Vec<String> = {
        let mut stmt = conn.prepare("SELECT version FROM seaql_migrations")?;
        stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    drop(conn);

    // Build a set of migration names the binary knows about
    let known: std::collections::HashSet<String> = migration::Migrator::migrations()
        .iter()
        .map(|m| m.name().to_owned())
        .collect();

    let unknown: Vec<String> = applied.into_iter().filter(|v| !known.contains(v)).collect();

    if unknown.is_empty() {
        Ok(())
    } else {
        Err(DatabaseSetupError::schema_incompatible(
            db_path.to_path_buf(),
            unknown,
        ))
    }
}

/// Ensure `auto_vacuum = INCREMENTAL` is enabled on a database file.
///
/// `auto_vacuum` is a schema-level PRAGMA stored in the database file header.
/// It cannot be reliably set via a connection pool's `after_connect` hook because
/// the file header is written when the connection is first established. This
/// function handles both new and existing databases:
///
/// - **New database** (file does not exist): creates the file via rusqlite and
///   sets `auto_vacuum = INCREMENTAL` before any tables are created.
/// - **Existing database with `auto_vacuum = 0`** (NONE): sets the PRAGMA and
///   runs a one-time `VACUUM` to restructure the file.
/// - **Existing database with `auto_vacuum = 1` or `2`**: no-op.
///
/// Uses a raw `rusqlite::Connection` (pre-pool) for the same reason as
/// `check_application_id`: no pool resources should be allocated until the
/// database file is structurally ready.
///
/// # Important
/// Call this AFTER `pre_migration_backup` (for existing DBs, the VACUUM rewrites
/// the file) and BEFORE `initialize_database`. The caller should wrap this in
/// `tokio::task::spawn_blocking` since `VACUUM` is heavyweight blocking I/O.
pub fn ensure_auto_vacuum(db_path: &std::path::Path) -> Result<(), DatabaseSetupError> {
    if !db_path.exists() {
        // Fresh install: create the file and set auto_vacuum before any tables.
        // The file is closed immediately — initialize_database opens the pool next.
        let conn = rusqlite::Connection::open(db_path)?;
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL")?;
        tracing::info!(
            "Created new database with auto_vacuum=INCREMENTAL at {}",
            db_path.display()
        );
        drop(conn);
        return Ok(());
    }

    let conn = rusqlite::Connection::open(db_path)?;
    let current: i32 = conn.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;

    match current {
        0 => {
            // NONE → INCREMENTAL: requires VACUUM to restructure the file.
            tracing::info!(
                "Upgrading auto_vacuum from NONE to INCREMENTAL on {}; running one-time VACUUM",
                db_path.display()
            );
            conn.execute_batch("PRAGMA auto_vacuum = 2; VACUUM;")?;
            tracing::info!("VACUUM complete for {}", db_path.display());
        }
        1 => {
            // FULL — already tracking free pages. No VACUUM needed.
            tracing::debug!(
                "auto_vacuum is FULL on {}, no upgrade needed",
                db_path.display()
            );
        }
        2 => {
            // INCREMENTAL — already set.
            tracing::debug!(
                "auto_vacuum is already INCREMENTAL on {}",
                db_path.display()
            );
        }
        other => {
            tracing::warn!(
                "Unexpected auto_vacuum value {other} on {}; skipping upgrade",
                db_path.display()
            );
        }
    }

    drop(conn);
    Ok(())
}

/// Open a raw SQLite connection pool with the same PRAGMAs as `initialize_database`.
///
/// This is for auxiliary databases (e.g. sessions.db) that don't use SeaORM
/// migrations but still need WAL mode and standard safety PRAGMAs.
pub async fn open_raw_sqlite_pool(
    database_url: &str,
) -> Result<sqlx::SqlitePool, DatabaseSetupError> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .after_connect(|conn, _meta| configure_sqlite_connection(conn))
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Query the `settings` table for the `setup_mode` value.
///
/// Returns `None` if the key doesn't exist (fresh install).
pub async fn get_setup_mode(
    db: &DatabaseConnection,
) -> Result<Option<mokumo_core::setup::SetupMode>, DatabaseSetupError> {
    let pool = db.get_sqlite_connection_pool();
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'setup_mode'")
            .fetch_optional(pool)
            .await
            .map_err(DatabaseSetupError::Query)?;

    match row {
        Some((Some(ref v),)) => {
            let mode: mokumo_core::setup::SetupMode = v
                .parse()
                .map_err(|e: String| DatabaseSetupError::Query(sqlx::Error::Protocol(e)))?;
            Ok(Some(mode))
        }
        _ => Ok(None),
    }
}

/// Fetch the shop name from the `settings` table.
///
/// Returns `None` if the key has not been written yet (before setup completes).
pub async fn get_shop_name(db: &DatabaseConnection) -> Result<Option<String>, DatabaseSetupError> {
    let pool = db.get_sqlite_connection_pool();
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'shop_name'")
            .fetch_optional(pool)
            .await
            .map_err(DatabaseSetupError::Query)?;
    Ok(row.and_then(|(v,)| v))
}

/// Check whether first-run setup has been completed.
///
/// Queries the `settings` table for a row with `key = 'setup_complete'` and
/// returns `true` only when `value = "true"`.
pub async fn is_setup_complete(db: &DatabaseConnection) -> Result<bool, DatabaseSetupError> {
    let pool = db.get_sqlite_connection_pool();
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'setup_complete'")
            .fetch_optional(pool)
            .await
            .map_err(DatabaseSetupError::Query)?;

    Ok(matches!(row, Some((Some(ref v),)) if v == "true"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mokumo_core::setup::SetupMode;

    async fn test_db() -> (DatabaseConnection, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
        let db = initialize_database(&url).await.unwrap();
        (db, tmp)
    }

    // ── pre_migration_backup ──────────────────────────────────────────────────

    #[tokio::test]
    async fn pre_migration_backup_skips_nonexistent_path() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonexistent.db");
        // Path doesn't exist — should return Ok immediately
        pre_migration_backup(&path).await.unwrap();
        // No backup files should have been created
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
        // Create a raw SQLite file without seaql_migrations table
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE foo (id INTEGER)").unwrap();
        }
        pre_migration_backup(&path).await.unwrap();
        // Only the original DB should exist — no backup files
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
        // Drop the connection so SQLite is idle before backup
        drop(db);

        pre_migration_backup(&path).await.unwrap();

        // A backup file matching the pattern should exist
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

        // Running backup now gives 4 total → should rotate oldest away
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
        // The oldest fake ("a_old1") should have been deleted
        assert!(
            !backups.iter().any(|n| n.contains("a_old1")),
            "oldest backup should have been removed"
        );
        // The newest real backup should remain
        assert!(
            backups
                .iter()
                .any(|n| n.starts_with("test.db.backup-v") && !n.contains("a_old")),
            "real backup should be retained"
        );
    }

    // ── get_setup_mode ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_setup_mode_returns_none_when_absent() {
        let (db, _tmp) = test_db().await;
        let mode = get_setup_mode(&db).await.unwrap();
        assert_eq!(mode, None);
    }

    #[tokio::test]
    async fn get_setup_mode_returns_demo() {
        let (db, _tmp) = test_db().await;
        let pool = db.get_sqlite_connection_pool();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('setup_mode', 'demo')")
            .execute(pool)
            .await
            .unwrap();
        let mode = get_setup_mode(&db).await.unwrap();
        assert_eq!(mode, Some(SetupMode::Demo));
    }

    #[tokio::test]
    async fn get_setup_mode_returns_production() {
        let (db, _tmp) = test_db().await;
        let pool = db.get_sqlite_connection_pool();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('setup_mode', 'production')")
            .execute(pool)
            .await
            .unwrap();
        let mode = get_setup_mode(&db).await.unwrap();
        assert_eq!(mode, Some(SetupMode::Production));
    }

    #[tokio::test]
    async fn get_setup_mode_returns_error_on_invalid_value() {
        let (db, _tmp) = test_db().await;
        let pool = db.get_sqlite_connection_pool();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('setup_mode', 'bogus')")
            .execute(pool)
            .await
            .unwrap();
        let result = get_setup_mode(&db).await;
        assert!(
            result.is_err(),
            "unknown setup_mode value should return an error"
        );
    }

    // ── open_raw_sqlite_pool ──────────────────────────────────────────────

    #[tokio::test]
    async fn open_raw_sqlite_pool_creates_accessible_pool() {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("raw.db").display());
        let pool = open_raw_sqlite_pool(&url).await.unwrap();
        let result: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await.unwrap();
        assert_eq!(result.0, 1);
    }

    // ── health_check ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn health_check_passes_on_fresh_database() {
        let (db, _tmp) = test_db().await;
        assert!(health_check(&db).await.is_ok());
    }

    // ── build_backup_path ──────────────────────────────────────────────────

    #[test]
    fn build_backup_path_appends_version_suffix() {
        let path = std::path::Path::new("/tmp/mokumo.db");
        let result = build_backup_path(path, "m20260326_000000_customers").unwrap();
        assert_eq!(
            result,
            std::path::PathBuf::from("/tmp/mokumo.db.backup-vm20260326_000000_customers")
        );
    }

    #[test]
    fn build_backup_path_preserves_parent_directory() {
        let path = std::path::Path::new("/home/user/data/shop.db");
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
        // Create backups out of order
        tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260326_z"), b"b3")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260322_a"), b"b1")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260324_m"), b"b2")
            .await
            .unwrap();
        // Non-matching file should be excluded
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
        // Pass pre-sorted list (oldest first) — same ordering collect_existing_backups produces
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
        // Matching backup
        tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260322_a"), b"ok")
            .await
            .unwrap();
        // Over-match candidates that must be excluded:
        // Has right prefix but "backup-v" only appears mid-name
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
