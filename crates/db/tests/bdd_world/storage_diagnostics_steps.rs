use super::DbWorld;
use cucumber::{given, then, when};

// --- Given steps ---

/// Fresh database with no deletions: freelist is negligible, fragmentation < 20%.
#[given("a database with no deleted rows")]
async fn no_deleted_rows(_w: &mut DbWorld) {
    // Default DbWorld starts with a fresh migrated database — no deletions.
}

#[given("an empty newly created database")]
async fn empty_new_database(_w: &mut DbWorld) {
    // Default DbWorld is already an empty newly created database.
}

/// Insert enough rows to occupy multiple pages, then delete all of them so the
/// freelist / page_count ratio exceeds 20 %.
#[given("a database where more than 20 percent of pages are free")]
async fn heavily_fragmented(w: &mut DbWorld) {
    // Create a scratch table that is wide enough for each row to fill most of a page.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _frag_scratch (id INTEGER PRIMARY KEY, data BLOB NOT NULL)",
    )
    .execute(&w.pool)
    .await
    .expect("failed to create scratch table");

    // 4 096 bytes per row × 64 rows ≈ 256 KB → grows across many pages.
    let blob = vec![0xABu8; 4096];
    for _ in 0..64i32 {
        sqlx::query("INSERT INTO _frag_scratch (data) VALUES (?)")
            .bind(&blob)
            .execute(&w.pool)
            .await
            .expect("insert failed");
    }

    // Delete all rows: free pages accumulate in the freelist.
    sqlx::query("DELETE FROM _frag_scratch")
        .execute(&w.pool)
        .await
        .expect("delete failed");
}

/// Create exactly 5 pages of user data then delete exactly 1, leaving a 1/5 = 20 % ratio.
///
/// We target a ratio of exactly 20 % so that the `> 0.20` check returns false.
/// This is inherently approximate because SQLite's B-tree allocator can decide
/// to merge or split pages. The step inserts 4 rows of 4 096 bytes each (page-filling
/// size with page_size=4096 plus overhead), then deletes 1. On typical SQLite builds
/// the resulting freelist_count / page_count is close to 20 % or slightly below; if
/// SQLite happens to consolidate pages the ratio will be even lower, which still means
/// vacuum_needed = false — the only guarantee the scenario requires.
#[given("a database where exactly 20 percent of pages are free")]
async fn boundary_fragmentation(w: &mut DbWorld) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _bound_scratch (id INTEGER PRIMARY KEY, data BLOB NOT NULL)",
    )
    .execute(&w.pool)
    .await
    .expect("failed to create scratch table");

    let blob = vec![0xCDu8; 4096];
    for _ in 0..5i32 {
        sqlx::query("INSERT INTO _bound_scratch (data) VALUES (?)")
            .bind(&blob)
            .execute(&w.pool)
            .await
            .expect("insert failed");
    }
    // Delete 1 row → at most 1/5 of pages are freed (≤ 20%).
    sqlx::query("DELETE FROM _bound_scratch WHERE id = (SELECT MIN(id) FROM _bound_scratch)")
        .execute(&w.pool)
        .await
        .expect("delete failed");
}

/// Remove the WAL file if it exists so wal_size_bytes == 0.
#[given("a database with no WAL file present")]
async fn no_wal_file(w: &mut DbWorld) {
    // WAL file is "{db_path}-wal". Removing it is safe for a test database that is
    // not currently using WAL journal mode for this connection (we hold an SQLx pool
    // in WAL mode, so the WAL should be empty after checkpoint).
    let wal_path = {
        let mut p = w.db_path.as_os_str().to_owned();
        p.push("-wal");
        std::path::PathBuf::from(p)
    };
    // Force a checkpoint + WAL truncation before removing.
    let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&w.pool)
        .await;
    // Remove the WAL file (ignore if already absent).
    let _ = std::fs::remove_file(&wal_path);
}

/// Write to the database to ensure a WAL file exists, and record its expected size.
#[given("a database with an active WAL file of known size")]
async fn active_wal_file(w: &mut DbWorld) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _wal_scratch (id INTEGER PRIMARY KEY, data TEXT NOT NULL)",
    )
    .execute(&w.pool)
    .await
    .expect("create table failed");

    // Write enough rows to ensure the WAL file is non-empty.
    for i in 0..100i32 {
        sqlx::query("INSERT INTO _wal_scratch (data) VALUES (?)")
            .bind(format!("row_{i}"))
            .execute(&w.pool)
            .await
            .expect("insert failed");
    }

    // Read the actual WAL size after writing.
    let wal_path = {
        let mut p = w.db_path.as_os_str().to_owned();
        p.push("-wal");
        std::path::PathBuf::from(p)
    };
    w.known_wal_size = std::fs::metadata(&wal_path).ok().map(|m| m.len());
}

// --- When steps ---

#[when("storage diagnostics are collected")]
async fn collect_storage_diagnostics(w: &mut DbWorld) {
    let db_path = w.db_path.clone();
    let result = tokio::task::spawn_blocking(move || mokumo_db::diagnose_database(&db_path))
        .await
        .expect("spawn_blocking panicked");
    w.last_db_diagnostics = Some(result);
}

// --- Then steps ---

#[then("vacuum_needed is false")]
async fn vacuum_not_needed(w: &mut DbWorld) {
    let diag = w
        .last_db_diagnostics
        .as_ref()
        .expect("no diagnostics collected")
        .as_ref()
        .expect("diagnose_database failed");
    let vacuum_needed =
        diag.page_count > 0 && (diag.freelist_count as f64 / diag.page_count as f64) > 0.20;
    assert!(
        !vacuum_needed,
        "Expected vacuum_needed=false, got freelist={}/{} ({:.1}%)",
        diag.freelist_count,
        diag.page_count,
        if diag.page_count > 0 {
            diag.freelist_count as f64 / diag.page_count as f64 * 100.0
        } else {
            0.0
        }
    );
}

#[then("vacuum_needed is true")]
async fn vacuum_needed(w: &mut DbWorld) {
    let diag = w
        .last_db_diagnostics
        .as_ref()
        .expect("no diagnostics collected")
        .as_ref()
        .expect("diagnose_database failed");
    let vacuum_needed =
        diag.page_count > 0 && (diag.freelist_count as f64 / diag.page_count as f64) > 0.20;
    assert!(
        vacuum_needed,
        "Expected vacuum_needed=true, got freelist={}/{} ({:.1}%)",
        diag.freelist_count,
        diag.page_count,
        if diag.page_count > 0 {
            diag.freelist_count as f64 / diag.page_count as f64 * 100.0
        } else {
            0.0
        }
    );
}

#[then("wal_size_bytes is 0")]
async fn wal_size_is_zero(w: &mut DbWorld) {
    let diag = w
        .last_db_diagnostics
        .as_ref()
        .expect("no diagnostics collected")
        .as_ref()
        .expect("diagnose_database failed");
    assert_eq!(
        diag.wal_size_bytes, 0,
        "Expected wal_size_bytes=0, got {}",
        diag.wal_size_bytes
    );
}

#[then("wal_size_bytes matches the size of the WAL file")]
async fn wal_size_matches(w: &mut DbWorld) {
    let diag = w
        .last_db_diagnostics
        .as_ref()
        .expect("no diagnostics collected")
        .as_ref()
        .expect("diagnose_database failed");
    let expected = w
        .known_wal_size
        .expect("known_wal_size not set — use 'Given a database with an active WAL file'");
    assert_eq!(
        diag.wal_size_bytes, expected,
        "Expected wal_size_bytes={expected}, got {}",
        diag.wal_size_bytes
    );
}
