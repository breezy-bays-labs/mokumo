pub mod activity;
pub mod customer;
pub mod migration;
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
    migration::Migrator::up(&db, None).await?;

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
) -> Result<(), Box<dyn std::error::Error>> {
    match tokio::fs::metadata(db_path).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!("No existing database at {:?}, skipping backup", db_path);
            return Ok(());
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
            return Ok(());
        }
        let v: String = conn.query_row("SELECT MAX(version) FROM seaql_migrations", [], |row| {
            row.get(0)
        })?;
        v
        // conn dropped here
    };

    // Build the backup filename as {original_name}.backup-v{version}
    let file_name = db_path
        .file_name()
        .ok_or("Invalid database path")?
        .to_str()
        .ok_or("Non-UTF8 database path")?;
    let backup_name = format!("{}.backup-v{}", file_name, version);
    let backup_path = db_path.with_file_name(&backup_name);

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
    .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })??;
    tracing::info!("Created database backup at {:?}", backup_path);

    // Rotate: keep only the last 3 backups
    let parent = db_path.parent().ok_or("Invalid database path")?;
    let prefix = format!("{}.", file_name);

    let mut backups: Vec<std::path::PathBuf> = Vec::new();
    let mut entries = tokio::fs::read_dir(parent).await?;
    while let Some(entry) = entries.next_entry().await? {
        let entry_name = entry.file_name();
        let name = entry_name.to_str().unwrap_or("");
        if name.starts_with(&prefix) && name.contains("backup-v") {
            backups.push(entry.path());
        }
    }

    // Sort lexicographically by version suffix — migration names are
    // timestamp-prefixed (e.g. "m20260326_...") so lexicographic = chronological.
    backups.sort_by(|a, b| {
        let version = |p: &std::path::PathBuf| {
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.rsplit("backup-v").next())
                .unwrap_or("")
                .to_string()
        };
        version(a).cmp(&version(b))
    });
    if backups.len() > 3 {
        let to_delete = backups.len() - 3;
        for path in backups.into_iter().take(to_delete) {
            match tokio::fs::remove_file(&path).await {
                Ok(()) => tracing::info!("Removed old backup {:?}", path),
                Err(e) => tracing::warn!(
                    "Failed to remove old backup {:?}: {}. Manual cleanup may be needed.",
                    path,
                    e
                ),
            }
        }
    }

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
}
