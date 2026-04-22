//! Domain-specific lifecycle functions for backup, restore, and reset.
//!
//! Called via `Graft` lifecycle hooks from kikan-cli commands.
//! These handle shop-specific artifacts (logo files) that live
//! alongside the database but aren't part of the SQLite backup.

use std::path::Path;

/// Supported logo file extensions.
const LOGO_EXTENSIONS: &[&str] = &["png", "jpeg", "webp"];

/// Copy the shop logo as a sibling file alongside a backup archive.
///
/// Reads `logo_extension` from `shop_settings` in the backup database,
/// then copies `{db_dir}/logo.{ext}` → `{backup_path}.logo.{ext}`.
/// Non-fatal: logs a warning and continues on failure.
pub fn copy_logo_to_backup(db_path: &Path, backup_path: &Path) {
    let db_dir = db_path.parent().unwrap_or(Path::new("."));

    if let Some(ext) = read_logo_extension(backup_path) {
        let logo_src = db_dir.join(format!("logo.{ext}"));
        if !logo_src.exists() {
            return;
        }
        let logo_dst = backup_path.with_extension(format!("logo.{ext}"));
        if let Err(e) = std::fs::copy(&logo_src, &logo_dst) {
            tracing::warn!(
                "copy_logo_to_backup: could not copy {:?} → {:?}: {e}",
                logo_src,
                logo_dst
            );
        }
    }
}

/// Remove stale logo files from a directory.
///
/// Sweeps `logo.{png,jpeg,webp}` — used before restoring a logo
/// to prevent orphan files when the extension has changed.
pub fn sweep_stale_logos(dir: &Path) {
    for ext in LOGO_EXTENSIONS {
        let stale = dir.join(format!("logo.{ext}"));
        if stale.exists()
            && let Err(e) = std::fs::remove_file(&stale)
        {
            tracing::warn!("sweep_stale_logos: could not remove {:?}: {e}", stale);
        }
    }
}

/// Restore the shop logo from a backup's sibling file.
///
/// Sweeps stale logos first, then copies the logo sibling
/// (`{backup_path}.logo.{ext}`) to `{db_dir}/logo.{ext}`.
/// Non-fatal: logs a warning and continues on failure.
pub fn restore_logo_from_backup(db_path: &Path, backup_path: &Path) {
    let db_dir = db_path.parent().unwrap_or(Path::new("."));

    sweep_stale_logos(db_dir);

    if let Some(ext) = read_logo_extension(backup_path) {
        let sibling = backup_path.with_extension(format!("logo.{ext}"));
        if sibling.exists() {
            let logo_dst = db_dir.join(format!("logo.{ext}"));
            if let Err(e) = std::fs::copy(&sibling, &logo_dst) {
                tracing::warn!(
                    "restore_logo_from_backup: could not copy {:?} → {:?}: {e}",
                    sibling,
                    logo_dst
                );
            }
        }
    }
}

/// Clean up domain-specific artifacts from a profile directory during reset.
pub fn cleanup_domain_artifacts(profile_dir: &Path) {
    sweep_stale_logos(profile_dir);
}

/// Filename prefix for file-drop password-reset HTML files.
///
/// Kept here so both the reset handler (writer) and the reset-db
/// cleanup (reader/remover) agree on the pattern.
pub const RECOVERY_FILE_PREFIX: &str = "mokumo-recovery-";

/// Error surfaced by [`cleanup_recovery_files`] — a single pair of
/// `(path, io::Error)` summarizing a scan or remove failure. The caller
/// (the `on_post_reset_db` hook) surfaces the first failure through
/// `Graft::on_post_reset_db`'s `Result<(), String>` contract; per-file
/// remove errors are logged inline and the sweep continues.
#[derive(Debug)]
pub struct RecoveryCleanupError {
    pub path: std::path::PathBuf,
    pub source: std::io::Error,
}

impl std::fmt::Display for RecoveryCleanupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "recovery cleanup failed on {}: {}",
            self.path.display(),
            self.source
        )
    }
}

