use cucumber::World;
use sea_orm::DatabaseConnection;
use sqlx::SqlitePool;

mod bundle_backup_steps;
mod install_validation_steps;
mod legacy_refuse_boot_steps;
mod migration_safety_steps;
mod restore_steps;
mod sidecar_recovery_steps;
mod storage_diagnostics_steps;

#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct PlatformBddWorld {
    #[allow(dead_code)]
    db: DatabaseConnection,
    #[allow(dead_code)]
    pool: SqlitePool,
    _tmp: tempfile::TempDir,
    // Migration safety scenario state
    ms_tmp: Option<tempfile::TempDir>,
    ms_db_path: Option<std::path::PathBuf>,
    ms_backup_path: Option<std::path::PathBuf>,
    ms_oldest_backup: Option<std::path::PathBuf>,
    ms_source_seaql_count: Option<i64>,
    ms_backup_seaql_before_upgrade: Option<i64>,
    ms_migration_failed: bool,
    ms_table_count_before: Option<i64>,
    // Install validation test state
    pub last_validation_result: Option<bool>,
    // Storage diagnostics test state
    pub db_path: std::path::PathBuf,
    pub last_db_diagnostics: Option<Result<kikan::db::DbDiagnostics, rusqlite::Error>>,
    pub known_wal_size: Option<u64>,
    // Restore step state
    pub restore_tmp: Option<tempfile::TempDir>,
    pub restore_candidate_path: Option<std::path::PathBuf>,
    pub restore_validate_result:
        Option<Result<mokumo_shop::restore::CandidateInfo, mokumo_shop::restore::RestoreError>>,
    pub restore_copy_result: Option<Result<(), mokumo_shop::restore::RestoreError>>,
    pub restore_production_dir: Option<std::path::PathBuf>,
    // Legacy-install refusal scenario state
    pub legacy_refuse: Option<legacy_refuse_boot_steps::LegacyRefuseCtx>,
    // Sidecar recovery scenario state
    pub sidecar_recovery: Option<sidecar_recovery_steps::SidecarRecoveryCtx>,
    // Bundle backup / restore scenario state
    pub bundle_backup: Option<bundle_backup_steps::BundleBackupCtx>,
}

impl PlatformBddWorld {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = tmp.path().join("test.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let db = mokumo_shop::db::initialize_database(&database_url)
            .await
            .expect("failed to initialize database");
        let pool = db.get_sqlite_connection_pool().clone();
        Self {
            db,
            pool,
            _tmp: tmp,
            ms_tmp: None,
            ms_db_path: None,
            ms_backup_path: None,
            ms_oldest_backup: None,
            ms_source_seaql_count: None,
            ms_backup_seaql_before_upgrade: None,
            ms_migration_failed: false,
            ms_table_count_before: None,
            last_validation_result: None,
            db_path,
            last_db_diagnostics: None,
            known_wal_size: None,
            restore_tmp: None,
            restore_candidate_path: None,
            restore_validate_result: None,
            restore_copy_result: None,
            restore_production_dir: None,
            legacy_refuse: None,
            sidecar_recovery: None,
            bundle_backup: None,
        }
    }
}
