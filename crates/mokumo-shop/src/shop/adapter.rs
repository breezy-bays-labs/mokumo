//! SQLite adapter for `ShopLogoRepository`.
//!
//! Every mutation runs inside a SeaORM transaction and the activity-log
//! INSERT is delegated to the injected `kikan::ActivityWriter` on the same
//! transaction for atomicity — matches the customer-vertical pattern.

use std::sync::Arc;

use chrono::Utc;
use kikan::activity::{ActivityLogEntry, ActivityWriter};
use kikan::actor::Actor;
use kikan::error::ActivityWriteError;
use kikan::error::DomainError;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, Statement, TransactionTrait,
};
use serde_json::json;

use crate::activity::ActivityAction;
use crate::shop::ShopLogoRepository;
use crate::shop::domain::ShopLogoInfo;

fn sea_err(e: sea_orm::DbErr) -> DomainError {
    DomainError::Internal {
        message: e.to_string(),
    }
}

fn sqlx_err(e: sqlx::Error) -> DomainError {
    DomainError::Internal {
        message: e.to_string(),
    }
}

fn activity_err(e: ActivityWriteError) -> DomainError {
    DomainError::Internal {
        message: format!("activity log write failed: {e}"),
    }
}

pub struct SqliteShopLogoRepository {
    db: DatabaseConnection,
    activity_writer: Arc<dyn ActivityWriter>,
}

impl SqliteShopLogoRepository {
    pub fn new(db: DatabaseConnection, activity_writer: Arc<dyn ActivityWriter>) -> Self {
        Self {
            db,
            activity_writer,
        }
    }

    async fn log_activity(
        &self,
        tx: &DatabaseTransaction,
        actor: &Actor,
        action: ActivityAction,
        payload: serde_json::Value,
    ) -> Result<(), DomainError> {
        let entry = ActivityLogEntry {
            actor_id: Some(actor.id().to_string()),
            actor_type: actor.actor_type().to_string(),
            entity_kind: "shop_settings".to_string(),
            entity_id: "1".to_string(),
            action: action.as_str().to_string(),
            payload,
            occurred_at: Utc::now(),
        };
        self.activity_writer
            .log(tx, entry)
            .await
            .map_err(activity_err)
    }
}

impl ShopLogoRepository for SqliteShopLogoRepository {
    async fn get_logo_info(&self) -> Result<Option<ShopLogoInfo>, DomainError> {
        let pool = self.db.get_sqlite_connection_pool();
        let row: Option<(Option<String>, Option<i64>)> =
            sqlx::query_as("SELECT logo_extension, logo_epoch FROM shop_settings WHERE id = 1")
                .fetch_optional(pool)
                .await
                .map_err(sqlx_err)?;

        match row {
            Some((Some(extension), Some(updated_at))) => Ok(Some(ShopLogoInfo {
                extension,
                updated_at,
            })),
            _ => Ok(None),
        }
    }

    async fn upsert_logo(
        &self,
        extension: &str,
        updated_at: i64,
        actor: &Actor,
    ) -> Result<(), DomainError> {
        let tx = self.db.begin().await.map_err(sea_err)?;

        tx.execute_raw(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "INSERT INTO shop_settings (id, shop_name, logo_extension, logo_epoch)
             VALUES (1, '', ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               logo_extension = excluded.logo_extension,
               logo_epoch = excluded.logo_epoch",
            vec![
                sea_orm::Value::from(extension.to_string()),
                sea_orm::Value::from(updated_at),
            ],
        ))
        .await
        .map_err(sea_err)?;

        self.log_activity(
            &tx,
            actor,
            ActivityAction::Updated,
            json!({"action": "shop_logo_uploaded"}),
        )
        .await?;

        tx.commit().await.map_err(sea_err)?;
        Ok(())
    }

    async fn delete_logo(&self, actor: &Actor) -> Result<(), DomainError> {
        let tx = self.db.begin().await.map_err(sea_err)?;

        tx.execute_raw(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "UPDATE shop_settings
                SET logo_extension = NULL, logo_epoch = NULL
              WHERE id = 1",
            vec![],
        ))
        .await
        .map_err(sea_err)?;

        self.log_activity(
            &tx,
            actor,
            ActivityAction::SoftDeleted,
            json!({"action": "shop_logo_deleted"}),
        )
        .await?;

        tx.commit().await.map_err(sea_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kikan::SqliteActivityWriter;

    async fn test_db() -> (DatabaseConnection, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
        let db = crate::db::initialize_database(&url).await.unwrap();
        (db, tmp)
    }

    fn make_repo(db: DatabaseConnection) -> SqliteShopLogoRepository {
        SqliteShopLogoRepository::new(db, Arc::new(SqliteActivityWriter::new()))
    }

    #[tokio::test]
    async fn upsert_and_get_logo_info() {
        let (db, _tmp) = test_db().await;
        let repo = make_repo(db);
        repo.upsert_logo("png", 1_000_000, &Actor::user(1))
            .await
            .unwrap();
        let info = repo.get_logo_info().await.unwrap().unwrap();
        assert_eq!(info.extension, "png");
        assert_eq!(info.updated_at, 1_000_000);
    }

    #[tokio::test]
    async fn upsert_extension_change_overwrites() {
        let (db, _tmp) = test_db().await;
        let repo = make_repo(db);
        repo.upsert_logo("png", 1_000_000, &Actor::user(1))
            .await
            .unwrap();
        repo.upsert_logo("jpeg", 2_000_000, &Actor::user(1))
            .await
            .unwrap();
        let info = repo.get_logo_info().await.unwrap().unwrap();
        assert_eq!(info.extension, "jpeg");
        assert_eq!(info.updated_at, 2_000_000);
    }

    #[tokio::test]
    async fn delete_logo_clears_info() {
        let (db, _tmp) = test_db().await;
        let repo = make_repo(db);
        repo.upsert_logo("png", 1_000_000, &Actor::user(1))
            .await
            .unwrap();
        repo.delete_logo(&Actor::user(1)).await.unwrap();
        let info = repo.get_logo_info().await.unwrap();
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn get_logo_info_returns_none_when_no_logo() {
        let (db, _tmp) = test_db().await;
        let repo = make_repo(db);
        let info = repo.get_logo_info().await.unwrap();
        assert!(info.is_none());
    }
}
