use std::path::{Path, PathBuf};

use cucumber::{given, then, when};
use kikan::meta::{UpgradeError, UpgradeOutcome, run_legacy_upgrade};
use kikan::migrations::platform::run_platform_meta_migrations;
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};

use super::KikanWorld;

pub struct LegacyUpgradeCtx {
    pub meta: Option<DatabaseConnection>,
    pub auth: Option<DatabaseConnection>,
    pub auth_path: Option<PathBuf>,
    pub auth_tmp: Option<tempfile::TempDir>,
    pub upgrade_result: Option<Result<UpgradeOutcome, UpgradeError>>,
    pub schema_compat_result: Option<Result<(), kikan::db::DatabaseSetupError>>,
}

impl LegacyUpgradeCtx {
    fn new() -> Self {
        Self {
            meta: None,
            auth: None,
            auth_path: None,
            auth_tmp: None,
            upgrade_result: None,
            schema_compat_result: None,
        }
    }
}

fn ctx(w: &mut KikanWorld) -> &mut LegacyUpgradeCtx {
    w.legacy_upgrade.get_or_insert_with(LegacyUpgradeCtx::new)
}

async fn scalar_i64(db: &DatabaseConnection, sql: &'static str) -> i64 {
    db.query_one_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index(0)
        .unwrap()
}

async fn legacy_table_exists(db: &DatabaseConnection, name: &str) -> bool {
    let sql = format!(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{}'",
        name.replace('\'', "''")
    );
    db.query_one_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index::<i64>(0)
        .unwrap()
        > 0
}

const PRE_STAGE3_SCHEMA: &str = "
    CREATE TABLE roles (
        id INTEGER PRIMARY KEY,
        name TEXT UNIQUE NOT NULL,
        description TEXT,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
    );
    INSERT INTO roles (id, name, description) VALUES
        (1, 'Admin', 'Full access to all features'),
        (2, 'Staff', 'Standard staff access'),
        (3, 'Guest', 'Read-only guest access');
    CREATE TABLE users (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        email TEXT UNIQUE NOT NULL,
        name TEXT NOT NULL,
        password_hash TEXT NOT NULL,
        role_id INTEGER NOT NULL DEFAULT 1 REFERENCES roles(id) ON DELETE RESTRICT,
        is_active BOOLEAN NOT NULL DEFAULT 1,
        last_login_at TEXT,
        recovery_code_hash TEXT,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
        deleted_at TEXT
    );
    CREATE INDEX idx_users_deleted_at ON users(id) WHERE deleted_at IS NULL;
    CREATE TRIGGER users_updated_at AFTER UPDATE ON users
        FOR EACH ROW BEGIN
            UPDATE users SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = OLD.id;
        END;
";

// ── Givens ────────────────────────────────────────────────────────

#[given("a meta DB with platform migrations applied")]
async fn given_meta_with_platform_migrations(w: &mut KikanWorld) {
    let pool = Database::connect("sqlite::memory:").await.unwrap();
    run_platform_meta_migrations(&pool).await.unwrap();
    ctx(w).meta = Some(pool);
}

#[given("a per-profile DB with legacy users and roles tables and one admin")]
async fn given_legacy_with_one_admin(w: &mut KikanWorld) {
    let pool = Database::connect("sqlite::memory:").await.unwrap();
    pool.execute_unprepared(PRE_STAGE3_SCHEMA).await.unwrap();
    pool.execute_unprepared(
        "INSERT INTO users (id, email, name, password_hash, role_id, is_active, \
                            created_at, updated_at) \
         VALUES (1, 'admin@example.com', 'Admin', 'hash-1', 1, 1, \
                 '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
    )
    .await
    .unwrap();
    ctx(w).auth = Some(pool);
}

#[given("a per-profile DB with two legacy admins")]
async fn given_legacy_with_two_admins(w: &mut KikanWorld) {
    let pool = Database::connect("sqlite::memory:").await.unwrap();
    pool.execute_unprepared(PRE_STAGE3_SCHEMA).await.unwrap();
    pool.execute_unprepared(
        "INSERT INTO users (id, email, name, password_hash, role_id, is_active, \
                            created_at, updated_at) \
         VALUES \
            (1, 'admin1@example.com', 'Admin 1', 'hash-1', 1, 1, \
             '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z'), \
            (2, 'admin2@example.com', 'Admin 2', 'hash-2', 1, 1, \
             '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
    )
    .await
    .unwrap();
    ctx(w).auth = Some(pool);
}

#[given("a per-profile DB with legacy users and roles tables and a custom role at id 4")]
async fn given_legacy_with_custom_role(w: &mut KikanWorld) {
    let pool = Database::connect("sqlite::memory:").await.unwrap();
    pool.execute_unprepared(PRE_STAGE3_SCHEMA).await.unwrap();
    pool.execute_unprepared(
        "INSERT INTO roles (id, name, description) VALUES (4, 'Owner', 'Shop owner'); \
         INSERT INTO users (id, email, name, password_hash, role_id, is_active, \
                            created_at, updated_at) \
         VALUES (1, 'admin@example.com', 'Admin', 'hash-1', 4, 1, \
                 '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
    )
    .await
    .unwrap();
    ctx(w).auth = Some(pool);
}

