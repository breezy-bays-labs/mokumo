//! Legacy install upgrade — silently records a `production/` install in
//! `meta.profiles` so the runtime registry knows about it.
//!
//! # Scope (PR A — Meta-only)
//!
//! This handler runs once per install when [`detect_boot_state`] returns
//! [`BootState::LegacyCompleted`]. It:
//!
//! 1. Derives a kebab-case slug from the legacy `shop_settings.shop_name`.
//! 2. INSERTs a row into `meta.profiles` and a row into `meta.activity_log`
//!    inside a single transaction on the meta DB.
//!
//! It does **not** rename the `production/` directory and it does **not**
//! update the `<data_dir>/active_profile` pointer. The binary's
//! `prepare_database` and the engine's pool map continue to address the
//! legacy install as `production` until PR B refactors those call sites
//! to consult `meta.profiles`. `meta.profiles` is therefore "shadow truth"
//! in PR A and becomes "physical truth" in PR B.
//!
//! Idempotency is provided by the caller, not this function: on the next
//! boot, `meta.profiles` will have one row, so [`detect_boot_state`]
//! returns [`BootState::PostUpgradeOrSetup`] and the upgrade arm is never
//! re-entered.
//!
//! # Audit
//!
//! The activity log entry uses [`ActivityAction::LegacyUpgradeMigrated`]
//! and lands in `meta.activity_log` (created by
//! `m_0003_create_meta_activity_log`). Its payload carries the original
//! `shop_name` and the legacy vertical DB path so an operator can correlate
//! the audit row with the on-disk layout.
//!
//! [`detect_boot_state`]: crate::meta::detect_boot_state
//! [`BootState::LegacyCompleted`]: crate::meta::BootState::LegacyCompleted
//! [`BootState::PostUpgradeOrSetup`]: crate::meta::BootState::PostUpgradeOrSetup
//! [`ActivityAction::LegacyUpgradeMigrated`]: kikan_types::activity::ActivityAction::LegacyUpgradeMigrated

use std::path::Path;

use kikan_types::activity::ActivityAction;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::json;
use thiserror::Error;

use crate::activity::insert_activity_log_raw;
use crate::slug::{Slug, SlugError, derive_slug};

/// Errors surfaced by [`run_legacy_upgrade`].
#[derive(Debug, Error)]
pub enum UpgradeError {
    /// `derive_slug(shop_name)` rejected the input. Wraps the specific
    /// rule violated (Empty / Reserved / TooLong / Unparseable).
    #[error("legacy upgrade rejected shop_name `{shop_name}`: {source}")]
    SlugDerivation {
        shop_name: String,
        #[source]
        source: SlugError,
    },

    /// SeaORM error while writing meta.profiles or meta.activity_log, or
    /// while opening / committing the transaction.
    #[error("legacy upgrade DB error: {0}")]
    Db(#[from] sea_orm::DbErr),

    /// `insert_activity_log_raw` failed (e.g. payload serialization). Bubbles
    /// up the underlying domain error so the caller sees the same surface as
    /// other activity-log call sites.
    #[error("legacy upgrade activity-log write failed: {0}")]
    ActivityLog(#[from] crate::error::DomainError),
}

/// Outcome of a successful upgrade — exposed so the caller can log the
/// derived slug without re-deriving it.
#[derive(Debug, Clone)]
pub struct UpgradeOutcome {
    pub slug: Slug,
}

/// Insert a `meta.profiles` row + a `meta.activity_log` audit entry for a
/// legacy `production/` install detected at boot.
///
/// `kind` is the production-equivalent profile-kind string (the caller
/// reads it from `Graft::auth_profile_kind`'s `Display`). Kikan stores it
/// opaquely; the vertical owns the vocabulary.
pub async fn run_legacy_upgrade(
    meta_db: &DatabaseConnection,
    shop_name: &str,
    vertical_db_path: &Path,
    kind: &str,
) -> Result<UpgradeOutcome, UpgradeError> {
    let slug = derive_slug(shop_name).map_err(|source| UpgradeError::SlugDerivation {
        shop_name: shop_name.to_owned(),
        source,
    })?;

    let txn = meta_db.begin().await?;

    txn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO profiles (slug, display_name, kind) VALUES (?, ?, ?)",
        [slug.as_str().into(), shop_name.into(), kind.into()],
    ))
    .await?;

