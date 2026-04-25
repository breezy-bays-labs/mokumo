use cucumber::{given, then, when};
use kikan::meta::{AbandonReason, BootState, BootStateDetectionError, detect_boot_state};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use std::path::{Path, PathBuf};

use super::KikanWorld;

const TEST_DB_FILE: &str = "vertical.db";

pub struct BootStateCtx {
    pub data_dir: tempfile::TempDir,
    pub meta_pool: Option<DatabaseConnection>,
    pub state: Option<Result<BootState, BootStateDetectionError>>,
}

impl BootStateCtx {
    fn data_path(&self) -> &Path {
        self.data_dir.path()
    }
}

fn seed_legacy_vertical(production: &Path, admin: bool, shop_name: &str) -> PathBuf {
    std::fs::create_dir_all(production).expect("mkdir production");
    let vertical = production.join(TEST_DB_FILE);
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

#[given("a fresh data directory")]
async fn fresh_data_dir(w: &mut KikanWorld) {
    let dir = tempfile::tempdir().unwrap();
    w.boot_state = Some(BootStateCtx {
        data_dir: dir,
        meta_pool: None,
        state: None,
    });
}

#[given("a meta pool with the profiles table created")]
async fn meta_pool_with_profiles_table(w: &mut KikanWorld) {
    let pool = Database::connect("sqlite::memory:").await.unwrap();
    pool.execute_unprepared("CREATE TABLE profiles (slug TEXT PRIMARY KEY)")
        .await
        .unwrap();
    w.boot_state.as_mut().unwrap().meta_pool = Some(pool);
}

#[given(regex = r"^meta\.profiles has (\d+) rows$")]
async fn meta_profiles_has_rows(w: &mut KikanWorld, n: usize) {
    let ctx = w.boot_state.as_mut().unwrap();
    let pool = ctx.meta_pool.as_ref().unwrap();
    for i in 0..n {
        let slug = format!("profile-{i}");
        pool.execute_unprepared(&format!("INSERT INTO profiles (slug) VALUES ('{slug}')"))
            .await
            .unwrap();
    }
}

#[given("a legacy production folder with no vertical DB")]
async fn legacy_production_without_vertical_db(w: &mut KikanWorld) {
    let ctx = w.boot_state.as_mut().unwrap();
    std::fs::create_dir_all(ctx.data_path().join("production")).unwrap();
}

#[given("a legacy production folder with a vertical DB that has no admin user")]
async fn legacy_production_no_admin(w: &mut KikanWorld) {
    let ctx = w.boot_state.as_mut().unwrap();
    seed_legacy_vertical(&ctx.data_path().join("production"), false, "Acme Printing");
}

#[given(regex = r#"^a legacy production folder with an admin user and shop_name "(.*)"$"#)]
async fn legacy_production_with_admin_and_shop_name(w: &mut KikanWorld, shop_name: String) {
    let ctx = w.boot_state.as_mut().unwrap();
    seed_legacy_vertical(&ctx.data_path().join("production"), true, &shop_name);
}

#[when("boot-state detection runs")]
async fn boot_state_detection_runs(w: &mut KikanWorld) {
    let ctx = w.boot_state.as_mut().unwrap();
    let pool = ctx.meta_pool.as_ref().unwrap();
    let result = detect_boot_state(ctx.data_path(), pool, TEST_DB_FILE).await;
    ctx.state = Some(result);
}

#[then("the boot state is FreshInstall")]
async fn assert_fresh_install(w: &mut KikanWorld) {
    let state = w
        .boot_state
        .as_ref()
        .and_then(|c| c.state.as_ref())
        .expect("detect_boot_state result");
    let state = state.as_ref().expect("Ok result");
    assert!(
        matches!(state, BootState::FreshInstall),
        "expected FreshInstall, got {state:?}"
    );
}

#[then(regex = r"^the boot state is PostUpgradeOrSetup with profile_count (\d+)$")]
async fn assert_post_upgrade(w: &mut KikanWorld, expected: usize) {
    let state = w
        .boot_state
        .as_ref()
        .and_then(|c| c.state.as_ref())
        .expect("detect_boot_state result");
    let state = state.as_ref().expect("Ok result");
    let BootState::PostUpgradeOrSetup { profile_count } = state else {
        panic!("expected PostUpgradeOrSetup, got {state:?}");
    };
    assert_eq!(*profile_count, expected);
}

#[then("the boot state is LegacyAbandoned with reason NoVerticalDbFile")]
async fn assert_abandoned_no_vertical_db(w: &mut KikanWorld) {
    assert_legacy_abandoned(w, AbandonReason::NoVerticalDbFile);
}

#[then("the boot state is LegacyAbandoned with reason NoAdminUser")]
async fn assert_abandoned_no_admin(w: &mut KikanWorld) {
    assert_legacy_abandoned(w, AbandonReason::NoAdminUser);
}

fn assert_legacy_abandoned(w: &KikanWorld, expected: AbandonReason) {
    let state = w
        .boot_state
        .as_ref()
        .and_then(|c| c.state.as_ref())
        .expect("detect_boot_state result");
    let state = state.as_ref().expect("Ok result");
    let BootState::LegacyAbandoned { reason } = state else {
        panic!("expected LegacyAbandoned, got {state:?}");
    };
    assert_eq!(*reason, expected, "reason mismatch");
}

#[then(regex = r#"^the boot state is LegacyCompleted with shop_name "(.*)"$"#)]
async fn assert_legacy_completed(w: &mut KikanWorld, expected_name: String) {
    let state = w
        .boot_state
        .as_ref()
        .and_then(|c| c.state.as_ref())
        .expect("detect_boot_state result");
    let state = state.as_ref().expect("Ok result");
    let BootState::LegacyCompleted { shop_name, .. } = state else {
        panic!("expected LegacyCompleted, got {state:?}");
    };
    assert_eq!(*shop_name, expected_name);
}

#[then("the boot state is LegacyDefensiveEmpty")]
async fn assert_legacy_defensive_empty(w: &mut KikanWorld) {
    let state = w
        .boot_state
        .as_ref()
        .and_then(|c| c.state.as_ref())
        .expect("detect_boot_state result");
    let state = state.as_ref().expect("Ok result");
    assert!(
        matches!(state, BootState::LegacyDefensiveEmpty { .. }),
        "expected LegacyDefensiveEmpty, got {state:?}"
    );
}
