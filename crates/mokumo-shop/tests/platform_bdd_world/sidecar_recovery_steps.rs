use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use cucumber::{given, then, when};
use kikan::{EngineError, Graft as _};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use tokio_util::sync::CancellationToken;

use super::PlatformBddWorld;

const VERTICAL_DB_FILE: &str = "mokumo.db";

/// Process-wide guard around `MOKUMO_DEMO_SIDECAR`. cucumber-rs runs
/// scenarios concurrently by default, so two scenarios in this feature
/// would otherwise race on the env var: B's `Drop` could unset the var
/// after A's `Given` set it but before A's `When` reads it. This mutex
/// serializes any scenario that mutates the env var for its full
/// lifetime (acquired in the `Given`, released when the ctx drops).
fn sidecar_env_lock() -> &'static parking_lot::Mutex<()> {
    static LOCK: std::sync::OnceLock<parking_lot::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| parking_lot::Mutex::new(()))
}

pub struct SidecarRecoveryCtx {
    pub data_dir: tempfile::TempDir,
    #[allow(dead_code)]
    pub sidecar_path: PathBuf,
    /// Cloned before `Engine::boot` consumes the original so post-boot
    /// assertions can query `meta.activity_log` and the recoveries map.
    pub meta_pool: Option<DatabaseConnection>,
    pub recoveries: Option<std::collections::HashMap<String, kikan::SidecarRecoveryDiagnostic>>,
    pub boot_result: Option<Result<(), EngineError>>,
    /// Held for the lifetime of the scenario; dropped together with the
    /// env-var reset so the lock is released only after this scenario's
    /// stale path is gone.
    _env_guard: parking_lot::MutexGuard<'static, ()>,
}

// Cucumber's `World` derive needs `Debug`; `MutexGuard` doesn't impl it.
impl std::fmt::Debug for SidecarRecoveryCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SidecarRecoveryCtx")
            .field("data_dir", &self.data_dir.path())
            .field("sidecar_path", &self.sidecar_path)
            .field("recoveries", &self.recoveries)
            .field("boot_result", &self.boot_result)
            .finish()
    }
}

impl Drop for SidecarRecoveryCtx {
    fn drop(&mut self) {
        // Unset the env var BEFORE releasing the lock so a queued
        // scenario can't observe this scenario's now-deleted TempDir
        // path. SAFETY: while `_env_guard` is held no other
        // sidecar-recovery scenario is touching the env var.
        unsafe {
            std::env::remove_var("MOKUMO_DEMO_SIDECAR");
        }
    }
}

fn write_kikan_seed_db(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let conn = rusqlite::Connection::open(path).unwrap();
    // Force a real header + page write so the file is non-empty.
    // app_id = 0 (not-yet-stamped) is a valid kikan db per
    // `kikan::db::check_application_id` — the schema content does not
    // matter to the recovery hook, only that the file exists and is a
    // valid sqlite db.
    conn.execute_batch("CREATE TABLE __seed (id INTEGER PRIMARY KEY);")
        .unwrap();
    drop(conn);
}

#[given("a fresh data directory with a bundled demo sidecar but no demo database file")]
async fn given_sidecar_present_db_missing(w: &mut PlatformBddWorld) {
    // Drop any in-flight previous ctx (and its env-var guard) BEFORE
    // acquiring the lock again — otherwise we'd deadlock with our own
    // World's prior scenario remnant.
    w.sidecar_recovery = None;
    let env_guard = sidecar_env_lock().lock();
    let dir = tempfile::tempdir().unwrap();
    let sidecar = dir.path().join("seed-demo.db");
    write_kikan_seed_db(&sidecar);
    // SAFETY: the env-var lock above ensures no other sidecar-recovery
    // scenario in this process is reading or writing the var.
    unsafe {
        std::env::set_var("MOKUMO_DEMO_SIDECAR", &sidecar);
    }
    w.sidecar_recovery = Some(SidecarRecoveryCtx {
        data_dir: dir,
        sidecar_path: sidecar,
        meta_pool: None,
        recoveries: None,
        boot_result: None,
        _env_guard: env_guard,
    });
}

#[given("a fresh data directory with a bundled demo sidecar and a healthy demo database file")]
async fn given_sidecar_and_healthy_db(w: &mut PlatformBddWorld) {
    w.sidecar_recovery = None;
    let env_guard = sidecar_env_lock().lock();
    let dir = tempfile::tempdir().unwrap();
    let sidecar = dir.path().join("seed-demo.db");
    write_kikan_seed_db(&sidecar);
    // Pre-place a healthy db at the destination so the hook short-circuits
    // to NotNeeded.
    let dest = dir.path().join("demo").join(VERTICAL_DB_FILE);
    write_kikan_seed_db(&dest);
    unsafe {
        std::env::set_var("MOKUMO_DEMO_SIDECAR", &sidecar);
    }
    w.sidecar_recovery = Some(SidecarRecoveryCtx {
        data_dir: dir,
        sidecar_path: sidecar,
        meta_pool: None,
        recoveries: None,
        boot_result: None,
        _env_guard: env_guard,
    });
}

