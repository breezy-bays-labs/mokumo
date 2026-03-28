use std::fs;
use std::path::Path;

use mokumo_api::cli_reset_db;

/// Helper: create an empty file at the given path.
fn touch(path: &Path) {
    fs::write(path, b"").unwrap();
}

#[test]
fn reset_db_deletes_main_db_and_sidecars() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();
    let recovery_dir = tmp.path().join("recovery");
    fs::create_dir(&recovery_dir).unwrap();

    touch(&data_dir.join("mokumo.db"));
    touch(&data_dir.join("mokumo.db-wal"));
    touch(&data_dir.join("mokumo.db-shm"));

    let report = cli_reset_db(data_dir, &recovery_dir, false).unwrap();

    assert_eq!(report.deleted.len(), 3);
    assert!(report.failed.is_empty());
    // -journal was never created, should be not_found
    assert_eq!(report.not_found.len(), 1);
    assert!(!data_dir.join("mokumo.db").exists());
    assert!(!data_dir.join("mokumo.db-wal").exists());
    assert!(!data_dir.join("mokumo.db-shm").exists());
}

#[test]
fn reset_db_not_found_files_in_report() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();
    let recovery_dir = tmp.path().join("recovery");
    fs::create_dir(&recovery_dir).unwrap();

    // Only create the main db — sidecars don't exist
    touch(&data_dir.join("mokumo.db"));

    let report = cli_reset_db(data_dir, &recovery_dir, false).unwrap();

    assert_eq!(report.deleted.len(), 1);
    // -wal, -shm, -journal should all be not_found
    assert_eq!(report.not_found.len(), 3);
    let not_found_names: Vec<String> = report
        .not_found
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(not_found_names.contains(&"mokumo.db-wal".to_string()));
    assert!(not_found_names.contains(&"mokumo.db-shm".to_string()));
    assert!(not_found_names.contains(&"mokumo.db-journal".to_string()));
}

#[test]
fn reset_db_preserves_backups_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();
    let recovery_dir = tmp.path().join("recovery");
    fs::create_dir(&recovery_dir).unwrap();

    touch(&data_dir.join("mokumo.db"));
    touch(&data_dir.join("mokumo.db.backup-v1"));

    let report = cli_reset_db(data_dir, &recovery_dir, false).unwrap();

    // Backup should NOT be in deleted list
    assert!(
        !report
            .deleted
            .iter()
            .any(|p| p.file_name().unwrap().to_string_lossy().contains("backup"))
    );
    // Backup file should still exist on disk
    assert!(data_dir.join("mokumo.db.backup-v1").exists());
}

#[test]
fn reset_db_deletes_backups_when_flag_set() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();
    let recovery_dir = tmp.path().join("recovery");
    fs::create_dir(&recovery_dir).unwrap();

    touch(&data_dir.join("mokumo.db"));
    touch(&data_dir.join("mokumo.db.backup-v1"));
    touch(&data_dir.join("mokumo.db.backup-v2"));

    let report = cli_reset_db(data_dir, &recovery_dir, true).unwrap();

    let deleted_names: Vec<String> = report
        .deleted
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(deleted_names.contains(&"mokumo.db.backup-v1".to_string()));
    assert!(deleted_names.contains(&"mokumo.db.backup-v2".to_string()));
    assert!(!data_dir.join("mokumo.db.backup-v1").exists());
    assert!(!data_dir.join("mokumo.db.backup-v2").exists());
}

#[test]
fn reset_db_cleans_recovery_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();
    let recovery_dir = tmp.path().join("recovery");
    fs::create_dir(&recovery_dir).unwrap();

    touch(&data_dir.join("mokumo.db"));
    // Only mokumo-recovery-*.html files should be deleted
    touch(&recovery_dir.join("mokumo-recovery-abc123.html"));
    touch(&recovery_dir.join("mokumo-recovery-def456.html"));

    let report = cli_reset_db(data_dir, &recovery_dir, false).unwrap();

    let deleted_names: Vec<String> = report
        .deleted
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(deleted_names.contains(&"mokumo-recovery-abc123.html".to_string()));
    assert!(deleted_names.contains(&"mokumo-recovery-def456.html".to_string()));
    assert!(!recovery_dir.join("mokumo-recovery-abc123.html").exists());
    assert!(!recovery_dir.join("mokumo-recovery-def456.html").exists());
}

#[test]
fn reset_db_preserves_non_recovery_files_in_recovery_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();
    let recovery_dir = tmp.path().join("recovery");
    fs::create_dir(&recovery_dir).unwrap();

    touch(&data_dir.join("mokumo.db"));
    // These should be deleted (match pattern)
    touch(&recovery_dir.join("mokumo-recovery-abc123.html"));
    // These should NOT be deleted (wrong prefix, wrong extension, or unrelated)
    touch(&recovery_dir.join("important-document.pdf"));
    touch(&recovery_dir.join("mokumo-recovery-abc123.txt"));
    touch(&recovery_dir.join("other-recovery-file.html"));

    let report = cli_reset_db(data_dir, &recovery_dir, false).unwrap();

    let deleted_names: Vec<String> = report
        .deleted
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(deleted_names.contains(&"mokumo-recovery-abc123.html".to_string()));
    // Non-matching files must survive
    assert!(recovery_dir.join("important-document.pdf").exists());
    assert!(recovery_dir.join("mokumo-recovery-abc123.txt").exists());
    assert!(recovery_dir.join("other-recovery-file.html").exists());
}

#[test]
fn reset_db_handles_nonexistent_recovery_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();
    let recovery_dir = tmp.path().join("does-not-exist");

    touch(&data_dir.join("mokumo.db"));

    // Should not error — nonexistent recovery dir is gracefully skipped
    let report = cli_reset_db(data_dir, &recovery_dir, false).unwrap();
    assert!(report.deleted.contains(&data_dir.join("mokumo.db")));
}

#[test]
fn reset_db_empty_data_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();
    let recovery_dir = tmp.path().join("recovery");
    fs::create_dir(&recovery_dir).unwrap();

    // No files at all — everything should be not_found
    let report = cli_reset_db(data_dir, &recovery_dir, false).unwrap();

    assert!(report.deleted.is_empty());
    assert!(report.failed.is_empty());
    // All 4 sidecar paths should be not_found
    assert_eq!(report.not_found.len(), 4);
}

#[test]
fn reset_db_ignores_subdirectory_in_recovery_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();
    let recovery_dir = tmp.path().join("recovery");
    fs::create_dir(&recovery_dir).unwrap();

    touch(&data_dir.join("mokumo.db"));
    // Subdirectory doesn't match mokumo-recovery-*.html — should be ignored entirely
    fs::create_dir(recovery_dir.join("unexpected-subdir")).unwrap();
    // A matching file should still be deleted
    touch(&recovery_dir.join("mokumo-recovery-abc123.html"));

    let report = cli_reset_db(data_dir, &recovery_dir, false).unwrap();

    // Subdirectory is not in failed (it's skipped by the filter, not attempted)
    assert!(report.failed.is_empty());
    // Matching file was deleted
    assert!(
        report
            .deleted
            .iter()
            .any(|p| p.file_name().unwrap().to_string_lossy() == "mokumo-recovery-abc123.html")
    );
    // Subdirectory still exists
    assert!(recovery_dir.join("unexpected-subdir").exists());
}
