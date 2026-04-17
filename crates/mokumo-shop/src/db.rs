//! Mokumo-vertical database primitives (pool opener + schema helpers).
//!
//! After S2.5 lifted the migrator into `mokumo_shop::migrations`, the thin
//! wrappers that bind the kikan pool opener + SeaORM's downgrade-detection
//! error path to this vertical's migrator live here. This is the last
//! piece of vertical DB plumbing that `crates/db` owned before the
//! migrator relocation; moving it up the ladder keeps `crates/db` free of
//! any runtime dependency on `mokumo-shop`.

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