/// Remove every `mokumo-recovery-*.html` file in `recovery_dir`.
///
/// - `NotFound` on the directory itself is OK (idempotent — reset
///   before anyone ever triggered a password-reset).
/// - A read-dir failure surfaces as `Err(RecoveryCleanupError)` so the
///   caller can relay it up through `Graft::on_post_reset_db`.
/// - Per-entry errors (bad filename, remove failure) are logged via
///   `tracing::warn!` and the sweep continues — the intent is
///   best-effort cleanup, not transactional deletion.
pub fn cleanup_recovery_files(recovery_dir: &Path) -> Result<(), RecoveryCleanupError> {
    let entries = match std::fs::read_dir(recovery_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(RecoveryCleanupError {
                path: recovery_dir.to_path_buf(),
                source: e,
            });
        }
    };

    for entry_result in entries {
        remove_recovery_entry_if_match(entry_result, recovery_dir);
    }
    Ok(())
}

// Extracted to keep `cleanup_recovery_files` under the per-function
// complexity budget (the CRAP gate counts each branch + continue here).
fn remove_recovery_entry_if_match(
    entry_result: std::io::Result<std::fs::DirEntry>,
    recovery_dir: &Path,
) {
    let entry = match entry_result {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(
                dir = %recovery_dir.display(),
                "cleanup_recovery_files: read-dir entry failed: {e}"
            );
            return;
        }
    };
    let name = entry.file_name();
    let Some(name_str) = name.to_str() else {
        return;
    };
    if !name_str.starts_with(RECOVERY_FILE_PREFIX) || !name_str.ends_with(".html") {
        return;
    }
    let path = entry.path();
    if let Err(e) = std::fs::remove_file(&path) {
        tracing::warn!(
            path = %path.display(),
            "cleanup_recovery_files: remove failed: {e}"
        );
    }
}