#[given("a per-profile DB with no legacy users table")]
async fn given_no_legacy_tables(w: &mut KikanWorld) {
    let pool = Database::connect("sqlite::memory:").await.unwrap();
    ctx(w).auth = Some(pool);
}

#[given("a per-profile DB seeded from the pre-Stage-3 fixture")]
async fn given_pre_stage3_fixture(w: &mut KikanWorld) {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set under cargo test");
    // Fixture lives at the workspace root (tests/fixtures/pre-stage3.sqlite).
    // CARGO_MANIFEST_DIR points to crates/kikan/.
    let fixture = Path::new(&manifest_dir)
        .join("../../tests/fixtures/pre-stage3.sqlite")
        .canonicalize()
        .expect("pre-stage3 fixture must exist at tests/fixtures/pre-stage3.sqlite");

    let tmp = tempfile::tempdir().unwrap();
    let copy = tmp.path().join("legacy.db");
    std::fs::copy(&fixture, &copy).expect("copy fixture");

    let url = format!("sqlite:{}?mode=rwc", copy.display());
    let pool = Database::connect(&url).await.unwrap();
    let c = ctx(w);
    c.auth = Some(pool);
    c.auth_path = Some(copy);
    c.auth_tmp = Some(tmp);
}

#[given("meta.users already contains the legacy admin row")]
async fn given_meta_already_has_admin(w: &mut KikanWorld) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    meta.execute_unprepared(
        "INSERT INTO users (id, email, name, password_hash, role_id, is_active, \
                            created_at, updated_at) \
         VALUES (1, 'admin@example.com', 'Admin', 'hash-1', 1, 1, \
                 '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
    )
    .await
    .unwrap();
}

#[given("meta.users already contains a foreign user at id 1")]
async fn given_meta_has_foreign_user(w: &mut KikanWorld) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    meta.execute_unprepared(
        "INSERT INTO users (id, email, name, password_hash, role_id, is_active, \
                            created_at, updated_at) \
         VALUES (1, 'someone-else@example.com', 'Someone Else', 'hash-2', 1, 1, \
                 '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
    )
    .await
    .unwrap();
}

#[given("meta.roles already contains a foreign role at id 4")]
async fn given_meta_has_foreign_role(w: &mut KikanWorld) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    meta.execute_unprepared(
        "INSERT INTO roles (id, name, description) VALUES (4, 'Suspended', 'Read-only')",
    )
    .await
    .unwrap();
}

#[given("meta.users contains only the first legacy admin's email")]
async fn given_meta_has_partial_intersection(w: &mut KikanWorld) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    meta.execute_unprepared(
        "INSERT INTO users (id, email, name, password_hash, role_id, is_active, \
                            created_at, updated_at) \
         VALUES (10, 'admin1@example.com', 'Admin 1', 'hash-1', 1, 1, \
                 '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
    )
    .await
    .unwrap();
}

#[given(
    "a per-profile SQLite file with seaql_migrations rows for users_and_roles and shop_settings"
)]
async fn given_seaql_with_platform_owned_rows(w: &mut KikanWorld) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("legacy_with_seaql.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);
         INSERT INTO seaql_migrations VALUES ('m20260321_000000_init', 1000);
         INSERT INTO seaql_migrations VALUES ('m20260327_000000_users_and_roles', 1001);
         INSERT INTO seaql_migrations VALUES ('m20260411_000000_shop_settings', 1002);",
    )
    .unwrap();
    drop(conn);
    let c = ctx(w);
    c.auth_path = Some(path);
    c.auth_tmp = Some(tmp);
}

// ── When ──────────────────────────────────────────────────────────

#[when("I run the legacy upgrade")]
async fn when_run_legacy_upgrade(w: &mut KikanWorld) {
    let c = ctx(w);
    let meta = c.meta.as_ref().expect("meta DB present").clone();
    let auth = c.auth.as_ref().expect("auth DB present").clone();
    let result = run_legacy_upgrade(
        &meta,
        &auth,
        "Acme Printing",
        Path::new("/data/production/mokumo.db"),
        "production",
    )
    .await;
    c.upgrade_result = Some(result);
}

