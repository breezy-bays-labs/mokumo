mod shop_settings;
mod users_and_roles;

use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::error::EngineError;
use crate::migrations::Migration;
use crate::migrations::bootstrap::BootstrapMigrations;
use crate::migrations::runner::run_migrations;

pub(crate) struct PlatformMigrations;

impl PlatformMigrations {
    pub(crate) fn migrations() -> Vec<Box<dyn Migration>> {
        vec![
            Box::new(users_and_roles::UsersAndRoles),
            Box::new(shop_settings::ShopSettings),
        ]
    }

    pub(crate) fn graft_id() -> crate::migrations::GraftId {
        BootstrapMigrations::graft_id()
    }
}

/// Run kikan's platform migrations (users, roles, shop_settings) against
/// the given database connection.
///
/// This is a convenience for vertical crates whose `initialize_database()`
/// helpers need the platform schema in place before running their own
/// SeaORM migrations. In production, the engine's DAG runner handles
/// ordering; this function is the equivalent for test/dev paths that
/// bypass the engine.
pub async fn run_platform_migrations(pool: &DatabaseConnection) -> Result<(), EngineError> {
    let migrations: Vec<Arc<dyn Migration>> = PlatformMigrations::migrations()
        .into_iter()
        .map(Arc::from)
        .collect();
    run_migrations(pool, &migrations).await
}
