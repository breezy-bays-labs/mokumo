pub mod activity;
pub mod customer;
pub mod migration;
pub mod sequence;

use mokumo_core::error::DomainError;
use sqlx::sqlite::SqlitePoolOptions;

pub use sea_orm::DatabaseConnection;

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
pub async fn initialize_database(database_url: &str) -> Result<DatabaseConnection, sqlx::Error> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .after_connect(|conn, _meta| {
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
        })
        .connect(database_url)
        .await?;

    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);

    use sea_orm_migration::MigratorTrait;
    migration::Migrator::up(&db, None)
        .await
        .map_err(|e| sqlx::Error::Protocol(format!("SeaORM migration failed: {e}")))?;

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

/// Check whether first-run setup has been completed.
///
/// Queries the `settings` table for a row with `key = 'setup_complete'` and
/// returns `true` only when `value = "true"`.
pub async fn is_setup_complete(db: &DatabaseConnection) -> Result<bool, sqlx::Error> {
    let pool = db.get_sqlite_connection_pool();
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'setup_complete'")
            .fetch_optional(pool)
            .await?;

    Ok(matches!(row, Some((Some(ref v),)) if v == "true"))
}