#[when("I check schema compatibility against a vertical migrator that does not declare those rows")]
async fn when_check_schema_compat(w: &mut KikanWorld) {
    use sea_orm_migration::{MigrationName, MigrationTrait, MigratorTrait, SchemaManager};

    struct VerticalMigrator;
    #[async_trait::async_trait]
    impl MigratorTrait for VerticalMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(InitOnly)]
        }
    }
    struct InitOnly;
    impl MigrationName for InitOnly {
        fn name(&self) -> &'static str {
            "m20260321_000000_init"
        }
    }
    #[async_trait::async_trait]
    impl MigrationTrait for InitOnly {
        async fn up(&self, _: &SchemaManager) -> Result<(), sea_orm::DbErr> {
            Ok(())
        }
    }

    let c = ctx(w);
    let path = c.auth_path.as_ref().expect("auth_path present").clone();
    c.schema_compat_result = Some(kikan::db::check_schema_compatibility::<VerticalMigrator>(
        &path,
    ));
}

// ── Then ──────────────────────────────────────────────────────────

#[then("the upgrade succeeds")]
async fn then_upgrade_succeeds(w: &mut KikanWorld) {
    let c = ctx(w);
    let result = c.upgrade_result.as_ref().expect("upgrade was invoked");
    assert!(
        result.is_ok(),
        "expected Ok, got {:?}",
        result.as_ref().err()
    );
}

#[then(expr = "the upgrade fails with UnsupportedLegacyState mentioning {string}")]
async fn then_upgrade_fails_unsupported(w: &mut KikanWorld, fragment: String) {
    let c = ctx(w);
    let err = c
        .upgrade_result
        .as_ref()
        .expect("upgrade was invoked")
        .as_ref()
        .expect_err("expected upgrade to fail");
    match err {
        UpgradeError::UnsupportedLegacyState(msg) => {
            assert!(
                msg.to_lowercase().contains(&fragment.to_lowercase()),
                "error message {msg:?} does not contain fragment {fragment:?}"
            );
        }
        other => panic!("expected UnsupportedLegacyState, got {other:?}"),
    }
}

#[then(expr = "meta.users contains {int} user")]
async fn then_meta_users_count_singular(w: &mut KikanWorld, n: i64) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    assert_eq!(scalar_i64(&meta, "SELECT COUNT(*) FROM users").await, n);
}

#[then(expr = "meta.users contains {int} users")]
async fn then_meta_users_count_plural(w: &mut KikanWorld, n: i64) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    assert_eq!(scalar_i64(&meta, "SELECT COUNT(*) FROM users").await, n);
}

#[then(expr = "meta.users contains at least {int} user")]
async fn then_meta_users_count_at_least(w: &mut KikanWorld, n: i64) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    let actual = scalar_i64(&meta, "SELECT COUNT(*) FROM users").await;
    assert!(actual >= n, "expected at least {n}, got {actual}");
}

#[then(expr = "meta.roles contains {int} platform-seeded roles")]
async fn then_meta_roles_count(w: &mut KikanWorld, n: i64) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    assert_eq!(scalar_i64(&meta, "SELECT COUNT(*) FROM roles").await, n);
}

#[then(expr = "meta.profiles contains {int} rows")]
async fn then_meta_profiles_count(w: &mut KikanWorld, n: i64) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    assert_eq!(scalar_i64(&meta, "SELECT COUNT(*) FROM profiles").await, n);
}

#[then(expr = "the meta legacy_upgrade_locks table contains {int} row")]
async fn then_lock_count_singular(w: &mut KikanWorld, n: i64) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    assert_eq!(
        scalar_i64(&meta, "SELECT COUNT(*) FROM legacy_upgrade_locks").await,
        n
    );
}

#[then(expr = "the meta legacy_upgrade_locks table contains {int} rows")]
async fn then_lock_count_plural(w: &mut KikanWorld, n: i64) {
    let meta = ctx(w).meta.as_ref().expect("meta DB present").clone();
    assert_eq!(
        scalar_i64(&meta, "SELECT COUNT(*) FROM legacy_upgrade_locks").await,
        n
    );
}

#[then("the per-profile DB has no legacy users table")]
async fn then_no_legacy_users(w: &mut KikanWorld) {
    let auth = ctx(w).auth.as_ref().expect("auth DB present").clone();
    assert!(!legacy_table_exists(&auth, "users").await);
}

#[then("the per-profile DB has no legacy roles table")]
async fn then_no_legacy_roles(w: &mut KikanWorld) {
    let auth = ctx(w).auth.as_ref().expect("auth DB present").clone();
    assert!(!legacy_table_exists(&auth, "roles").await);
}

#[then("the per-profile DB still has the legacy users table")]
async fn then_legacy_users_present(w: &mut KikanWorld) {
    let auth = ctx(w).auth.as_ref().expect("auth DB present").clone();
    assert!(legacy_table_exists(&auth, "users").await);
}

#[then("the schema-compat check passes")]
async fn then_schema_compat_passes(w: &mut KikanWorld) {
    let c = ctx(w);
    let result = c
        .schema_compat_result
        .as_ref()
        .expect("schema-compat check was invoked");
    assert!(
        result.is_ok(),
        "expected Ok, got {:?}",
        result.as_ref().err()
    );
}
