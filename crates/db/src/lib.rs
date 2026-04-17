pub mod activity;
pub mod meta;
pub mod migration;
pub mod restore;
pub mod role;
pub mod sequence;
pub mod shop;

use mokumo_core::error::DomainError;

pub use sea_orm::DatabaseConnection;

// Stage 3 (#507): platform primitives lifted to `kikan::db` and
// `kikan::backup`. These re-exports keep existing tests (and the
// migration/bdd/restore step definitions under `crates/db/tests/`)
// compiling against `mokumo_db::*` while services/api migrates its call
// sites to the kikan paths directly. Both `crates/db` and these
// re-exports dissolve in S3.1b when the crate is removed.
pub use kikan::backup::{
    BackupError, BackupResult, RestoreResult, build_timestamped_name, collect_existing_backups,
    create_backup, pre_migration_backup, restore_from_backup, verify_integrity,
};
pub use kikan::db::{
    CONFIGURED_MMAP_SIZE, DBERRCOMPAT_PATTERN, DatabaseSetupError,
    KIKAN_APPLICATION_ID as MOKUMO_APPLICATION_ID, check_application_id, ensure_auto_vacuum,
    initialize_database as initialize_pool, open_raw_sqlite_pool,
};

/// Re-export of [`kikan::backup`] under the pre-Stage-3 path
/// (`mokumo_db::backup`) so call sites using `mokumo_db::backup::...`
/// resolve during the transition. New code should import from
/// [`kikan::backup`] directly.
pub use kikan::backup;

