use super::*;
use crate::db::initialize_database;

async fn test_db() -> (sea_orm::DatabaseConnection, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
    let db = initialize_database(&url).await.unwrap();
    // Create a seaql_migrations table so pre_migration_backup has something to snapshot.
    let pool = db.get_sqlite_connection_pool();
    sqlx::query(
        "CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO seaql_migrations VALUES ('m20260321_000000_init', 0)")
        .execute(pool)
        .await
        .unwrap();
    (db, tmp)
}

#[tokio::test]
async fn pre_migration_backup_skips_nonexistent_path() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nonexistent.db");
    pre_migration_backup(&path).await.unwrap();
    let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
    assert!(
        entries.next_entry().await.unwrap().is_none(),
        "no files should exist after backup of missing path"
    );
}

#[tokio::test]
async fn pre_migration_backup_skips_when_no_migration_table() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bare.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE foo (id INTEGER)").unwrap();
    }
    pre_migration_backup(&path).await.unwrap();
    let mut count = 0i32;
    let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
    while entries.next_entry().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 1, "only the original DB should exist — no backup");
}

#[tokio::test]
async fn pre_migration_backup_creates_backup_file() {
    let (db, tmp) = test_db().await;
    let path = tmp.path().join("test.db");
    drop(db);

    pre_migration_backup(&path).await.unwrap();

    let mut backups = Vec::new();
    let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains("backup-v") {
            backups.push(name);
        }
    }
    assert_eq!(
        backups.len(),
        1,
        "exactly one backup should have been created"
    );
    assert!(
        backups[0].starts_with("test.db.backup-v"),
        "backup file should be named test.db.backup-v{{version}}"
    );
}

#[tokio::test]
async fn pre_migration_backup_rotates_old_backups() {
    let (db, tmp) = test_db().await;
    let path = tmp.path().join("test.db");
    drop(db);

    // Create 3 fake older backups (sort before real migration names lexicographically)
    for i in 1..=3 {
        let fake = tmp.path().join(format!("test.db.backup-va_old{i}"));
        tokio::fs::write(&fake, b"fake").await.unwrap();
    }

    pre_migration_backup(&path).await.unwrap();

    let mut backups = Vec::new();
    let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains("backup-v") {
            backups.push(name);
        }
    }
    assert_eq!(backups.len(), 3, "rotation should keep only 3 backups");
    assert!(
        !backups.iter().any(|n| n.contains("a_old1")),
        "oldest backup should have been removed"
    );
    assert!(
        backups
            .iter()
            .any(|n| n.starts_with("test.db.backup-v") && !n.contains("a_old")),
        "real backup should be retained"
    );
}

// ── build_backup_path ──────────────────────────────────────────────────

#[test]
fn build_backup_path_appends_version_suffix() {
    let path = Path::new("/tmp/mokumo.db");
    let result = build_backup_path(path, "m20260326_000000_customers").unwrap();
    assert_eq!(
        result,
        PathBuf::from("/tmp/mokumo.db.backup-vm20260326_000000_customers")
    );
}

#[test]
fn build_backup_path_preserves_parent_directory() {
    let path = Path::new("/home/user/data/shop.db");
    let result = build_backup_path(path, "m20260101_000000_init").unwrap();
    assert_eq!(
        result.file_name().unwrap().to_str().unwrap(),
        "shop.db.backup-vm20260101_000000_init"
    );
    assert_eq!(
        result.parent().unwrap().to_str().unwrap(),
        "/home/user/data"
    );
}

// ── collect_existing_backups ───────────────────────────────────────────

#[tokio::test]
async fn collect_existing_backups_empty_when_none_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("fresh.db");
    tokio::fs::write(&db_path, b"dummy").await.unwrap();
    let backups = collect_existing_backups(&db_path).await.unwrap();
    assert!(backups.is_empty(), "no backup files should be found");
}

#[tokio::test]
async fn collect_existing_backups_finds_matching_files_sorted() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260326_z"), b"b3")
        .await
        .unwrap();
    tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260322_a"), b"b1")
        .await
        .unwrap();
    tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260324_m"), b"b2")
        .await
        .unwrap();
    tokio::fs::write(tmp.path().join("other.db.backup-vm20260322_a"), b"ignore")
        .await
        .unwrap();
    let backups = collect_existing_backups(&db_path).await.unwrap();
    assert_eq!(backups.len(), 3);
    let names: Vec<String> = backups
        .iter()
        .map(|(p, _)| p.file_name().unwrap().to_str().unwrap().to_string())
        .collect();
    assert!(names[0].contains("20260322_a"), "oldest first: {names:?}");
    assert!(names[1].contains("20260324_m"), "middle: {names:?}");
    assert!(names[2].contains("20260326_z"), "newest last: {names:?}");
}

// ── rotate_backups ────────────────────────────────────────────────────

#[tokio::test]
async fn rotate_backups_keeps_all_when_within_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let files: Vec<_> = (1..=3)
        .map(|i| tmp.path().join(format!("backup_{i}")))
        .collect();
    for f in &files {
        tokio::fs::write(f, b"x").await.unwrap();
    }
    rotate_backups(files, 3).await;
    let mut count = 0i32;
    let mut entries = tokio::fs::read_dir(tmp.path()).await.unwrap();
    while entries.next_entry().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 3, "all backups should be retained when within limit");
}

#[tokio::test]
async fn rotate_backups_deletes_oldest_when_over_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let files: Vec<_> = ["backup_a", "backup_b", "backup_c", "backup_d"]
        .iter()
        .map(|name| tmp.path().join(name))
        .collect();
    for f in &files {
        tokio::fs::write(f, b"x").await.unwrap();
    }
    rotate_backups(files, 3).await;
    assert!(
        !tmp.path().join("backup_a").exists(),
        "oldest backup should be deleted"
    );
    assert!(tmp.path().join("backup_b").exists());
    assert!(tmp.path().join("backup_c").exists());
    assert!(tmp.path().join("backup_d").exists());
}

// ── collect_existing_backups over-match guard ─────────────────────────

#[tokio::test]
async fn collect_existing_backups_excludes_over_matched_names() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("mokumo.db");
    tokio::fs::write(tmp.path().join("mokumo.db.backup-vm20260322_a"), b"ok")
        .await
        .unwrap();
    tokio::fs::write(tmp.path().join("mokumo.db.foo.backup-vm20260322"), b"no")
        .await
        .unwrap();
    let backups = collect_existing_backups(&db_path).await.unwrap();
    assert_eq!(
        backups.len(),
        1,
        "only exact-prefix backup should match: {backups:?}"
    );
    assert!(
        backups[0]
            .0
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("mokumo.db.backup-v"),
    );
}
