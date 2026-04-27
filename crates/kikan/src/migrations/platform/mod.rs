mod active_integrations;
mod integration_event_log;
mod m_0001_create_meta_profiles;
mod m_0002_partition_legacy_history;
mod m_0003_create_meta_activity_log;
mod m_0004_create_legacy_upgrade_locks;
mod prevent_last_admin_deactivation;
mod profile_user_roles;
mod shop_settings;
mod users_and_roles;

use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::error::EngineError;
use crate::migrations::runner::{run_migrations, run_migrations_for_target};
use crate::migrations::{GraftId, Migration, MigrationTarget};

pub(crate) struct PlatformMigrations;

impl PlatformMigrations {
    pub(crate) fn migrations() -> Vec<Box<dyn Migration>> {
        vec![
            Box::new(m_0001_create_meta_profiles::CreateMetaProfiles),
            Box::new(m_0002_partition_legacy_history::PartitionLegacyHistory),
            Box::new(m_0003_create_meta_activity_log::CreateMetaActivityLog),
            Box::new(m_0004_create_legacy_upgrade_locks::CreateLegacyUpgradeLocks),
            Box::new(users_and_roles::UsersAndRoles),
            Box::new(shop_settings::ShopSettings),
            Box::new(profile_user_roles::ProfileUserRoles),
            Box::new(prevent_last_admin_deactivation::PreventLastAdminDeactivation),
            // `active_integrations` MUST precede `integration_event_log` —
            // the latter has a FK to the former.
            Box::new(active_integrations::ActiveIntegrations),
            Box::new(integration_event_log::IntegrationEventLog),
        ]
    }

    /// Graft id for the engine-platform migration set per
    /// `adr-kikan-upgrade-migration-strategy.md` §"Existing Mokumo
    /// migrations partition". Distinct from the bootstrap graft id
    /// (`kikan`) which marks the two unconditional history-table
    /// bootstrap migrations.
    pub(crate) fn graft_id() -> GraftId {
        GraftId::new("kikan::engine")
    }

    /// Names of every platform migration. Used by
    /// `kikan::db::check_schema_compatibility` to recognise platform-owned
    /// migration rows in legacy `seaql_migrations` tables (pre-PR-A
    /// fixtures applied `users_and_roles` and `shop_settings` per-profile;
    /// after PR-A those moved to platform ownership). Without this union,
    /// a vertical migrator would treat platform-owned rows as orphaned
    /// future migrations and refuse to boot.
    pub(crate) fn migration_names() -> Vec<&'static str> {
        Self::migrations().iter().map(|m| m.name()).collect()
    }
}

fn arc_migrations() -> Vec<Arc<dyn Migration>> {
    PlatformMigrations::migrations()
        .into_iter()
        .map(Arc::from)
        .collect()
}

/// Run kikan's full platform migration set against a single pool. Used by
/// vertical crates' `initialize_database()` helpers in tests and dev paths
/// that operate against one combined database. Production routes through
/// `run_platform_meta_migrations` and `run_platform_per_profile_migrations`
/// because Meta and PerProfile migrations land on different pools.
pub async fn run_platform_migrations(pool: &DatabaseConnection) -> Result<(), EngineError> {
    run_migrations(pool, &arc_migrations()).await
}

/// Run only the Meta-target migrations from the platform set against the
/// given pool. Used by vertical-agnostic init paths that own a meta.db pool
/// directly (e.g. CLI bootstrap before the engine boots).
pub async fn run_platform_meta_migrations(
    meta_pool: &DatabaseConnection,
) -> Result<(), EngineError> {
    run_migrations_for_target(meta_pool, &arc_migrations(), MigrationTarget::Meta).await
}

/// Run only the PerProfile-target migrations from the platform set against
/// the given pool. Used by vertical init paths that need platform tables
/// (`shop_settings`, etc.) on a per-profile pool.
pub async fn run_platform_per_profile_migrations(
    per_profile_pool: &DatabaseConnection,
) -> Result<(), EngineError> {
    run_migrations_for_target(
        per_profile_pool,
        &arc_migrations(),
        MigrationTarget::PerProfile,
    )
    .await
}