/// Read the `logo_extension` from `shop_settings` in a SQLite database.
///
/// Returns `None` if the table doesn't exist, the column is NULL,
/// or the database can't be opened.
fn read_logo_extension(db_path: &Path) -> Option<String> {
    let conn =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .ok()?;
    let ext = conn
        .query_row(
            "SELECT logo_extension FROM shop_settings WHERE id = 1 AND logo_extension IS NOT NULL",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()?;
    // Validate against the allowlist to prevent path traversal via crafted DB values.
    if LOGO_EXTENSIONS.contains(&ext.as_str()) {
        Some(ext)
    } else {
        tracing::warn!("read_logo_extension: unexpected extension {ext:?}, ignoring");
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_removes_existing_logo_files() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path();

        std::fs::write(profile.join("logo.png"), b"fake-png").unwrap();
        std::fs::write(profile.join("logo.jpeg"), b"fake-jpeg").unwrap();
        std::fs::write(profile.join("other.txt"), b"keep-me").unwrap();

        sweep_stale_logos(profile);

        assert!(!profile.join("logo.png").exists());
        assert!(!profile.join("logo.jpeg").exists());
        assert!(
            profile.join("other.txt").exists(),
            "non-logo files untouched"
        );
    }

    #[test]
    fn sweep_is_idempotent_on_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Should not panic or error
        sweep_stale_logos(dir.path());
    }

    #[test]
    fn cleanup_domain_artifacts_removes_logos() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("logo.webp"), b"fake").unwrap();

        cleanup_domain_artifacts(dir.path());

        assert!(!dir.path().join("logo.webp").exists());
    }

    #[test]
    fn read_logo_extension_returns_none_for_nonexistent_db() {
        assert!(read_logo_extension(Path::new("/nonexistent.db")).is_none());
    }

    #[test]
    fn read_logo_extension_returns_none_for_missing_table() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("empty.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE other (id INTEGER)")
            .unwrap();
        drop(conn);

        assert!(read_logo_extension(&db_path).is_none());
    }

    #[test]
    fn copy_logo_to_backup_copies_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join("production");
        std::fs::create_dir_all(&db_dir).unwrap();

        // Create a DB with shop_settings
        let db_path = db_dir.join("mokumo.db");
        let backup_path = dir.path().join("backup.db");

        // Create backup DB with logo_extension set
        let conn = rusqlite::Connection::open(&backup_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE shop_settings (id INTEGER PRIMARY KEY, logo_extension TEXT);
             INSERT INTO shop_settings (id, logo_extension) VALUES (1, 'png');",
        )
        .unwrap();
        drop(conn);

        // Create the source logo file
        std::fs::write(db_dir.join("logo.png"), b"logo-data").unwrap();

        copy_logo_to_backup(&db_path, &backup_path);

        let sibling = backup_path.with_extension("logo.png");
        assert!(sibling.exists(), "logo sibling should be created");
        assert_eq!(std::fs::read(&sibling).unwrap(), b"logo-data");
    }

    #[test]
    fn cleanup_recovery_files_removes_matching_files() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path();

        std::fs::write(recovery.join("mokumo-recovery-abc123.html"), b"recovery").unwrap();
        std::fs::write(recovery.join("mokumo-recovery-def456.html"), b"recovery").unwrap();
        std::fs::write(recovery.join("other-file.txt"), b"keep").unwrap();

        cleanup_recovery_files(recovery).unwrap();

        assert!(
            !recovery.join("mokumo-recovery-abc123.html").exists(),
            "first recovery file should be removed"
        );
        assert!(
            !recovery.join("mokumo-recovery-def456.html").exists(),
            "second recovery file should be removed"
        );
        assert!(
            recovery.join("other-file.txt").exists(),
            "non-matching files should be untouched"
        );
    }

    #[test]
    fn cleanup_recovery_files_leaves_mismatched_prefix_alone() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path();

        std::fs::write(recovery.join("mokumo-recovery-nohtml.txt"), b"x").unwrap();
        std::fs::write(recovery.join("mokumo-recovery-.html"), b"x").unwrap();
        std::fs::write(recovery.join("other-recovery-abc.html"), b"x").unwrap();

        cleanup_recovery_files(recovery).unwrap();

        // `.txt` extension → not matched
        assert!(recovery.join("mokumo-recovery-nohtml.txt").exists());
        // No hash between prefix and suffix → still matches the pattern
        // (consistent with the previous kikan-cli implementation)
        assert!(!recovery.join("mokumo-recovery-.html").exists());
        // Wrong prefix → untouched
        assert!(recovery.join("other-recovery-abc.html").exists());
    }

    #[test]
    fn cleanup_recovery_files_is_idempotent_on_missing_dir() {
        // Nonexistent recovery dir is a normal post-reset state — no
        // password-reset was ever requested. Silently Ok.
        let result = cleanup_recovery_files(Path::new("/nonexistent/recovery"));
        assert!(result.is_ok());
    }

    #[test]
    fn restore_logo_copies_from_backup_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join("production");
        std::fs::create_dir_all(&db_dir).unwrap();

        let db_path = db_dir.join("mokumo.db");
        let backup_path = dir.path().join("backup.db");

        // Create backup DB with logo_extension
        let conn = rusqlite::Connection::open(&backup_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE shop_settings (id INTEGER PRIMARY KEY, logo_extension TEXT);
             INSERT INTO shop_settings (id, logo_extension) VALUES (1, 'jpeg');",
        )
        .unwrap();
        drop(conn);

        // Create the backup sibling logo file
        let sibling = backup_path.with_extension("logo.jpeg");
        std::fs::write(&sibling, b"restored-logo").unwrap();

        // Create a stale logo that should be swept
        std::fs::write(db_dir.join("logo.png"), b"stale").unwrap();

        restore_logo_from_backup(&db_path, &backup_path);

        assert!(
            db_dir.join("logo.jpeg").exists(),
            "restored logo should exist"
        );
        assert_eq!(
            std::fs::read(db_dir.join("logo.jpeg")).unwrap(),
            b"restored-logo"
        );
        assert!(
            !db_dir.join("logo.png").exists(),
            "stale logo should be swept"
        );
    }
}