/// Returns the names of all migrations registered with the Migrator, in declaration order.
///
/// Used by `mokumo migrate status` to compare known migrations against those recorded
/// in the `seaql_migrations` table, computing which are pending.
pub fn known_migration_names() -> Vec<String> {
    use crate::migration::Migrator;
    use sea_orm_migration::MigratorTrait;
    Migrator::migrations()
        .iter()
        .map(|m| m.name().to_string())
        .collect()
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

/// Create a mokumo-vertical database: open a pool with the kikan PRAGMA
/// set, run the mokumo migrator, and apply the post-migration advisory
/// steps.
///
/// Pre-Stage-3 this was `mokumo_db::initialize_database`. During Stage 3
/// the pool/PRAGMA primitives live in `kikan::db`; this function remains
/// as a thin vertical wrapper that binds the kikan pool opener to
/// `crate::migration::Migrator`. It disappears alongside the rest of
/// `crates/db` in S3.1b.
///
/// Re-surfaces SeaORM's "downgrade detected" error variant as
/// [`DatabaseSetupError::SchemaIncompatible`] so callers produce a
/// human-readable message.
pub async fn initialize_database(
    database_url: &str,
) -> Result<DatabaseConnection, DatabaseSetupError> {
    use sea_orm_migration::MigratorTrait;

    let db = kikan::db::initialize_database(database_url).await?;

    match migration::Migrator::up(&db, None).await {
        Ok(()) => {}
        Err(sea_orm::DbErr::Custom(ref msg)) if msg.contains(DBERRCOMPAT_PATTERN) => {
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

    kikan::db::post_migration_optimize(&db).await;
    kikan::db::log_user_version(&db).await;

    Ok(db)
}

/// Check whether the database schema is compatible with this binary by
/// comparing applied migrations in `seaql_migrations` against the
/// mokumo-vertical migrator's known migrations.
///
/// Pre-Stage-3 this was defined inline in `mokumo_db`. Stage 3 moved the
/// generic comparison into `kikan::db::check_schema_compatibility<M>`;
/// this function binds it to `crate::migration::Migrator`.
pub fn check_schema_compatibility(db_path: &std::path::Path) -> Result<(), DatabaseSetupError> {
    kikan::db::check_schema_compatibility::<migration::Migrator>(db_path)
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

/// Check whether the database has a fully-seeded demo admin account.
///
/// Returns `true` when `admin@demo.local` exists, is active, is not soft-deleted,
/// and has a non-empty `password_hash`. Returns `false` on any DB error (logged at
/// error level) or when the predicate is not met.
///
/// This is a plain database predicate — it does not inspect the active profile.
/// Callers are responsible for only invoking this check on the demo database.
pub async fn validate_installation(db: &DatabaseConnection) -> bool {
    use kikan::auth::entity_user::{Column, Entity as UserEntity};
    use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

    let result = UserEntity::find()
        .filter(Column::Email.eq("admin@demo.local"))
        .filter(Column::PasswordHash.ne(""))
        .filter(Column::IsActive.eq(true))
        .filter(Column::DeletedAt.is_null())
        .count(db)
        .await;

    match result {
        Ok(count) => count > 0,
        Err(e) => {
            tracing::error!("validate_installation failed — defaulting to false: {e}");
            false
        }
    }
}

/// Disk-level diagnostics for a SQLite database file.
///
/// Collected synchronously via rusqlite (no async) so this can be called
/// from both the doctor CLI (sync context) and via `spawn_blocking` from
/// async handlers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DbDiagnostics {
    pub auto_vacuum: i32,
    pub freelist_count: i64,
    pub page_count: i64,
    pub page_size: i64,
    /// Size of the WAL file in bytes; 0 when no WAL file exists.
    pub wal_size_bytes: u64,
}

impl DbDiagnostics {
    /// Returns `true` when more than 20 % of pages are free (unfragmented space
    /// reclaimed by deletions). Threshold is defined here so all callers stay in sync.
    pub fn vacuum_needed(&self) -> bool {
        self.page_count > 0 && (self.freelist_count as f64 / self.page_count as f64) > 0.20
    }
}

/// Collect disk-level diagnostics for a SQLite database file.
///
/// Opens the file read-only (SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX), reads
/// the four key PRAGMAs, and measures the WAL file size via `fs::metadata`.
/// Read-only mode avoids contending for write locks on a WAL-mode database and
/// prevents accidental file creation when the path does not yet exist.
///
/// # Errors
///
/// Returns `rusqlite::Error` if the file cannot be opened or a PRAGMA
/// query fails. Callers should treat errors as "unknown" diagnostics rather
/// than a hard failure.
pub fn diagnose_database(db_path: &std::path::Path) -> Result<DbDiagnostics, rusqlite::Error> {
    use rusqlite::OpenFlags;
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    fn get_pragma<T: rusqlite::types::FromSql>(
        conn: &rusqlite::Connection,
        pragma: &str,
    ) -> Result<T, rusqlite::Error> {
        conn.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
    }

    let auto_vacuum: i32 = get_pragma(&conn, "auto_vacuum")?;
    let freelist_count: i64 = get_pragma(&conn, "freelist_count")?;
    let page_count: i64 = get_pragma(&conn, "page_count")?;
    let page_size: i64 = get_pragma(&conn, "page_size")?;

    // WAL file lives at "{db_path}-wal"; missing → 0 bytes.
    let wal_size_bytes = {
        let mut wal = db_path.as_os_str().to_owned();
        wal.push("-wal");
        std::fs::metadata(std::path::Path::new(&wal))
            .map(|m| m.len())
            .unwrap_or(0)
    };

    Ok(DbDiagnostics {
        auto_vacuum,
        freelist_count,
        page_count,
        page_size,
        wal_size_bytes,
    })
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

/// Query the `settings` table for the `setup_mode` value.
///
/// Returns `None` if the key doesn't exist (fresh install).
pub async fn get_setup_mode(
    db: &DatabaseConnection,
) -> Result<Option<kikan::SetupMode>, DatabaseSetupError> {
    let pool = db.get_sqlite_connection_pool();
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'setup_mode'")
            .fetch_optional(pool)
            .await
            .map_err(DatabaseSetupError::Query)?;

    match row {
        Some((Some(ref v),)) => {
            let mode: kikan::SetupMode = v
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

/// Fetch the logo extension and cache-buster timestamp from shop_settings.
///
/// Returns `None` if the row does not exist or either `logo_extension` or `logo_updated_at` is NULL.
pub async fn get_logo_info(
    db: &DatabaseConnection,
) -> Result<Option<(String, i64)>, DatabaseSetupError> {
    shop::get_logo_info(db)
        .await
        .map_err(|e| DatabaseSetupError::Query(sqlx::Error::Protocol(e.to_string())))
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
    use kikan::SetupMode;

    async fn test_db() -> (DatabaseConnection, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
        let db = initialize_database(&url).await.unwrap();
        (db, tmp)
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

    // ── health_check ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn health_check_passes_on_fresh_database() {
        let (db, _tmp) = test_db().await;
        assert!(health_check(&db).await.is_ok());
    }
}
