use kikan::GraftId;
use kikan::MigrationRef;
use kikan::tenancy::guards;

fn create_db_with_app_id(dir: &std::path::Path, app_id: i64) -> std::path::PathBuf {
    let path = dir.join("test.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(&format!("PRAGMA application_id = {app_id}"))
        .unwrap();
    conn.execute_batch("CREATE TABLE test_table (id INTEGER PRIMARY KEY)")
        .unwrap();
    drop(conn);
    path
}

// --- check_application_id ---

#[test]
fn check_application_id_valid_mokumo() {
    let tmp = tempfile::tempdir().unwrap();
    let path = create_db_with_app_id(tmp.path(), guards::MOKUMO_APPLICATION_ID);
    guards::check_application_id(&path).unwrap();
}

#[test]
fn check_application_id_default_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let path = create_db_with_app_id(tmp.path(), 0);
    guards::check_application_id(&path).unwrap();
}

#[test]
fn check_application_id_foreign_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = create_db_with_app_id(tmp.path(), 42);
    let result = guards::check_application_id(&path);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not a Mokumo database"));
}

// --- ensure_auto_vacuum ---

#[test]
fn ensure_auto_vacuum_fresh_db() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("fresh.db");
    guards::ensure_auto_vacuum(&path).unwrap();

    let conn = rusqlite::Connection::open(&path).unwrap();
    let av: i32 = conn
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();
    assert_eq!(av, 2); // INCREMENTAL
}

#[test]
fn ensure_auto_vacuum_upgrades_none_to_incremental() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("none.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY)")
            .unwrap();
    }
    guards::ensure_auto_vacuum(&path).unwrap();

    let conn = rusqlite::Connection::open(&path).unwrap();
    let av: i32 = conn
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();
    assert_eq!(av, 2);
}

#[test]
fn ensure_auto_vacuum_noop_if_already_incremental() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("incr.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA auto_vacuum = INCREMENTAL; CREATE TABLE t (id INTEGER PRIMARY KEY)",
        )
        .unwrap();
    }
    guards::ensure_auto_vacuum(&path).unwrap();

    let conn = rusqlite::Connection::open(&path).unwrap();
    let av: i32 = conn
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();
    assert_eq!(av, 2);
}

// --- check_schema_compatibility ---

#[test]
fn check_schema_compatibility_fresh_install() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nonexistent.db");
    let known = vec![MigrationRef {
        graft: GraftId::new("mokumo"),
        name: "m001",
    }];
    guards::check_schema_compatibility(&path, &known).unwrap();
}

#[test]
fn check_schema_compatibility_all_known() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("known.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE kikan_migrations (graft_id TEXT, name TEXT, applied_at INTEGER);
             INSERT INTO kikan_migrations VALUES ('mokumo', 'm001', 1000);
             INSERT INTO kikan_migrations VALUES ('mokumo', 'm002', 1001);",
        )
        .unwrap();
    }
    let known = vec![
        MigrationRef {
            graft: GraftId::new("mokumo"),
            name: "m001",
        },
        MigrationRef {
            graft: GraftId::new("mokumo"),
            name: "m002",
        },
        MigrationRef {
            graft: GraftId::new("mokumo"),
            name: "m003",
        },
    ];
    guards::check_schema_compatibility(&path, &known).unwrap();
}

#[test]
fn check_schema_compatibility_unknown_migration_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("unknown.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE kikan_migrations (graft_id TEXT, name TEXT, applied_at INTEGER);
             INSERT INTO kikan_migrations VALUES ('mokumo', 'm001', 1000);
             INSERT INTO kikan_migrations VALUES ('mokumo', 'm_future', 1001);",
        )
        .unwrap();
    }
    let known = vec![MigrationRef {
        graft: GraftId::new("mokumo"),
        name: "m001",
    }];
    let result = guards::check_schema_compatibility(&path, &known);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("schema incompatible"));
}

#[test]
fn check_schema_compatibility_seaql_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("seaql.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE seaql_migrations (version TEXT, applied_at INTEGER);
             INSERT INTO seaql_migrations VALUES ('m001', 1000);
             INSERT INTO seaql_migrations VALUES ('m002', 1001);",
        )
        .unwrap();
    }
    let known = vec![
        MigrationRef {
            graft: GraftId::new("mokumo"),
            name: "m001",
        },
        MigrationRef {
            graft: GraftId::new("mokumo"),
            name: "m002",
        },
    ];
    guards::check_schema_compatibility(&path, &known).unwrap();
}

// --- pre_migration_backup ---

#[tokio::test]
async fn pre_migration_backup_skips_nonexistent() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nonexistent.db");
    let result = guards::pre_migration_backup(&path).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn pre_migration_backup_creates_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mokumo.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE kikan_migrations (graft_id TEXT, name TEXT, applied_at INTEGER);
             INSERT INTO kikan_migrations VALUES ('mokumo', 'm001', 1000);",
        )
        .unwrap();
    }
    let result = guards::pre_migration_backup(&path).await.unwrap();
    assert!(result.is_some());
    let backup_path = result.unwrap();
    assert!(backup_path.exists());
    assert!(backup_path.to_string_lossy().contains("backup-v"));
}

#[tokio::test]
async fn pre_migration_backup_skips_no_migration_table() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("empty.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE other (id INTEGER PRIMARY KEY)")
            .unwrap();
    }
    let result = guards::pre_migration_backup(&path).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn pre_migration_backup_rotates_to_three() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mokumo.db");

    for i in 1..=4 {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS kikan_migrations (graft_id TEXT, name TEXT, applied_at INTEGER);
             DELETE FROM kikan_migrations;
             INSERT INTO kikan_migrations VALUES ('mokumo', 'm{i:03}', {i}000);"
        )).unwrap();
        drop(conn);
        guards::pre_migration_backup(&path).await.unwrap();
    }

    let mut backups = Vec::new();
    let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains("backup-v") {
            backups.push(name);
        }
    }
    assert_eq!(backups.len(), 3, "should keep only 3 backups");
}

// --- resolve_active_profile ---

#[test]
fn resolve_active_profile_defaults_to_demo() {
    use kikan_types::SetupMode;
    let tmp = tempfile::tempdir().unwrap();
    let mode = kikan::tenancy::resolve::resolve_active_profile(tmp.path(), SetupMode::Demo);
    assert_eq!(mode, SetupMode::Demo);
}

#[test]
fn resolve_active_profile_reads_production() {
    use kikan_types::SetupMode;
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("active_profile"), "production").unwrap();
    let mode = kikan::tenancy::resolve::resolve_active_profile(tmp.path(), SetupMode::Demo);
    assert_eq!(mode, SetupMode::Production);
}

#[test]
fn resolve_active_profile_invalid_defaults_to_demo() {
    use kikan_types::SetupMode;
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("active_profile"), "garbage").unwrap();
    let mode = kikan::tenancy::resolve::resolve_active_profile(tmp.path(), SetupMode::Demo);
    assert_eq!(mode, SetupMode::Demo);
}

// `migrate_flat_layout` moved to `mokumo-shop` (Mokumo-specific legacy
// migration). Coverage lives in
// `crates/mokumo-shop/tests/platform_startup_guards.rs`.