#[when("the engine boots from the fresh data directory")]
async fn when_engine_boots_fresh(w: &mut PlatformBddWorld) {
    use kikan::tenancy::ProfileDirName;
    use kikan_types::SetupMode;

    let ctx = w.sidecar_recovery.as_mut().unwrap();
    let data_dir = ctx.data_dir.path().to_path_buf();

    let meta_db = Database::connect("sqlite::memory:").await.unwrap();
    let demo_db = mokumo_shop::db::initialize_database("sqlite::memory:")
        .await
        .unwrap();
    let production_db = mokumo_shop::db::initialize_database("sqlite::memory:")
        .await
        .unwrap();

    let session_pool = production_db.get_sqlite_connection_pool().clone();
    let session_store = tower_sessions_sqlx_store::SqliteStore::new(session_pool);
    session_store.migrate().await.unwrap();

    ctx.meta_pool = Some(meta_db.clone());

    let mut pools = std::collections::HashMap::with_capacity(2);
    pools.insert(ProfileDirName::from(SetupMode::Demo.as_dir_name()), demo_db);
    pools.insert(
        ProfileDirName::from(SetupMode::Production.as_dir_name()),
        production_db,
    );
    let active_profile = ProfileDirName::from(SetupMode::Production.as_dir_name());

    let recovery_dir = data_dir.join("recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();

    let graft = mokumo_shop::graft::MokumoApp::new(None).with_recovery_dir(recovery_dir);
    let profile_initializer: kikan::platform_state::SharedProfileDbInitializer =
        Arc::new(mokumo_shop::profile_db_init::MokumoProfileDbInitializer);
    let boot_config = kikan::BootConfig::new(data_dir);

    let result = kikan::Engine::<mokumo_shop::graft::MokumoApp>::boot(
        boot_config,
        &graft,
        meta_db,
        pools,
        active_profile,
        session_store,
        profile_initializer,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(true)),
        CancellationToken::new(),
    )
    .await;

    ctx.recoveries = result.as_ref().ok().map(|(_engine, app_state)| {
        let platform = mokumo_shop::graft::MokumoApp::platform_state(app_state);
        let map = platform.sidecar_recoveries.read();
        map.iter()
            .map(|(k, v)| (k.as_str().to_string(), v.clone()))
            .collect()
    });
    ctx.boot_result = Some(result.map(|_| ()));
}

#[then("the demo database file exists")]
async fn then_demo_db_exists(w: &mut PlatformBddWorld) {
    let ctx = w.sidecar_recovery.as_ref().unwrap();
    let dest = ctx.data_dir.path().join("demo").join(VERTICAL_DB_FILE);
    assert!(
        dest.exists(),
        "expected {} to exist after sidecar recovery, but it does not",
        dest.display()
    );
}

#[then(expr = "PlatformState reports a sidecar recovery for {string}")]
async fn then_recovery_reported(w: &mut PlatformBddWorld, profile_dir: String) {
    let ctx = w.sidecar_recovery.as_ref().unwrap();
    ctx.boot_result
        .as_ref()
        .expect("boot was invoked")
        .as_ref()
        .expect("boot succeeded");
    let recoveries = ctx
        .recoveries
        .as_ref()
        .expect("recoveries map populated on boot success");
    let entry = recoveries
        .get(&profile_dir)
        .unwrap_or_else(|| panic!("no sidecar recovery for `{profile_dir}`"));
    assert!(
        entry.bytes > 0,
        "recovery for `{profile_dir}` reported zero bytes"
    );
    assert!(
        entry
            .dest
            .ends_with(format!("{profile_dir}/{VERTICAL_DB_FILE}").as_str()),
        "dest path {} does not end with {profile_dir}/{VERTICAL_DB_FILE}",
        entry.dest.display()
    );
}

#[then("PlatformState reports no sidecar recoveries")]
async fn then_no_recoveries(w: &mut PlatformBddWorld) {
    let ctx = w.sidecar_recovery.as_ref().unwrap();
    let recoveries = ctx
        .recoveries
        .as_ref()
        .expect("recoveries map populated on boot success");
    assert!(
        recoveries.is_empty(),
        "expected empty recoveries, got {:?}",
        recoveries.keys().collect::<Vec<_>>()
    );
}

#[then(expr = "meta.activity_log has a profile_sidecar_recovered entry for {string}")]
async fn then_activity_log_entry(w: &mut PlatformBddWorld, expected_profile: String) {
    let ctx = w.sidecar_recovery.as_ref().unwrap();
    let meta = ctx.meta_pool.as_ref().expect("meta pool was captured");
    let row = meta
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_type, entity_id, action FROM activity_log \
             WHERE entity_id = ? AND action = 'profile_sidecar_recovered'",
            [expected_profile.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("no profile_sidecar_recovered entry for `{expected_profile}`"));
    let entity_type: String = row.try_get_by_index(0).unwrap();
    let entity_id: String = row.try_get_by_index(1).unwrap();
    let action: String = row.try_get_by_index(2).unwrap();
    assert_eq!(entity_type, "profile");
    assert_eq!(entity_id, expected_profile);
    assert_eq!(action, "profile_sidecar_recovered");
}
