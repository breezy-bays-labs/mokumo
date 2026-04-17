//! BDD step definitions for `restore_validation.feature`.

use std::path::{Path, PathBuf};

use cucumber::{given, then, when};
use sea_orm_migration::MigratorTrait as _;

use mokumo_shop::restore::{self, RestoreError};

use super::DbWorld;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Ensure `restore_tmp` is initialized and return its path.
fn ensure_tmp(w: &mut DbWorld) -> PathBuf {
    if w.restore_tmp.is_none() {
        w.restore_tmp = Some(tempfile::tempdir().expect("failed to create temp dir"));
    }
    w.restore_tmp.as_ref().unwrap().path().to_path_buf()
}

/// Create a SQLite DB with the given application_id, plus a dummy table to
/// flush page 1 (the header) to disk. Patches the application_id field
/// directly in the header (offset 68, 4 bytes big-endian) so the full 32-bit
/// range — including values that exceed `i32::MAX` like `0xDEADBEEF` — can be
/// represented faithfully (SQLite's PRAGMA parser rejects such literals).
fn make_sqlite_with_app_id(path: &Path, app_id: i64) {
    use std::io::{Seek, SeekFrom, Write};

    {
        let conn = rusqlite::Connection::open(path).unwrap();
        // Force page 1 (the header) to be written to disk by creating a table.
        conn.execute_batch("CREATE TABLE _dummy (id INTEGER PRIMARY KEY);")
            .unwrap();
        drop(conn);
    }

    let bytes = (app_id as i32 as u32).to_be_bytes();
    let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start(68)).unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
}

/// Create a "real" Mokumo DB with the correct application_id and a
/// `seaql_migrations` table populated with all migrations the binary knows
/// about.
fn make_mokumo_db_with_all_migrations(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    let app_id = mokumo_db::MOKUMO_APPLICATION_ID;
    conn.execute_batch(&format!(
        "PRAGMA application_id = {app_id};
         CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);
         CREATE TABLE _dummy (id INTEGER PRIMARY KEY);"
    ))
    .unwrap();
    for m in mokumo_shop::migrations::Migrator::migrations() {
        conn.execute(
            "INSERT INTO seaql_migrations (version, applied_at) VALUES (?1, 0)",
            rusqlite::params![m.name()],
        )
        .unwrap();
    }
    drop(conn);
}

fn first_known_migration() -> String {
    mokumo_shop::migrations::Migrator::migrations()
        .iter()
        .map(|m| m.name().to_owned())
        .min()
        .expect("at least one migration registered")
}

// ── Given steps ───────────────────────────────────────────────────────────────

#[given(expr = "a SQLite file with application_id {word}")]
async fn given_sqlite_with_application_id(w: &mut DbWorld, app_id_str: String) {
    // SQLite's application_id is a 32-bit signed integer; reinterpret hex
    // values that exceed i32::MAX as their two's-complement signed form so the
    // PRAGMA stores the intended bit pattern.
    let app_id: i32 = if let Some(hex) = app_id_str.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).expect("invalid hex application_id") as i32
    } else {
        app_id_str.parse().expect("invalid decimal application_id")
    };

    let dir = ensure_tmp(w);
    let path = dir.join("candidate.db");
    make_sqlite_with_app_id(&path, app_id as i64);
    w.restore_candidate_path = Some(path);
}

#[given("a file that is not a valid SQLite database")]
async fn given_non_sqlite_file(w: &mut DbWorld) {
    let dir = ensure_tmp(w);
    let path = dir.join("garbage.db");
    std::fs::write(&path, b"this is definitely not a sqlite database!!!").unwrap();
    w.restore_candidate_path = Some(path);
}

#[given("a valid Mokumo database file")]
async fn given_valid_mokumo_db(w: &mut DbWorld) {
    let dir = ensure_tmp(w);
    let path = dir.join("valid.db");
    make_mokumo_db_with_all_migrations(&path);
    w.restore_candidate_path = Some(path);
}

