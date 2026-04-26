//! Database diagnostics (disk + runtime) and liveness checks.
//!
//! Platform-generic SQLite utilities — disk size, page counts, readiness
//! probes. Vertical-agnostic by contract (I1).

use crate::error::DomainError;
use sea_orm::DatabaseConnection;

/// Run a liveness check against the database.
///
/// Thin wrapper so callers don't need a direct `sea-orm` dependency.
pub async fn health_check(db: &DatabaseConnection) -> Result<(), DomainError> {
    use sea_orm::ConnectionTrait;
    db.execute_unprepared("SELECT 1")
        .await
        .map(|_| ())
        .map_err(|e| DomainError::Internal {
            message: e.to_string(),
        })
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
/// sqlx pool. Keeps sqlx out of caller crates per the layering boundary.
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

/// Check whether the database has a fully-seeded demo admin account.
///
/// Returns `true` when `admin@demo.local` exists, is active, is not
/// soft-deleted, and has a non-empty `password_hash`. Returns `false` on
/// any DB error (logged at error level) or when the predicate is not met.
///
/// This is a plain database predicate — it does not inspect the active
/// profile. Callers are responsible for only invoking this check on the
/// demo database.
pub async fn validate_installation(db: &DatabaseConnection) -> bool {
    use crate::auth::entity_user::{Column, Entity as UserEntity};
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
