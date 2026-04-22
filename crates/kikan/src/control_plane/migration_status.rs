//! Pure-function migration status collection for the admin surface.
//!
//! Queries the `kikan_migrations` table on both profile databases and
//! returns the list of applied migrations with their graft IDs. Handles
//! the "table does not exist" case gracefully (returns empty list).

use kikan_types::admin::{AppliedMigration, MigrationStatusResponse, ProfileMigrationStatus};
use sea_orm::{DatabaseConnection, FromQueryResult, Statement};

use crate::db::diagnostics::read_db_runtime_diagnostics;
use crate::{ControlPlaneError, PlatformState};

/// Collect migration status for both profiles.
pub async fn collect_migration_status(
    state: &PlatformState,
) -> Result<MigrationStatusResponse, ControlPlaneError> {
    let production_db = state.db_for("production").ok_or_else(|| {
        ControlPlaneError::Internal(anyhow::anyhow!(
            "production profile pool missing from PlatformState"
        ))
    })?;
    let demo_db = state.db_for("demo").ok_or_else(|| {
        ControlPlaneError::Internal(anyhow::anyhow!(
            "demo profile pool missing from PlatformState"
        ))
    })?;
    let production = profile_migration_status(production_db).await?;
    let demo = profile_migration_status(demo_db).await?;

    Ok(MigrationStatusResponse { production, demo })
}

async fn profile_migration_status(
    db: &DatabaseConnection,
) -> Result<ProfileMigrationStatus, ControlPlaneError> {
    let schema_version = match read_db_runtime_diagnostics(db).await {
        Ok(d) => d.schema_version,
        Err(_) => 0,
    };

    let applied = query_applied_migrations(db).await?;

    Ok(ProfileMigrationStatus {
        applied,
        schema_version,
    })
}

/// Query the kikan_migrations table for applied migrations.
///
/// Returns an empty Vec if the table does not exist (fresh database).
async fn query_applied_migrations(
    db: &DatabaseConnection,
) -> Result<Vec<AppliedMigration>, ControlPlaneError> {
    #[derive(Debug, FromQueryResult)]
    struct Row {
        graft_id: String,
        name: String,
        applied_at: i64,
    }

    // Query applied migrations. If the table does not exist (fresh DB),
    // the query will fail — catch and return empty list.
    let rows: Vec<Row> = match Row::find_by_statement(Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT graft_id, name, applied_at FROM kikan_migrations ORDER BY applied_at ASC",
    ))
    .all(db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            // "no such table" is expected for fresh databases.
            let msg = e.to_string();
            if msg.contains("no such table") {
                return Ok(Vec::new());
            }
            return Err(ControlPlaneError::Internal(anyhow::anyhow!(
                "query kikan_migrations failed: {e}"
            )));
        }
    };

    Ok(rows
        .into_iter()
        .map(|r| AppliedMigration {
            graft_id: r.graft_id,
            name: r.name,
            applied_at: r.applied_at,
        })
        .collect())
}
