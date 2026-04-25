use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use cucumber::{given, then, when};
use kikan::EngineError;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use tokio_util::sync::CancellationToken;

use super::PlatformBddWorld;

const VERTICAL_DB_FILE: &str = "mokumo.db";

#[derive(Debug)]
pub struct LegacyRefuseCtx {
    pub data_dir: tempfile::TempDir,
    pub vertical_db_path: PathBuf,
    /// Cloned before `Engine::boot` consumes the original so post-boot
    /// assertions can query `meta.profiles` and `meta.activity_log`.
    pub meta_pool: Option<DatabaseConnection>,
    pub boot_result: Option<Result<(), EngineError>>,
}

fn seed_legacy_vertical(production: &std::path::Path, admin: bool, shop_name: &str) -> PathBuf {
    std::fs::create_dir_all(production).expect("mkdir production");
    let vertical = production.join(VERTICAL_DB_FILE);
    let conn = rusqlite::Connection::open(&vertical).expect("open legacy vertical db");
    conn.execute_batch(
        "CREATE TABLE roles (id INTEGER PRIMARY KEY, name TEXT);
         INSERT INTO roles (id, name) VALUES (1, 'Admin');
         CREATE TABLE users (
             id INTEGER PRIMARY KEY,
             role_id INTEGER NOT NULL,
             is_active INTEGER NOT NULL DEFAULT 1,
             deleted_at TEXT
         );
         CREATE TABLE shop_settings (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             shop_name TEXT NOT NULL DEFAULT ''
         );",
    )
    .expect("seed legacy vertical schema");
    conn.execute(
        "INSERT INTO shop_settings (id, shop_name) VALUES (1, ?1)",
        rusqlite::params![shop_name],
    )
    .expect("seed shop_settings row");
    if admin {
        conn.execute(
            "INSERT INTO users (role_id, is_active, deleted_at) VALUES (1, 1, NULL)",
            [],
        )
        .expect("seed admin user");
    }
    vertical
}

fn install_ctx(w: &mut PlatformBddWorld, dir: tempfile::TempDir, vertical: PathBuf) {
    w.legacy_refuse = Some(LegacyRefuseCtx {
        data_dir: dir,
        vertical_db_path: vertical,
        meta_pool: None,
        boot_result: None,
    });
}

#[given("a legacy production database with an admin user and an empty shop_name")]
async fn given_legacy_empty_shop_name(w: &mut PlatformBddWorld) {
    let dir = tempfile::tempdir().unwrap();
    let vertical = seed_legacy_vertical(&dir.path().join("production"), true, "");
    install_ctx(w, dir, vertical);
}

#[given(expr = "a legacy production database with an admin user and shop_name {string}")]
async fn given_legacy_with_shop_name(w: &mut PlatformBddWorld, shop_name: String) {
    let dir = tempfile::tempdir().unwrap();
    let vertical = seed_legacy_vertical(&dir.path().join("production"), true, &shop_name);
    install_ctx(w, dir, vertical);
}

#[given("a legacy production folder with a vertical DB that has no admin user")]
async fn given_legacy_no_admin(w: &mut PlatformBddWorld) {
    let dir = tempfile::tempdir().unwrap();
    let vertical = seed_legacy_vertical(&dir.path().join("production"), false, "Acme Printing");
    install_ctx(w, dir, vertical);
}

#[when("the engine boots")]
async fn when_engine_boots(w: &mut PlatformBddWorld) {
    use kikan::tenancy::ProfileDirName;
    use kikan_types::SetupMode;

    let ctx = w.legacy_refuse.as_mut().unwrap();
    let data_dir = ctx.data_dir.path().to_path_buf();

    // Bare pool — `Engine::boot` runs Meta migrations on it.
    let meta_db = Database::connect("sqlite::memory:").await.unwrap();
    // Per-profile pools must mirror what the binary's `prepare_database`
    // produces: kikan platform migrations + vertical SeaORM migrations
    // already applied. The kikan-side migration tracker (`kikan_migrations`)
    // is populated, so `Engine::boot::run_per_profile_migrations` finds
    // the PerProfile-targeted migrations already applied and is a no-op.
    // Without this, vertical migrations like `m20260416_000000_login_lockout`
    // (which ALTERs the platform `users` table) fail because they expect
    // pre-existing schema that the binary normally creates.
    let demo_db = mokumo_shop::db::initialize_database("sqlite::memory:")
        .await
        .unwrap();
    let production_db = mokumo_shop::db::initialize_database("sqlite::memory:")
        .await
        .unwrap();

    let session_pool = production_db.get_sqlite_connection_pool().clone();
    let session_store = tower_sessions_sqlx_store::SqliteStore::new(session_pool);
    session_store.migrate().await.unwrap();

    // Clone the meta pool so post-boot assertions can read it. SeaORM's
    // `DatabaseConnection` is internally Arc-wrapped (O(1) clone), and
    // both clones reference the same underlying SQLite connection pool.
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
    .await
    .map(|_| ());
    ctx.boot_result = Some(result);
}

#[then("the engine refuses to boot with DefensiveEmptyShopName pointing at the production db")]
async fn then_refused_with_defensive_empty(w: &mut PlatformBddWorld) {
    let ctx = w.legacy_refuse.as_ref().unwrap();
    let result = ctx.boot_result.as_ref().expect("boot was invoked");
    let err = result
        .as_ref()
        .expect_err("expected Engine::boot to fail with DefensiveEmptyShopName");
    let EngineError::DefensiveEmptyShopName { path } = err else {
        panic!("expected DefensiveEmptyShopName, got {err:?}");
    };
    assert_eq!(path, &ctx.vertical_db_path);
}

