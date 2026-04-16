use axum::{Json, Router, extract::State, routing::get};
use mokumo_types::settings::{LanAccessRequest, LanAccessResponse};
use sea_orm_migration::sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value,
};

use crate::SharedState;
use crate::error::AppError;

pub const LAN_ACCESS_KEY: &str = "lan_access_enabled";

pub fn router() -> Router<SharedState> {
    Router::new().route("/lan-access", get(get_lan_access).put(put_lan_access))
}

async fn get_lan_access(
    State(state): State<SharedState>,
) -> Result<Json<LanAccessResponse>, AppError> {
    let db = state.db_for(*state.active_profile.read());
    let enabled = read_lan_access_enabled(db).await?;
    Ok(Json(LanAccessResponse { enabled }))
}

async fn put_lan_access(
    State(state): State<SharedState>,
    Json(req): Json<LanAccessRequest>,
) -> Result<Json<LanAccessResponse>, AppError> {
    let db = state.db_for(*state.active_profile.read());
    write_lan_access_enabled(db, req.enabled).await?;
    Ok(Json(LanAccessResponse {
        enabled: req.enabled,
    }))
}

/// Read the `lan_access_enabled` preference from `kikan_meta`.
///
/// Absent (never set) or unparseable values return `false` — the safe default
/// at M00 where desktop binds loopback-only and mDNS is a no-op.
pub async fn read_lan_access_enabled(db: &DatabaseConnection) -> Result<bool, AppError> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT value FROM kikan_meta WHERE key = ?",
            vec![Value::from(LAN_ACCESS_KEY.to_string())],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to read lan_access preference: {e}");
            AppError::InternalError("Failed to read LAN access preference".into())
        })?;

    let value: Option<String> = match row {
        Some(r) => r.try_get_by_index(0).map_err(|e| {
            tracing::error!("Failed to decode lan_access preference row: {e}");
            AppError::InternalError("Failed to read LAN access preference".into())
        })?,
        None => None,
    };

    Ok(value.map(|v| v == "true").unwrap_or(false))
}

pub async fn write_lan_access_enabled(
    db: &DatabaseConnection,
    enabled: bool,
) -> Result<(), AppError> {
    let value = if enabled { "true" } else { "false" };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO kikan_meta (key, value) VALUES (?, ?) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        vec![
            Value::from(LAN_ACCESS_KEY.to_string()),
            Value::from(value.to_string()),
        ],
    ))
    .await
    .map_err(|e| {
        tracing::error!("Failed to write lan_access preference: {e}");
        AppError::InternalError("Failed to write LAN access preference".into())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{ConnectionTrait, Database};

    async fn setup_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared(
            "CREATE TABLE kikan_meta (key TEXT PRIMARY KEY, value TEXT) WITHOUT ROWID",
        )
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn unset_preference_defaults_to_false() {
        let db = setup_db().await;
        assert!(!read_lan_access_enabled(&db).await.unwrap());
    }

    #[tokio::test]
    async fn write_then_read_true() {
        let db = setup_db().await;
        write_lan_access_enabled(&db, true).await.unwrap();
        assert!(read_lan_access_enabled(&db).await.unwrap());
    }

    #[tokio::test]
    async fn write_then_read_false() {
        let db = setup_db().await;
        write_lan_access_enabled(&db, true).await.unwrap();
        write_lan_access_enabled(&db, false).await.unwrap();
        assert!(!read_lan_access_enabled(&db).await.unwrap());
    }

    #[tokio::test]
    async fn repeated_writes_upsert() {
        let db = setup_db().await;
        for _ in 0..3 {
            write_lan_access_enabled(&db, true).await.unwrap();
        }
        assert!(read_lan_access_enabled(&db).await.unwrap());
    }
}
