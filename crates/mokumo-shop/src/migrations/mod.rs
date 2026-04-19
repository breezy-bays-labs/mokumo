pub mod m20260321_000000_init;
pub mod m20260322_000000_settings;
pub mod m20260324_000000_number_sequences;
pub mod m20260324_000001_customers_and_activity;
pub mod m20260326_000000_customers_deleted_at_index;
pub mod m20260404_000000_set_pragmas;
pub mod m20260416_000000_login_lockout;
pub mod m20260418_000000_activity_log_composite_index;

use sea_orm_migration::prelude::*;

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260321_000000_init::Migration),
            Box::new(m20260322_000000_settings::Migration),
            Box::new(m20260324_000000_number_sequences::Migration),
            Box::new(m20260324_000001_customers_and_activity::Migration),
            Box::new(m20260326_000000_customers_deleted_at_index::Migration),
            Box::new(m20260404_000000_set_pragmas::Migration),
            Box::new(m20260416_000000_login_lockout::Migration),
            Box::new(m20260418_000000_activity_log_composite_index::Migration),
        ]
    }
}