#[then("the engine refuses to boot because the legacy shop_name is unparseable")]
async fn then_refused_unparseable(w: &mut PlatformBddWorld) {
    use kikan::meta::UpgradeError;
    use kikan::slug::SlugError;

    let ctx = w.legacy_refuse.as_ref().unwrap();
    let err = ctx
        .boot_result
        .as_ref()
        .expect("boot was invoked")
        .as_ref()
        .expect_err("expected Engine::boot to fail with LegacyUpgrade(Unparseable)");
    let EngineError::LegacyUpgrade(UpgradeError::SlugDerivation { source, .. }) = err else {
        panic!("expected LegacyUpgrade(SlugDerivation), got {err:?}");
    };
    assert!(
        matches!(source, SlugError::Unparseable { .. }),
        "expected Unparseable, got {source:?}"
    );
}

#[then(expr = "the engine refuses to boot because the derived slug is reserved {string}")]
async fn then_refused_reserved(w: &mut PlatformBddWorld, expected: String) {
    use kikan::meta::UpgradeError;
    use kikan::slug::SlugError;

    let ctx = w.legacy_refuse.as_ref().unwrap();
    let err = ctx
        .boot_result
        .as_ref()
        .expect("boot was invoked")
        .as_ref()
        .expect_err("expected Engine::boot to fail with LegacyUpgrade(Reserved)");
    let EngineError::LegacyUpgrade(UpgradeError::SlugDerivation { source, .. }) = err else {
        panic!("expected LegacyUpgrade(SlugDerivation), got {err:?}");
    };
    let SlugError::Reserved(name) = source else {
        panic!("expected Reserved, got {source:?}");
    };
    assert_eq!(name, &expected);
}

#[then("the engine boots successfully")]
async fn then_boots_successfully(w: &mut PlatformBddWorld) {
    let ctx = w.legacy_refuse.as_ref().unwrap();
    let result = ctx.boot_result.as_ref().expect("boot was invoked");
    if let Err(err) = result {
        panic!("expected Engine::boot to succeed, got {err:?}");
    }
}

#[then(expr = "meta.profiles has a row with slug {string} and display_name {string}")]
async fn then_meta_profiles_has_row(
    w: &mut PlatformBddWorld,
    expected_slug: String,
    expected_display_name: String,
) {
    let ctx = w.legacy_refuse.as_ref().unwrap();
    let pool = ctx.meta_pool.as_ref().expect("meta_pool retained");
    let row = pool
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT slug, display_name, kind FROM profiles WHERE slug = ?",
            [expected_slug.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("no meta.profiles row for slug `{expected_slug}`"));
    assert_eq!(
        row.try_get_by_index::<String>(0).unwrap(),
        expected_slug,
        "slug mismatch"
    );
    assert_eq!(
        row.try_get_by_index::<String>(1).unwrap(),
        expected_display_name,
        "display_name mismatch"
    );
    assert_eq!(
        row.try_get_by_index::<String>(2).unwrap(),
        "production",
        "kind should be `production` (mokumo's auth_profile_kind)"
    );
}

#[then(expr = "meta.activity_log has a legacy_upgrade_migrated entry for {string}")]
async fn then_meta_activity_log_has_upgrade_entry(w: &mut PlatformBddWorld, expected_slug: String) {
    let ctx = w.legacy_refuse.as_ref().unwrap();
    let pool = ctx.meta_pool.as_ref().expect("meta_pool retained");
    let row = pool
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_type, entity_id, action, actor_id, payload \
             FROM activity_log \
             WHERE entity_id = ? AND action = 'legacy_upgrade_migrated'",
            [expected_slug.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("no legacy_upgrade_migrated entry for `{expected_slug}`"));
    assert_eq!(row.try_get_by_index::<String>(0).unwrap(), "profile");
    assert_eq!(row.try_get_by_index::<String>(1).unwrap(), expected_slug);
    assert_eq!(
        row.try_get_by_index::<String>(2).unwrap(),
        "legacy_upgrade_migrated"
    );
    assert_eq!(row.try_get_by_index::<String>(3).unwrap(), "system");
    let payload: serde_json::Value =
        serde_json::from_str(&row.try_get_by_index::<String>(4).unwrap()).unwrap();
    assert!(
        payload.get("shop_name").is_some(),
        "payload should include shop_name"
    );
    assert!(
        payload.get("vertical_db_path").is_some(),
        "payload should include vertical_db_path"
    );
}

#[then("the on-disk production folder is unchanged")]
async fn then_production_folder_unchanged(w: &mut PlatformBddWorld) {
    let ctx = w.legacy_refuse.as_ref().unwrap();
    let production = ctx.data_dir.path().join("production");
    assert!(
        production.is_dir(),
        "production folder should still exist (PR A is meta-only — no rename)"
    );
    assert!(
        production.join(VERTICAL_DB_FILE).is_file(),
        "production/{VERTICAL_DB_FILE} should still exist"
    );
}

#[then("meta.profiles has no rows")]
async fn then_meta_profiles_empty(w: &mut PlatformBddWorld) {
    let ctx = w.legacy_refuse.as_ref().unwrap();
    let pool = ctx.meta_pool.as_ref().expect("meta_pool retained");
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
    assert_eq!(count, 0, "meta.profiles should be empty");
}
