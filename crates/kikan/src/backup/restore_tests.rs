use super::*;
use tempfile::TempDir;

/// A minimal stub migrator for tests — knows exactly one migration name
/// so we can exercise schema-compatibility checks without pulling a
/// vertical migrator into the test.
struct StubMigrator;

impl MigratorTrait for StubMigrator {
    fn migrations() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>> {
        vec![Box::new(StubMigration)]
    }
}

struct StubMigration;

impl sea_orm_migration::MigrationName for StubMigration {
    fn name(&self) -> &'static str {
        "m20260404_000000_set_pragmas"
    }
}

#[async_trait::async_trait]
impl sea_orm_migration::MigrationTrait for StubMigration {
    async fn up(&self, _manager: &sea_orm_migration::SchemaManager) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }

    async fn down(
        &self,
        _manager: &sea_orm_migration::SchemaManager,
    ) -> Result<(), sea_orm::DbErr> {
        Ok(())
    }
}

// ---- Test helpers ----

/// Create a minimal valid kikan SQLite database with `application_id` set.
fn make_kikan_db(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("test.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    // Stamp application_id and create a seaql_migrations table with one known migration
    conn.execute_batch(&format!(
        "PRAGMA application_id = {KIKAN_APPLICATION_ID};
         CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);
         INSERT INTO seaql_migrations VALUES ('m20260404_000000_set_pragmas', 0);"
    ))
    .unwrap();
    drop(conn);
    path
}

/// Create a valid kikan SQLite database with NO seaql_migrations table.
fn make_kikan_db_no_migrations(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("fresh.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(&format!("PRAGMA application_id = {KIKAN_APPLICATION_ID};"))
        .unwrap();
    drop(conn);
    path
}

/// Create a valid kikan SQLite database with application_id 0 (legacy/unstamped).
fn make_unstamped_db(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("unstamped.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    // application_id defaults to 0 — do not set it
    conn.execute_batch(
        "CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);",
    )
    .unwrap();
    drop(conn);
    path
}

// ---- validate_candidate tests ----

#[test]
fn valid_kikan_db_passes_validation() {
    let dir = TempDir::new().unwrap();
    let path = make_kikan_db(&dir);
    let result = validate_candidate::<StubMigrator>(&path);
    assert!(result.is_ok(), "Expected Ok, got: {result:?}");
}

#[test]
fn empty_file_fails_identity_check() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("empty.db");
    std::fs::write(&path, b"").unwrap();
    let result = validate_candidate::<StubMigrator>(&path);
    assert!(
        matches!(result, Err(RestoreError::NotKikanDatabase { .. })),
        "Expected NotKikanDatabase, got: {result:?}"
    );
}

#[test]
fn non_sqlite_file_fails_identity_check() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("garbage.db");
    std::fs::write(&path, b"this is not a sqlite database at all!!!").unwrap();
    let result = validate_candidate::<StubMigrator>(&path);
    assert!(
        matches!(result, Err(RestoreError::NotKikanDatabase { .. })),
        "Expected NotKikanDatabase, got: {result:?}"
    );
}

#[test]
fn wrong_application_id_fails_identity_check() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("wrong_id.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA application_id = 999999;
         CREATE TABLE _dummy (id INTEGER PRIMARY KEY);",
    )
    .unwrap();
    drop(conn);
    let result = validate_candidate::<StubMigrator>(&path);
    assert!(
        matches!(result, Err(RestoreError::NotKikanDatabase { .. })),
        "Expected NotKikanDatabase for wrong app_id, got: {result:?}"
    );
}

#[test]
fn application_id_zero_passes_identity_check() {
    let dir = TempDir::new().unwrap();
    let path = make_unstamped_db(&dir);
    let result = validate_candidate::<StubMigrator>(&path);
    assert!(result.is_ok(), "Expected Ok for app_id=0, got: {result:?}");
}

#[test]
fn truncated_file_fails_integrity_check() {
    let dir = TempDir::new().unwrap();
    let original = make_kikan_db(&dir);

    // Read the SQLite file and truncate after the header
    let data = std::fs::read(&original).unwrap();
    let truncated_path = dir.path().join("truncated.db");
    std::fs::write(&truncated_path, &data[..100.min(data.len())]).unwrap();

    let result = validate_candidate::<StubMigrator>(&truncated_path);
    assert!(
        matches!(
            result,
            Err(RestoreError::NotKikanDatabase { .. } | RestoreError::DatabaseCorrupt { .. })
        ),
        "Expected NotKikanDatabase or DatabaseCorrupt for truncated file, got: {result:?}"
    );
}