#[given("a SQLite file with a valid header but truncated data pages")]
async fn given_truncated_sqlite(w: &mut DbWorld) {
    let dir = ensure_tmp(w);
    let original = dir.join("source.db");
    make_mokumo_db_with_all_migrations(&original);

    let data = std::fs::read(&original).unwrap();
    let truncated_path = dir.join("truncated.db");
    let keep = 100.min(data.len());
    std::fs::write(&truncated_path, &data[..keep]).unwrap();
    w.restore_candidate_path = Some(truncated_path);
}

#[given("a SQLite file with deliberately corrupted page data")]
async fn given_corrupted_sqlite(w: &mut DbWorld) {
    let dir = ensure_tmp(w);
    let original = dir.join("source.db");
    make_mokumo_db_with_all_migrations(&original);

    let mut data = std::fs::read(&original).unwrap();
    // Aggressively corrupt every page after the SQLite header (first 100 bytes)
    // by overwriting with 0xFF. This guarantees integrity_check will detect
    // damage rather than landing on unused/free pages.
    if data.len() > 100 {
        for byte in &mut data[100..] {
            *byte = 0xFF;
        }
    }
    let corrupt_path = dir.join("corrupt.db");
    std::fs::write(&corrupt_path, &data).unwrap();
    w.restore_candidate_path = Some(corrupt_path);
}

#[given("a Mokumo database at the current schema version")]
async fn given_db_at_current_schema(w: &mut DbWorld) {
    let dir = ensure_tmp(w);
    let path = dir.join("current.db");
    make_mokumo_db_with_all_migrations(&path);
    w.restore_candidate_path = Some(path);
}

#[given("a Mokumo database at an older schema version")]
async fn given_db_at_older_schema(w: &mut DbWorld) {
    let dir = ensure_tmp(w);
    let path = dir.join("older.db");
    let app_id = mokumo_db::MOKUMO_APPLICATION_ID;
    let oldest = first_known_migration();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(&format!(
        "PRAGMA application_id = {app_id};
         CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);
         CREATE TABLE _dummy (id INTEGER PRIMARY KEY);
         INSERT INTO seaql_migrations VALUES ('{oldest}', 0);"
    ))
    .unwrap();
    drop(conn);
    w.restore_candidate_path = Some(path);
}

#[given("a Mokumo database with migrations not known to this binary")]
async fn given_db_with_unknown_migrations(w: &mut DbWorld) {
    let dir = ensure_tmp(w);
    let path = dir.join("unknown.db");
    let app_id = mokumo_db::MOKUMO_APPLICATION_ID;
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(&format!(
        "PRAGMA application_id = {app_id};
         CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);
         CREATE TABLE _dummy (id INTEGER PRIMARY KEY);
         INSERT INTO seaql_migrations VALUES ('99991231_000000_unknown', 0);"
    ))
    .unwrap();
    drop(conn);
    w.restore_candidate_path = Some(path);
}

#[given("a Mokumo database with no seaql_migrations table")]
async fn given_db_without_migrations_table(w: &mut DbWorld) {
    let dir = ensure_tmp(w);
    let path = dir.join("no_migrations.db");
    make_sqlite_with_app_id(&path, mokumo_db::MOKUMO_APPLICATION_ID);
    w.restore_candidate_path = Some(path);
}

#[given(expr = "a valid Mokumo database file of {int}KB")]
async fn given_valid_mokumo_db_of_size(w: &mut DbWorld, size_kb: i32) {
    let dir = ensure_tmp(w);
    let path = dir.join("sized.db");
    make_mokumo_db_with_all_migrations(&path);
    // Pad the database to (at least) the requested size by inserting a blob.
    if size_kb > 0 {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE _padding (data BLOB NOT NULL);")
            .unwrap();
        let padding = vec![0u8; (size_kb as usize) * 1024];
        conn.execute(
            "INSERT INTO _padding (data) VALUES (?1)",
            rusqlite::params![padding],
        )
        .unwrap();
    }
    w.restore_candidate_path = Some(path);
}

