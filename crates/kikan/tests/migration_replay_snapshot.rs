//! Replay-safety proof for the migration-runner bootstrap + backfill path.
//!
//! Runs the kikan migration runner's bootstrap + backfill against the
//! `tests/fixtures/pre-stage3.sqlite` snapshot and asserts that every
//! migration recorded in `seaql_migrations` is backfilled into
//! `kikan_migrations` under `graft_id = "mokumo"` without any migration
//! being re-executed.
//!
//! Load-bearing for R11 (migration continuity) and R12 (session continuity):
//! the runner keys on `(graft_id, name)`, so a migration's file location
//! may change without invalidating previously-stamped databases.

use std::path::PathBuf;

use kikan::GraftId;
use kikan::migrations::runner::run_migrations_with_backfill;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/pre-stage3.sqlite")
        .canonicalize()
        .expect("pre-stage3.sqlite fixture must exist (run scripts/capture-pre-stage3-fixture.sh)")
}

/// Names of the migrations applied in the pre-Stage-3 snapshot, in the
/// order they were applied. Matches `SELECT version FROM seaql_migrations
/// ORDER BY applied_at` against the captured fixture.
const PRE_STAGE3_MIGRATIONS: &[&str] = &[
    "m20260321_000000_init",
    "m20260322_000000_settings",
    "m20260324_000000_number_sequences",
    "m20260324_000001_customers_and_activity",
    "m20260326_000000_customers_deleted_at_index",
    "m20260327_000000_users_and_roles",
    "m20260404_000000_set_pragmas",
    "m20260411_000000_shop_settings",
];

#[tokio::test]
async fn pre_stage3_snapshot_backfills_without_re_running_migrations() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::copy(fixture_path(), tmp.path()).unwrap();

    let url = format!("sqlite:{}?mode=rwc", tmp.path().display());
    let conn = kikan::db::initialize_database(&url).await.unwrap();
    let pool = conn.get_sqlite_connection_pool();

    // Pre-replay: the snapshot has the 8 pre-Stage-3 migrations in
    // seaql_migrations and no kikan_migrations table at all.
    let pre_seaql: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM seaql_migrations")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        pre_seaql as usize,
        PRE_STAGE3_MIGRATIONS.len(),
        "pre-Stage-3 snapshot must carry all {} original migrations",
        PRE_STAGE3_MIGRATIONS.len()
    );

    let kikan_table_pre: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='kikan_migrations'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        kikan_table_pre, 0,
        "pre-Stage-3 snapshot must not yet have a kikan_migrations table"
    );

    // Act: run the kikan migration runner with an empty migration list and
    // the backfill hint. This exercises bootstrap + backfill only — no
    // migration is re-executed (the list is empty).
    run_migrations_with_backfill(&conn, &[], Some(GraftId::new("mokumo")))
        .await
        .expect("bootstrap + backfill must succeed against a pre-Stage-3 snapshot");

    // Post-replay: all 8 pre-Stage-3 migrations are present in
    // kikan_migrations under graft_id="mokumo", in the same order, with
    // kikan-internal bootstrap rows under graft_id="kikan".
    let mokumo_rows: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM kikan_migrations WHERE graft_id='mokumo' ORDER BY applied_at",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mokumo_names: Vec<&str> = mokumo_rows.iter().map(String::as_str).collect();
    assert_eq!(
        mokumo_names, PRE_STAGE3_MIGRATIONS,
        "every pre-Stage-3 migration must be backfilled under graft_id=mokumo \
         in the same order, without any re-run or re-ordering"
    );

    // Replay is idempotent: running the backfill again must not duplicate rows.
    run_migrations_with_backfill(&conn, &[], Some(GraftId::new("mokumo")))
        .await
        .expect("second backfill pass must be a no-op");

    let rerun_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM kikan_migrations WHERE graft_id='mokumo'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        rerun_count as usize,
        PRE_STAGE3_MIGRATIONS.len(),
        "second backfill must not duplicate the mokumo rows"
    );
}

/// Post-S2.5 replay safety: a *new* migration added after the snapshot
/// capture applies cleanly on top of the backfilled set without
/// disturbing the already-applied rows. This guards against the
/// regression where relocating migration files would inadvertently
/// re-key the runner's dedup logic (e.g. by path instead of by
/// `(graft_id, name)`), causing a previously-applied migration to be
/// treated as new.
#[tokio::test]
async fn net_new_migration_applies_on_top_of_backfilled_snapshot() {
    use std::sync::Arc;

    use async_trait::async_trait;
    use kikan::migrations::conn::MigrationConn;
    use kikan::{Migration, MigrationRef, MigrationTarget};
    use sea_orm::{ConnectionTrait, DbErr};

    struct SyntheticMigration;

    #[async_trait]
    impl Migration for SyntheticMigration {
        fn name(&self) -> &'static str {
            "m99999999_999999_post_stage3_probe"
        }

        fn graft_id(&self) -> kikan::GraftId {
            kikan::GraftId::new("mokumo")
        }

        fn target(&self) -> MigrationTarget {
            MigrationTarget::PerProfile
        }

        fn dependencies(&self) -> Vec<MigrationRef> {
            // Intentionally empty: the DAG resolver only checks refs within
            // the `all_migrations` slice. Pre-Stage-3 migrations are not in
            // the slice (they live in kikan_migrations via backfill), so
            // referencing them here would produce DanglingRef. Ordering
            // between the backfilled rows and this probe is governed by
            // applied_at, not by the DAG.
            vec![]
        }

        async fn up(&self, conn: &MigrationConn) -> Result<(), DbErr> {
            conn.schema_manager()
                .get_connection()
                .execute_unprepared("CREATE TABLE post_stage3_probe (id INTEGER PRIMARY KEY)")
                .await?;
            Ok(())
        }
    }

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::copy(fixture_path(), tmp.path()).unwrap();

    let url = format!("sqlite:{}?mode=rwc", tmp.path().display());
    let conn = kikan::db::initialize_database(&url).await.unwrap();
    let pool = conn.get_sqlite_connection_pool();

    let new_migration: Arc<dyn Migration> = Arc::new(SyntheticMigration);

    run_migrations_with_backfill(
        &conn,
        std::slice::from_ref(&new_migration),
        Some(kikan::GraftId::new("mokumo")),
    )
    .await
    .expect("backfill + net-new migration must apply without error");

    let mokumo_rows: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM kikan_migrations WHERE graft_id='mokumo' ORDER BY applied_at, name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(
        mokumo_rows.len(),
        PRE_STAGE3_MIGRATIONS.len() + 1,
        "pre-Stage-3 rows + synthetic probe must both be present"
    );
    assert!(
        mokumo_rows
            .iter()
            .any(|n| n == "m99999999_999999_post_stage3_probe"),
        "synthetic migration must land in kikan_migrations"
    );

    let probe_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='post_stage3_probe'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        probe_count, 1,
        "the probe migration's CREATE TABLE side-effect must be visible"
    );

    // Idempotent replay: running once more must not re-apply the probe or
    // duplicate any row.
    run_migrations_with_backfill(
        &conn,
        std::slice::from_ref(&new_migration),
        Some(kikan::GraftId::new("mokumo")),
    )
    .await
    .expect("second pass must be a no-op");

    let final_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM kikan_migrations WHERE graft_id='mokumo'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        final_count as usize,
        PRE_STAGE3_MIGRATIONS.len() + 1,
        "idempotent replay must not duplicate the synthetic row"
    );
}