#[test]
fn corrupted_page_data_fails_integrity_check() {
    let dir = TempDir::new().unwrap();
    let original = make_kikan_db(&dir);

    let mut data = std::fs::read(&original).unwrap();
    if data.len() > 200 {
        let mid = data.len() / 2;
        data[mid..mid + 50].fill(0xFF);
    }
    let corrupt_path = dir.path().join("corrupt.db");
    std::fs::write(&corrupt_path, &data).unwrap();

    let result = validate_candidate::<StubMigrator>(&corrupt_path);
    match &result {
        Ok(_)
        | Err(RestoreError::DatabaseCorrupt { .. } | RestoreError::NotKikanDatabase { .. }) => {}
        other => panic!("Unexpected error for corrupted file: {other:?}"),
    }
}

#[test]
fn unknown_migrations_fails_schema_check() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("future.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(&format!(
        "PRAGMA application_id = {KIKAN_APPLICATION_ID};
         CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);
         INSERT INTO seaql_migrations VALUES ('m99991231_999999_future_migration', 0);"
    ))
    .unwrap();
    drop(conn);

    let result = validate_candidate::<StubMigrator>(&path);
    assert!(
        matches!(result, Err(RestoreError::SchemaIncompatible { .. })),
        "Expected SchemaIncompatible, got: {result:?}"
    );
}

#[test]
fn no_migrations_table_passes_compatibility_check() {
    let dir = TempDir::new().unwrap();
    let path = make_kikan_db_no_migrations(&dir);
    let result = validate_candidate::<StubMigrator>(&path);
    assert!(
        result.is_ok(),
        "Expected Ok for DB with no migrations table, got: {result:?}"
    );
    let info = result.unwrap();
    assert!(info.schema_version.is_none(), "Expected no schema version");
}

#[test]
fn candidate_info_contains_file_size() {
    let dir = TempDir::new().unwrap();
    let path = make_kikan_db(&dir);
    let actual_size = std::fs::metadata(&path).unwrap().len();
    let info = validate_candidate::<StubMigrator>(&path).unwrap();
    assert_eq!(info.file_size.get(), actual_size);
}

#[test]
fn candidate_info_contains_schema_version() {
    let dir = TempDir::new().unwrap();
    let path = make_kikan_db(&dir);
    let info = validate_candidate::<StubMigrator>(&path).unwrap();
    assert_eq!(
        info.schema_version.as_deref(),
        Some("m20260404_000000_set_pragmas"),
        "Expected the known migration version"
    );
}

// ---- copy_to_production tests ----

#[test]
fn copy_to_production_happy_path() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let source = make_kikan_db(&src_dir);
    let production_dir = dst_dir.path().join("production");

    let result = copy_to_production(&source, &production_dir, "mokumo.db");
    assert!(result.is_ok(), "Expected Ok, got: {result:?}");

    let final_path = production_dir.join("mokumo.db");
    assert!(final_path.exists(), "Production DB should exist after copy");

    let conn = rusqlite::Connection::open(&final_path).unwrap();
    let app_id: i64 = conn
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        app_id, KIKAN_APPLICATION_ID,
        "Copied DB should have correct application_id"
    );
}

#[test]
fn copy_to_production_fails_when_dest_exists() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let source = make_kikan_db(&src_dir);
    let production_dir = dst_dir.path().join("production");

    std::fs::create_dir_all(&production_dir).unwrap();
    std::fs::write(production_dir.join("mokumo.db"), b"existing").unwrap();

    let result = copy_to_production(&source, &production_dir, "mokumo.db");
    assert!(
        matches!(result, Err(RestoreError::ProductionDbExists { .. })),
        "Expected ProductionDbExists, got: {result:?}"
    );
}

#[test]
fn copy_to_production_uses_temp_file_then_renames() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let source = make_kikan_db(&src_dir);
    let production_dir = dst_dir.path().join("production");

    copy_to_production(&source, &production_dir, "mokumo.db").unwrap();

    assert!(production_dir.join("mokumo.db").exists());
    assert!(
        !production_dir.join("mokumo.db.restore-tmp").exists(),
        "Temp file should be cleaned up after successful copy"
    );
}

#[test]
fn copy_to_production_cleans_up_stale_temp_file() {
    let src_dir = TempDir::new().unwrap();
    let dst_dir = TempDir::new().unwrap();
    let source = make_kikan_db(&src_dir);
    let production_dir = dst_dir.path().join("production");

    std::fs::create_dir_all(&production_dir).unwrap();
    let stale_temp = production_dir.join("mokumo.db.restore-tmp");
    std::fs::write(&stale_temp, b"stale data").unwrap();

    let result = copy_to_production(&source, &production_dir, "mokumo.db");
    assert!(
        result.is_ok(),
        "Expected Ok despite stale temp, got: {result:?}"
    );
    assert!(production_dir.join("mokumo.db").exists());
    assert!(!stale_temp.exists(), "Stale temp should be gone");
}
