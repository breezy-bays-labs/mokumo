//! Mokumo-vertical database primitives (pool opener + schema helpers).
//!
//! Thin wrappers that bind the vertical-agnostic `kikan::db::open_pool`
//! primitives and SeaORM's downgrade-detection error path to this
//! vertical's migrator ([`crate::migrations::Migrator`]).

use kikan::db::{DBERRCOMPAT_PATTERN, DatabaseSetupError};
use sea_orm::DatabaseConnection;
use sea_orm_migration::MigratorTrait;

use crate::migrations::Migrator;

/// Returns the names of all migrations registered with the mokumo
/// vertical migrator, in declaration order. Used by `mokumo migrate
/// status` to compute which migrations are still pending.
pub fn known_migration_names() -> Vec<String> {
    Migrator::migrations()
        .iter()
        .map(|m| m.name().to_string())
        .collect()
}

/// Create a mokumo-vertical database: open a pool with the kikan PRAGMA
/// set, run the mokumo migrator, and apply the post-migration advisory
/// steps.
///
/// Re-surfaces SeaORM's "downgrade detected" error variant as
/// [`DatabaseSetupError::SchemaIncompatible`] so callers produce a
/// human-readable message.
pub async fn initialize_database(
    database_url: &str,
) -> Result<DatabaseConnection, DatabaseSetupError> {
    let db = kikan::db::initialize_database(database_url).await?;

    // Platform tables (users, roles, shop_settings) are owned by kikan's
    // platform graft. Run them first so the vertical migrator can ALTER
    // TABLE users (login_lockout) without error.
    kikan::migrations::platform::run_platform_migrations(&db)
        .await
        .map_err(|e| DatabaseSetupError::Migration(sea_orm::DbErr::Custom(e.to_string())))?;

    match Migrator::up(&db, None).await {
        Ok(()) => {}
        Err(sea_orm::DbErr::Custom(ref msg)) if msg.contains(DBERRCOMPAT_PATTERN) => {
            let path = sqlite_url_to_path(database_url);
            // Prefer the structured list of unknown migrations from the
            // compatibility check over the raw SeaORM message so the
            // user-facing error surfaces clean migration names.
            let unknown = match kikan::db::check_schema_compatibility::<Migrator>(&path) {
                Err(DatabaseSetupError::SchemaIncompatible {
                    unknown_migrations, ..
                }) => unknown_migrations,
                _ => vec![msg.clone()],
            };
            return Err(DatabaseSetupError::schema_incompatible(path, unknown));
        }
        Err(e) => return Err(DatabaseSetupError::Migration(e)),
    }

    kikan::db::post_migration_optimize(&db).await;
    kikan::db::log_user_version(&db).await;

    Ok(db)
}

/// Check whether the database schema is compatible with this binary by
/// comparing applied migrations in `seaql_migrations` against the
/// mokumo-vertical migrator's known migrations. Thin binding of
/// [`kikan::db::check_schema_compatibility`] to [`Migrator`].
pub fn check_schema_compatibility(db_path: &std::path::Path) -> Result<(), DatabaseSetupError> {
    kikan::db::check_schema_compatibility::<Migrator>(db_path)
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

/// Convert a `sqlite:[//[/]]path[?query]` URL into a filesystem path.
///
/// Handles `sqlite:`, `sqlite://`, and `sqlite:///` prefixes and strips
/// trailing `?` query parameters (e.g. `mode=rwc`).
fn sqlite_url_to_path(database_url: &str) -> std::path::PathBuf {
    let stripped = database_url
        .strip_prefix("sqlite:///")
        .or_else(|| database_url.strip_prefix("sqlite://"))
        .or_else(|| database_url.strip_prefix("sqlite:"))
        .unwrap_or(database_url);
    let path_str = stripped.split('?').next().unwrap_or(stripped);
    std::path::PathBuf::from(path_str)
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
        assert!(get_setup_mode(&db).await.is_err());
    }

    #[tokio::test]
    async fn is_setup_complete_false_on_fresh_db() {
        let (db, _tmp) = test_db().await;
        assert!(!is_setup_complete(&db).await.unwrap());
    }

    #[tokio::test]
    async fn is_setup_complete_true_when_flag_set() {
        let (db, _tmp) = test_db().await;
        let pool = db.get_sqlite_connection_pool();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('setup_complete', 'true')")
            .execute(pool)
            .await
            .unwrap();
        assert!(is_setup_complete(&db).await.unwrap());
    }

    #[tokio::test]
    async fn get_shop_name_returns_none_when_absent() {
        let (db, _tmp) = test_db().await;
        assert_eq!(get_shop_name(&db).await.unwrap(), None);
    }

    #[tokio::test]
    async fn get_shop_name_returns_stored_value() {
        let (db, _tmp) = test_db().await;
        let pool = db.get_sqlite_connection_pool();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('shop_name', 'Acme')")
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(get_shop_name(&db).await.unwrap(), Some("Acme".into()));
    }

    // ── kikan::db::diagnostics smoke tests (migrated DB fixtures live here) ──

    #[tokio::test]
    async fn runtime_diagnostics_reports_schema_version() {
        let (db, _tmp) = test_db().await;
        let diag = kikan::db::read_db_runtime_diagnostics(&db).await.unwrap();
        assert!(
            diag.schema_version > 0,
            "migrated db should have non-zero user_version, got {}",
            diag.schema_version
        );
        assert!(diag.wal_mode, "initialize_database must enable WAL mode");
    }

    #[tokio::test]
    async fn health_check_passes_on_fresh_database() {
        let (db, _tmp) = test_db().await;
        assert!(kikan::db::health_check(&db).await.is_ok());
    }
}
