use mokumo_core::activity::ActivityAction;
use mokumo_core::error::DomainError;
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;

/// Upsert the logo metadata row in shop_settings.
///
/// Commits `logo_extension` and `logo_epoch`, then inserts an activity
/// log entry in the same transaction.
pub async fn upsert_logo(
    db: &DatabaseConnection,
    ext: &str,
    updated_at: i64,
    actor_id: &str,
) -> Result<(), DomainError> {
    let txn = db.begin().await.map_err(crate::sea_err)?;

    txn.execute_raw(Statement::from_sql_and_values(
        sea_orm::DbBackend::Sqlite,
        "INSERT INTO shop_settings (id, shop_name, logo_extension, logo_epoch)
         VALUES (1, '', ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           logo_extension = excluded.logo_extension,
           logo_epoch = excluded.logo_epoch",
        vec![
            sea_orm::Value::from(ext.to_string()),
            sea_orm::Value::from(updated_at),
        ],
    ))
    .await
    .map_err(crate::sea_err)?;

    crate::activity::insert_activity_log_raw(
        &txn,
        "shop_settings",
        "1",
        ActivityAction::Updated,
        actor_id,
        "user",
        &json!({"action": "shop_logo_uploaded"}),
    )
    .await?;

    txn.commit().await.map_err(crate::sea_err)?;
    Ok(())
}

/// Null out the logo columns in shop_settings and log the removal.
pub async fn delete_logo(db: &DatabaseConnection, actor_id: &str) -> Result<(), DomainError> {
    let txn = db.begin().await.map_err(crate::sea_err)?;

    txn.execute_raw(Statement::from_sql_and_values(
        sea_orm::DbBackend::Sqlite,
        "UPDATE shop_settings
            SET logo_extension = NULL, logo_epoch = NULL
          WHERE id = 1",
        vec![],
    ))
    .await
    .map_err(crate::sea_err)?;

    crate::activity::insert_activity_log_raw(
        &txn,
        "shop_settings",
        "1",
        ActivityAction::Updated,
        actor_id,
        "user",
        &json!({"action": "shop_logo_deleted"}),
    )
    .await?;

    txn.commit().await.map_err(crate::sea_err)?;
    Ok(())
}

/// Fetch the logo extension and cache-buster timestamp from shop_settings.
///
/// Returns `None` if the singleton row does not exist or `logo_extension` is NULL.
pub async fn get_logo_info(db: &DatabaseConnection) -> Result<Option<(String, i64)>, DomainError> {
    let pool = db.get_sqlite_connection_pool();
    let row: Option<(Option<String>, Option<i64>)> =
        sqlx::query_as("SELECT logo_extension, logo_epoch FROM shop_settings WHERE id = 1")
            .fetch_optional(pool)
            .await
            .map_err(crate::db_err)?;

    match row {
        Some((Some(ext), Some(ts))) => Ok(Some((ext, ts))),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> (DatabaseConnection, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
        let db = crate::initialize_database(&url).await.unwrap();
        (db, tmp)
    }

    #[tokio::test]
    async fn upsert_and_get_logo_info() {
        let (db, _tmp) = test_db().await;
        upsert_logo(&db, "png", 1_000_000, "user-1").await.unwrap();
        let info = get_logo_info(&db).await.unwrap();
        assert!(info.is_some());
        let (ext, ts) = info.unwrap();
        assert_eq!(ext, "png");
        assert_eq!(ts, 1_000_000);
    }

    #[tokio::test]
    async fn upsert_extension_change_overwrites() {
        let (db, _tmp) = test_db().await;
        upsert_logo(&db, "png", 1_000_000, "user-1").await.unwrap();
        upsert_logo(&db, "jpeg", 2_000_000, "user-1").await.unwrap();
        let (ext, ts) = get_logo_info(&db).await.unwrap().unwrap();
        assert_eq!(ext, "jpeg");
        assert_eq!(ts, 2_000_000);
    }

    #[tokio::test]
    async fn delete_logo_clears_info() {
        let (db, _tmp) = test_db().await;
        upsert_logo(&db, "png", 1_000_000, "user-1").await.unwrap();
        delete_logo(&db, "user-1").await.unwrap();
        let info = get_logo_info(&db).await.unwrap();
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn get_logo_info_returns_none_when_no_logo() {
        let (db, _tmp) = test_db().await;
        let info = get_logo_info(&db).await.unwrap();
        assert!(info.is_none());
    }
}