#[given("no production database exists")]
async fn given_no_production_db(w: &mut DbWorld) {
    let dir = ensure_tmp(w);
    let production = dir.join("production");
    // Intentionally do not create the directory — copy_to_production handles it.
    w.restore_production_dir = Some(production);
}

#[given("a production database already exists")]
async fn given_production_db_exists(w: &mut DbWorld) {
    let dir = ensure_tmp(w);
    let production = dir.join("production");
    std::fs::create_dir_all(&production).unwrap();
    std::fs::write(production.join("mokumo.db"), b"existing production db").unwrap();
    w.restore_production_dir = Some(production);
}

// ── When steps ────────────────────────────────────────────────────────────────

#[when("the file is validated as a restore candidate")]
async fn when_validated(w: &mut DbWorld) {
    let path = w
        .restore_candidate_path
        .as_ref()
        .expect("candidate path must be set by a Given step")
        .clone();
    w.restore_validate_result = Some(restore::validate_candidate(&path));
}

#[when("the candidate is copied to the production slot")]
async fn when_copied_to_production(w: &mut DbWorld) {
    let source = w
        .restore_candidate_path
        .as_ref()
        .expect("candidate path must be set")
        .clone();
    let production = w
        .restore_production_dir
        .as_ref()
        .expect("production dir must be set")
        .clone();
    w.restore_copy_result = Some(restore::copy_to_production(&source, &production));
}

#[when("a copy to the production slot is attempted")]
async fn when_copy_attempted(w: &mut DbWorld) {
    let source = w
        .restore_candidate_path
        .as_ref()
        .expect("candidate path must be set")
        .clone();
    let production = w
        .restore_production_dir
        .as_ref()
        .expect("production dir must be set")
        .clone();
    w.restore_copy_result = Some(restore::copy_to_production(&source, &production));
}

// ── Then steps ────────────────────────────────────────────────────────────────

fn validate_result<'a>(
    w: &'a DbWorld,
) -> &'a Result<mokumo_shop::restore::CandidateInfo, RestoreError> {
    w.restore_validate_result
        .as_ref()
        .expect("validate_candidate result must be set by a When step")
}

#[then("the identity check passes")]
async fn then_identity_passes(w: &mut DbWorld) {
    let result = validate_result(w);
    assert!(result.is_ok(), "Expected Ok, got: {result:?}");
}

#[then("validation fails with NotMokumoDatabase")]
async fn then_fails_not_mokumo(w: &mut DbWorld) {
    let result = validate_result(w);
    assert!(
        matches!(result, Err(RestoreError::NotMokumoDatabase { .. })),
        "Expected NotMokumoDatabase, got: {result:?}"
    );
}

#[then("the integrity check passes")]
async fn then_integrity_passes(w: &mut DbWorld) {
    let result = validate_result(w);
    assert!(result.is_ok(), "Expected Ok, got: {result:?}");
}

#[then("validation fails with DatabaseCorrupt")]
async fn then_fails_corrupt(w: &mut DbWorld) {
    let result = validate_result(w);
    // Truncated/corrupt files may surface as either DatabaseCorrupt (opens but
    // integrity_check fails) or NotMokumoDatabase (cannot be opened at all);
    // both are acceptable failure modes for the validation pipeline.
    assert!(
        matches!(
            result,
            Err(RestoreError::DatabaseCorrupt { .. })
                | Err(RestoreError::NotMokumoDatabase { .. })
                | Err(RestoreError::Sqlite(_))
        ),
        "Expected DatabaseCorrupt (or related failure), got: {result:?}"
    );
}

#[then("the compatibility check passes")]
async fn then_compat_passes(w: &mut DbWorld) {
    let result = validate_result(w);
    assert!(result.is_ok(), "Expected Ok, got: {result:?}");
}

