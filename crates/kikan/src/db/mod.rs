//! Platform-side database startup primitives.
//!
//! - [`pragmas`] — SQLite PRAGMA configuration applied to every
//!   kikan-managed pool.
//! - [`application_id`] — `PRAGMA application_id` constant
//!   ([`KIKAN_APPLICATION_ID`]) and [`check_application_id`] guard.
//! - [`init`] — [`initialize_database`] (pool-only, no migrations),
//!   [`check_schema_compatibility`] (generic over `MigratorTrait`),
//!   [`ensure_auto_vacuum`], [`open_raw_sqlite_pool`], and
//!   [`DatabaseSetupError`].
//!
//! Migrations are the vertical's responsibility per the Migration
//! Ownership golden rule — this module never runs `MigratorTrait::up`.

pub mod application_id;
pub mod diagnostics;
pub mod init;
pub mod meta;
pub mod pragmas;

pub use application_id::{KIKAN_APPLICATION_ID, check_application_id};
pub use diagnostics::{
    DbDiagnostics, DbRuntimeDiagnostics, diagnose_database, health_check,
    read_db_runtime_diagnostics, validate_installation,
};
pub use init::{
    DBERRCOMPAT_PATTERN, DatabaseConnection, DatabaseSetupError, check_schema_compatibility,
    ensure_auto_vacuum, initialize_database, log_user_version, open_raw_sqlite_pool,
    post_migration_optimize,
};
pub use pragmas::CONFIGURED_MMAP_SIZE;
