pub mod activity;
pub mod meta;
pub mod role;

pub use sea_orm::DatabaseConnection;

// Stage 3 (#507): platform primitives lifted to `kikan::db` and
// `kikan::backup`. These re-exports keep existing tests (and the
// migration/bdd/restore step definitions under `crates/db/tests/`)
// compiling against `mokumo_db::*` while services/api migrates its call
// sites to the kikan paths directly. Both `crates/db` and these
// re-exports dissolve in S3.1b when the crate is removed.
pub use kikan::backup::{
    BackupError, BackupResult, RestoreResult, build_timestamped_name, collect_existing_backups,
    create_backup, pre_migration_backup, restore_from_backup, verify_integrity,
};
pub use kikan::db::{
    CONFIGURED_MMAP_SIZE, DBERRCOMPAT_PATTERN, DatabaseSetupError, DbDiagnostics,
    DbRuntimeDiagnostics, KIKAN_APPLICATION_ID as MOKUMO_APPLICATION_ID, check_application_id,
    diagnose_database, ensure_auto_vacuum, health_check, initialize_database as initialize_pool,
    open_raw_sqlite_pool, read_db_runtime_diagnostics, validate_installation,
};

/// Re-export of [`kikan::backup`] under the pre-Stage-3 path
/// (`mokumo_db::backup`) so call sites using `mokumo_db::backup::...`
/// resolve during the transition. New code should import from
/// [`kikan::backup`] directly.
pub use kikan::backup;
