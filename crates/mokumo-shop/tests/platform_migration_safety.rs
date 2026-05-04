use kikan::backup::pre_migration_backup;
use mokumo_shop::db::initialize_database;

#[tokio::test]
async fn no_backup_on_first_run() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // Database file does not exist yet — should skip silently
    let result = pre_migration_backup(&db_path).await;
    assert!(
        result.is_ok(),
        "Should succeed (skip) when no DB file exists"
    );

    // Verify no backup files were created
    let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
    let mut count = 0;
    while entries.next_entry().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 0, "No files should exist on first run");
}

#[tokio::test]
async fn backup_created_with_correct_name() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // Create and initialize the database so seaql_migrations exists
    let db = initialize_database(&url).await.unwrap();
    drop(db);

    // Query the max version from seaql_migrations (TEXT column)
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let version: String = conn
        .query_row("SELECT MAX(version) FROM seaql_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    drop(conn);

    // Run the backup
    pre_migration_backup(&db_path).await.unwrap();

    // Verify backup file exists with correct name
    let expected_backup = dir.path().join(format!("test.db.backup-v{version}"));
    assert!(
        expected_backup.exists(),
        "Backup file should exist at {expected_backup:?}"
    );
}

#[tokio::test]
async fn backup_rotation_keeps_only_last_three() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // Create and initialize the database
    let db = initialize_database(&url).await.unwrap();
    drop(db);

    // Create 4 fake old backup files (simulating previous versions)
    for v in 1..=4 {
        let backup_name = format!("test.db.backup-v{v}");
        let backup_path = dir.path().join(&backup_name);
        tokio::fs::write(&backup_path, b"fake backup")
            .await
            .unwrap();
    }

    // Now run a real backup (this will create a 5th backup file)
    pre_migration_backup(&db_path).await.unwrap();

    // Count remaining backup files
    let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
    let mut backups: Vec<String> = Vec::new();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_str().unwrap().to_string();
        if name.starts_with("test.db.") && name.contains("backup-v") {
            backups.push(name);
        }
    }

    assert_eq!(
        backups.len(),
        3,
        "Should keep only 3 backups, found: {backups:?}"
    );
}

#[tokio::test]
async fn backup_skipped_when_db_exists_but_no_migrations_table() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // Create a plain SQLite file with no seaql_migrations table
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute("CREATE TABLE dummy (id INTEGER PRIMARY KEY)", [])
        .unwrap();
    drop(conn);

    let result = pre_migration_backup(&db_path).await;
    assert!(
        result.is_ok(),
        "Should succeed (skip) when no migrations table exists"
    );

    // Verify no backup files were created
    let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
    let mut backup_count = 0;
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_str().unwrap().to_string();
        if name.contains("backup-v") {
            backup_count += 1;
        }
    }
    assert_eq!(
        backup_count, 0,
        "No backup files should exist when migrations table is missing"
    );
}