    let payload = json!({
        "shop_name": shop_name,
        "vertical_db_path": vertical_db_path.display().to_string(),
        "kind": kind,
    });
    insert_activity_log_raw(
        &txn,
        "profile",
        slug.as_str(),
        ActivityAction::LegacyUpgradeMigrated,
        "system",
        "system",
        &payload,
    )
    .await?;

    txn.commit().await?;

    Ok(UpgradeOutcome { slug })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations::platform::run_platform_meta_migrations;
    use sea_orm::Database;

    async fn meta_pool() -> DatabaseConnection {
        let pool = Database::connect("sqlite::memory:").await.unwrap();
        run_platform_meta_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn happy_path_inserts_profile_row_with_derived_slug() {
        let pool = meta_pool().await;
        let outcome = run_legacy_upgrade(
            &pool,
            "Acme Printing",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap();
        assert_eq!(outcome.slug.as_str(), "acme-printing");

        let rows: Vec<(String, String, String)> = pool
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT slug, display_name, kind FROM profiles",
            ))
            .await
            .unwrap()
            .into_iter()
            .map(|r| {
                (
                    r.try_get_by_index::<String>(0).unwrap(),
                    r.try_get_by_index::<String>(1).unwrap(),
                    r.try_get_by_index::<String>(2).unwrap(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![(
                "acme-printing".to_string(),
                "Acme Printing".to_string(),
                "production".to_string(),
            )]
        );
    }

    #[tokio::test]
    async fn happy_path_writes_activity_log_entry() {
        let pool = meta_pool().await;
        run_legacy_upgrade(
            &pool,
            "Acme Printing",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap();

        let row = pool
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT entity_type, entity_id, action, actor_id, actor_type, payload \
                 FROM activity_log",
            ))
            .await
            .unwrap()
            .expect("expected an activity_log row");
        assert_eq!(row.try_get_by_index::<String>(0).unwrap(), "profile");
        assert_eq!(row.try_get_by_index::<String>(1).unwrap(), "acme-printing");
        assert_eq!(
            row.try_get_by_index::<String>(2).unwrap(),
            "legacy_upgrade_migrated"
        );
        assert_eq!(row.try_get_by_index::<String>(3).unwrap(), "system");
        assert_eq!(row.try_get_by_index::<String>(4).unwrap(), "system");
        let payload: serde_json::Value =
            serde_json::from_str(&row.try_get_by_index::<String>(5).unwrap()).unwrap();
        assert_eq!(payload["shop_name"], "Acme Printing");
        assert_eq!(payload["vertical_db_path"], "/data/production/mokumo.db");
        assert_eq!(payload["kind"], "production");
    }

    #[tokio::test]
    async fn unparseable_shop_name_returns_slug_derivation_error() {
        let pool = meta_pool().await;
        let err = run_legacy_upgrade(
            &pool,
            "!!!",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            UpgradeError::SlugDerivation {
                ref shop_name,
                source: SlugError::Unparseable { .. },
            } if shop_name == "!!!"
        ));
        // Transaction never opened on slug failure → no profile row.
        let count: i64 = pool
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) FROM profiles",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index(0)
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn reserved_slug_returns_slug_derivation_error() {
        let pool = meta_pool().await;
        let err = run_legacy_upgrade(
            &pool,
            "Demo",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            UpgradeError::SlugDerivation {
                source: SlugError::Reserved(ref s),
                ..
            } if s == "demo"
        ));
    }
}
