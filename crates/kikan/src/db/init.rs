//! Database pool creation and schema compatibility checks.
//!
//! This module owns the **pool side** of startup. Migrations are the
//! vertical's responsibility per the "Migration Ownership" golden rule:
//! [`initialize_database`] creates a WAL-configured connection pool but
//! does **not** run migrations. Callers run their own
//! `MigratorTrait::up()` (or the kikan migration runner via
//! `Engine::run_migrations`) after this returns.
//!
//! [`check_schema_compatibility`] is generic over
//! [`sea_orm_migration::MigratorTrait`] so callers can supply their own
//! migrator type without forcing a dependency on a specific vertical's
//! migration list.

use sqlx::sqlite::SqlitePoolOptions;

use crate::db::pragmas::configure_sqlite_connection;
pub use sea_orm::DatabaseConnection;

/// SeaORM emits this message when a DB has migrations the binary doesn't
/// know about (downgrade scenario). Exposed as defense-in-depth so callers
/// that run `MigratorTrait::up` can translate `DbErr::Custom(msg)` into
/// [`DatabaseSetupError::SchemaIncompatible`] after a
/// [`check_schema_compatibility`] pass.
///
/// Validated against `sea-orm-migration = "=2.0.0-rc.37"`. If SeaORM
/// changes this message format in a future version, the downgrade-handling
/// tests in `crates/mokumo-shop/tests/` will catch it.
pub const DBERRCOMPAT_PATTERN: &str = "Migration file of version";

/// Error type for platform database initialization (pool creation + schema
/// compatibility + pre-pool guards).
///
/// Vertical migration errors are the caller's responsibility — they
/// surface from the caller's chosen [`MigratorTrait::up`] invocation.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseSetupError {
    #[error("pool creation failed: {0}")]
    Pool(#[from] sqlx::Error),

    #[error("migration failed: {0}")]
    Migration(#[from] sea_orm::DbErr),

    #[error("database query failed: {0}")]
    Query(sqlx::Error),

    /// Returned when the database file does not appear to be a kikan
    /// database (PRAGMA application_id is non-zero and not
    /// `0x4D4B4D4F`).
    #[error("not a kikan database: {}", path.display())]
    NotKikanDatabase { path: std::path::PathBuf },

    /// Returned when the database contains applied migrations not known to
    /// this binary — indicating the database was created by a newer
    /// version of the vertical.
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
    /// Panics if `unknown_migrations` is empty — an incompatibility
    /// without any unknown migrations is a bug in the caller.
    pub fn schema_incompatible(path: std::path::PathBuf, unknown_migrations: Vec<String>) -> Self {
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

/// Create a SQLite connection pool with WAL mode, 5s busy timeout, foreign
/// keys enforced, and the platform-standard PRAGMA set.
///
/// Pool-first wrapping: create `SqlitePool` with PRAGMA hooks, then wrap
/// via `SqlxSqliteConnector::from_sqlx_sqlite_pool` for
/// `DatabaseConnection`.
///
/// The `database_url` should include `?mode=rwc` if the file may not
/// exist yet.
///
/// # Migrations
/// This function does **not** run migrations. Callers are responsible for
/// invoking their chosen [`sea_orm_migration::MigratorTrait::up`] (or the
/// kikan `Engine::run_migrations` helper) after the pool is returned. The
/// caller should translate `DbErr::Custom(msg)` where `msg` contains
/// [`DBERRCOMPAT_PATTERN`] into [`DatabaseSetupError::SchemaIncompatible`]
/// so downgrade attempts surface a human-readable message.
pub async fn initialize_database(
    database_url: &str,
) -> Result<DatabaseConnection, DatabaseSetupError> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .after_connect(|conn, _meta| configure_sqlite_connection(conn))
        .connect(database_url)
        .await?;

    Ok(sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool))
}

/// Run `PRAGMA optimize(0xfffe)` so SQLite can update its query-planner
/// statistics. Advisory — failure is logged but does not abort
/// initialization. Callers typically invoke this after their migration
/// runner completes.
pub async fn post_migration_optimize(db: &DatabaseConnection) {
    let pool = db.get_sqlite_connection_pool();
    if let Err(e) = sqlx::query("PRAGMA optimize(0xfffe)").execute(pool).await {
        tracing::warn!("PRAGMA optimize(0xfffe) after migration failed: {e}");
    }
}

/// Log `PRAGMA user_version` for diagnostic visibility. Set by migrations;
/// never used for decisions.
pub async fn log_user_version(db: &DatabaseConnection) {
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

/// Check whether the database schema is compatible with this binary by
/// comparing applied migrations in `seaql_migrations` against the given
/// migrator type's known migrations.
///
/// Returns `Err(SchemaIncompatible)` if the database has any migrations
/// the binary does not know about — indicating the database was created
/// by a newer version of the vertical.
///
/// Silently succeeds when:
/// - The database file does not exist yet (fresh install).
/// - The `seaql_migrations` table does not exist (fresh database with no
///   migrations run).
/// - All applied migrations are known to the binary.
///
/// Uses a raw `rusqlite::Connection` (pre-pool) so pool resources are
/// never allocated against an incompatible schema.
///
/// # Important
/// Call this BEFORE opening any SQLx pool to the same database.
pub fn check_schema_compatibility<M: sea_orm_migration::MigratorTrait>(
    db_path: &std::path::Path,
) -> Result<(), DatabaseSetupError> {
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
    let known: std::collections::HashSet<String> = M::migrations()
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
/// `auto_vacuum` is a schema-level PRAGMA stored in the database file
/// header. It cannot be reliably set via a connection pool's
/// `after_connect` hook because the file header is written when the
/// connection is first established. This function handles both new and
/// existing databases:
///
/// - **New database** (file does not exist): creates the file via
///   rusqlite and sets `auto_vacuum = INCREMENTAL` before any tables are
///   created.
/// - **Existing database with `auto_vacuum = 0`** (NONE): sets the PRAGMA
///   and runs a one-time `VACUUM` to restructure the file.
/// - **Existing database with `auto_vacuum = 1` or `2`**: no-op.
///
/// Uses a raw `rusqlite::Connection` (pre-pool) for the same reason as
/// `check_application_id`: no pool resources should be allocated until
/// the database file is structurally ready.
///
/// # Important
/// Call this AFTER `pre_migration_backup` (for existing DBs, the VACUUM
/// rewrites the file) and BEFORE `initialize_database`. The caller should
/// wrap this in `tokio::task::spawn_blocking` since `VACUUM` is
/// heavyweight blocking I/O.
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
            tracing::debug!(
                "auto_vacuum is FULL on {}, no upgrade needed",
                db_path.display()
            );
        }
        2 => {
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

/// Open a raw SQLite connection pool with the same PRAGMAs as
/// [`initialize_database`].
///
/// This is for auxiliary databases (e.g. sessions.db) that don't use
/// SeaORM migrations but still need WAL mode and standard safety PRAGMAs.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_raw_sqlite_pool_creates_accessible_pool() {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("raw.db").display());
        let pool = open_raw_sqlite_pool(&url).await.unwrap();
        let result: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await.unwrap();
        assert_eq!(result.0, 1);
    }

    #[tokio::test]
    async fn initialize_database_creates_pool_without_migrations() {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("raw.db").display());
        let db = initialize_database(&url).await.unwrap();
        // Pool works.
        use sea_orm::ConnectionTrait;
        db.execute_unprepared("SELECT 1").await.unwrap();
        // No seaql_migrations table — pool-only, no migrations ran.
        let pool = db.get_sqlite_connection_pool();
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='seaql_migrations'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(count.0, 0);
    }
}