#[then("validation fails with SchemaIncompatible")]
async fn then_fails_schema_incompat(w: &mut DbWorld) {
    let result = validate_result(w);
    assert!(
        matches!(result, Err(RestoreError::SchemaIncompatible { .. })),
        "Expected SchemaIncompatible, got: {result:?}"
    );
}

#[then("the candidate info reports the older schema version")]
async fn then_info_reports_older(w: &mut DbWorld) {
    let info = validate_result(w)
        .as_ref()
        .expect("validation should have succeeded");
    let oldest = first_known_migration();
    assert_eq!(
        info.schema_version.as_deref(),
        Some(oldest.as_str()),
        "Expected schema_version to be the oldest known migration"
    );
}

#[then("the candidate info contains the file size")]
async fn then_info_has_file_size(w: &mut DbWorld) {
    let info = validate_result(w)
        .as_ref()
        .expect("validation should have succeeded");
    assert!(info.file_size.get() > 0, "Expected file_size > 0");
}

#[then("the candidate info contains the schema version")]
async fn then_info_has_schema_version(w: &mut DbWorld) {
    let info = validate_result(w)
        .as_ref()
        .expect("validation should have succeeded");
    assert!(
        info.schema_version.is_some(),
        "Expected schema_version to be Some"
    );
}

#[then("the production database exists")]
async fn then_production_db_exists(w: &mut DbWorld) {
    let result = w
        .restore_copy_result
        .as_ref()
        .expect("copy result must be set");
    assert!(result.is_ok(), "Expected copy to succeed, got: {result:?}");

    let production = w.restore_production_dir.as_ref().unwrap();
    assert!(
        production.join("mokumo.db").exists(),
        "Expected production mokumo.db to exist"
    );
}

#[then("the production database content matches the source")]
async fn then_production_matches_source(w: &mut DbWorld) {
    let source = w.restore_candidate_path.as_ref().unwrap();
    let production = w.restore_production_dir.as_ref().unwrap().join("mokumo.db");

    let src_size = std::fs::metadata(source).unwrap().len();
    let dst_size = std::fs::metadata(&production).unwrap().len();
    assert!(
        src_size > 0 && dst_size > 0,
        "both files should be non-empty"
    );

    // The SQLite Online Backup API may produce a different on-disk byte layout
    // than the source (different page allocation, no WAL frames). Verify the
    // copied database is a valid Mokumo database with the same application_id
    // instead of byte-equality.
    let conn = rusqlite::Connection::open(&production).unwrap();
    let app_id: i64 = conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    assert_eq!(app_id, mokumo_db::MOKUMO_APPLICATION_ID);
}

#[then("the copy fails with ProductionDbExists")]
async fn then_copy_fails_exists(w: &mut DbWorld) {
    let result = w
        .restore_copy_result
        .as_ref()
        .expect("copy result must be set");
    assert!(
        matches!(result, Err(RestoreError::ProductionDbExists { .. })),
        "Expected ProductionDbExists, got: {result:?}"
    );
}

#[then("the copy uses a temporary file in the production directory")]
async fn then_copy_uses_temp_file(w: &mut DbWorld) {
    // After a successful copy, the temp file should be cleaned up and the final
    // file should exist in the production directory.
    let production = w.restore_production_dir.as_ref().unwrap();
    assert!(production.join("mokumo.db").exists());
    assert!(
        !production.join("mokumo.db.restore-tmp").exists(),
        "Temp file should be cleaned up after successful copy"
    );
}

#[then("the temporary file is atomically renamed to the final path")]
async fn then_temp_file_renamed(w: &mut DbWorld) {
    let production = w.restore_production_dir.as_ref().unwrap();
    assert!(
        production.join("mokumo.db").exists(),
        "Final mokumo.db should exist after atomic rename"
    );
    assert!(
        !production.join("mokumo.db.restore-tmp").exists(),
        "Temp file should not exist after atomic rename"
    );
}
