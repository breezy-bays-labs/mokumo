use kikan::db::ensure_auto_vacuum;
use mokumo_shop::db::initialize_database;
use sqlx::Row;

#[tokio::test]
async fn pragmas_are_set_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // ensure_auto_vacuum must run before pool creation (auto_vacuum is a
    // file-header PRAGMA, not a per-connection PRAGMA).
    ensure_auto_vacuum(&db_path).unwrap();
    let db = initialize_database(&url).await.unwrap();
    let pool = db.get_sqlite_connection_pool();

    let journal: String = sqlx::query("PRAGMA journal_mode")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(journal.to_lowercase(), "wal");

    let synchronous: i32 = sqlx::query("PRAGMA synchronous")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(synchronous, 1); // NORMAL

    let busy_timeout: i32 = sqlx::query("PRAGMA busy_timeout")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(busy_timeout, 5000);

    let foreign_keys: i32 = sqlx::query("PRAGMA foreign_keys")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(foreign_keys, 1); // ON

    let cache_size: i32 = sqlx::query("PRAGMA cache_size")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(cache_size, -64000);

    let auto_vacuum: i32 = sqlx::query("PRAGMA auto_vacuum")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(auto_vacuum, 2); // INCREMENTAL

    let mmap_size: i64 = sqlx::query("PRAGMA mmap_size")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    // mmap_size is platform-conditional: 256 MB on Linux, disabled (0) elsewhere.
    assert_eq!(mmap_size, kikan::db::CONFIGURED_MMAP_SIZE);

    drop(db);
}

#[tokio::test]
async fn database_creation_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // First initialization
    ensure_auto_vacuum(&db_path).unwrap();
    let db1 = initialize_database(&url).await.unwrap();
    drop(db1);

    // Second initialization on same file
    ensure_auto_vacuum(&db_path).unwrap();
    let _db2 = initialize_database(&url).await.unwrap();
}
