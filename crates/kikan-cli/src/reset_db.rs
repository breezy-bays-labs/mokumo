//! CLI reset-db command — delete database files, sidecars, backups, and recovery files.
//!
//! Pure filesystem operation (no daemon required). Dispatches the
//! `Graft::on_post_reset_db` lifecycle hook for domain-specific cleanup.

use std::path::{Path, PathBuf};

use kikan::Graft;

use crate::CliError;

/// SQLite sidecar suffixes deleted alongside the main database file.
pub const DB_SIDECAR_SUFFIXES: &[&str] = &["", "-wal", "-shm", "-journal"];

/// Report from a database reset operation.
#[derive(Debug, Default)]
pub struct ResetReport {
    pub deleted: Vec<PathBuf>,
    pub not_found: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, std::io::Error)>,
    pub recovery_dir_error: Option<(PathBuf, std::io::Error)>,
    pub backup_dir_error: Option<(PathBuf, std::io::Error)>,
}

/// Delete database files, sidecars, and optionally backups + recovery files.
///
/// `profile_dir` is the directory containing `mokumo.db` for the target profile
/// (e.g. `data_dir/demo` or `data_dir/production`).
///
/// After filesystem cleanup, dispatches `graft.on_post_reset_db()` for
/// domain-specific cleanup (e.g. logo removal).
pub fn run<G: Graft>(
    graft: &G,
    profile_dir: &Path,
    recovery_dir: &Path,
    include_backups: bool,
) -> Result<ResetReport, CliError> {
    let mut report = ResetReport::default();

    // 1. Database file + sidecars
    for suffix in DB_SIDECAR_SUFFIXES {
        let path = profile_dir.join(format!("mokumo.db{suffix}"));
        delete_file(&path, &mut report);
    }

    // 2. Backup files (opt-in)
    if include_backups {
        match std::fs::read_dir(profile_dir) {
            Ok(entries) => {
                for entry_result in entries {
                    let entry = match entry_result {
                        Ok(e) => e,
                        Err(e) => {
                            report.failed.push((profile_dir.to_path_buf(), e));
                            continue;
                        }
                    };
                    let name = entry.file_name();
                    if let Some(name_str) = name.to_str()
                        && name_str.starts_with("mokumo.db.backup-v")
                    {
                        delete_file(&entry.path(), &mut report);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                report.backup_dir_error = Some((profile_dir.to_path_buf(), e));
            }
        }
    }

    // 3. Recovery directory contents (only mokumo-recovery-*.html files)
    match std::fs::read_dir(recovery_dir) {
        Ok(entries) => {
            for entry_result in entries {
                let entry = match entry_result {
                    Ok(e) => e,
                    Err(e) => {
                        report.failed.push((recovery_dir.to_path_buf(), e));
                        continue;
                    }
                };
                let name = entry.file_name();
                if let Some(name_str) = name.to_str()
                    && name_str.starts_with("mokumo-recovery-")
                    && name_str.ends_with(".html")
                {
                    delete_file(&entry.path(), &mut report);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            report.recovery_dir_error = Some((recovery_dir.to_path_buf(), e));
        }
    }

    // 4. Domain-specific cleanup via lifecycle hook
    graft
        .on_post_reset_db(profile_dir, recovery_dir)
        .map_err(|e| CliError::Other(format!("domain cleanup failed: {e}")))?;

    Ok(report)
}

fn delete_file(path: &Path, report: &mut ResetReport) {
    match std::fs::remove_file(path) {
        Ok(()) => report.deleted.push(path.to_path_buf()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            report.not_found.push(path.to_path_buf());
        }
        Err(e) => report.failed.push((path.to_path_buf(), e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal Graft impl for testing — lifecycle hooks are default no-ops.
    struct TestGraft;
    impl Graft for TestGraft {
        type AppState = ();
        type DomainState = ();
        fn id() -> kikan::GraftId {
            kikan::GraftId::new("test")
        }
        fn migrations(&self) -> Vec<Box<dyn kikan::Migration>> {
            vec![]
        }
        async fn build_domain_state(
            &self,
            _ctx: &kikan::EngineContext,
        ) -> Result<(), kikan::EngineError> {
            Ok(())
        }
        fn compose_state(_p: kikan::PlatformState, _c: kikan::ControlPlaneState, _d: ()) -> () {}
        fn platform_state(_: &()) -> &kikan::PlatformState {
            unimplemented!()
        }
        fn control_plane_state(_: &()) -> &kikan::ControlPlaneState {
            unimplemented!()
        }
        fn data_plane_routes(_state: &()) -> axum::Router<()> {
            axum::Router::new()
        }
    }

    #[test]
    fn deletes_database_and_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path();

        // Create DB + sidecars
        std::fs::write(profile.join("mokumo.db"), b"main").unwrap();
        std::fs::write(profile.join("mokumo.db-wal"), b"wal").unwrap();
        std::fs::write(profile.join("mokumo.db-shm"), b"shm").unwrap();

        let recovery = tempfile::tempdir().unwrap();
        let report = run(&TestGraft, profile, recovery.path(), false).unwrap();

        assert_eq!(report.deleted.len(), 3);
        assert_eq!(report.not_found.len(), 1); // -journal doesn't exist
        assert!(report.failed.is_empty());
    }

    #[test]
    fn includes_backups_when_requested() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path();

        std::fs::write(profile.join("mokumo.db"), b"main").unwrap();
        std::fs::write(profile.join("mokumo.db.backup-v20260101"), b"backup").unwrap();

        let recovery = tempfile::tempdir().unwrap();
        let report = run(&TestGraft, profile, recovery.path(), true).unwrap();

        assert!(
            report.deleted.iter().any(|p| p
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .contains("backup-v")),
            "backup file should be deleted"
        );
    }

    #[test]
    fn cleans_recovery_files() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path();
        let recovery = tempfile::tempdir().unwrap();

        std::fs::write(
            recovery.path().join("mokumo-recovery-abc123.html"),
            b"recovery",
        )
        .unwrap();
        // Non-matching file should be left alone
        std::fs::write(recovery.path().join("other-file.txt"), b"keep").unwrap();

        let report = run(&TestGraft, profile, recovery.path(), false).unwrap();

        assert!(
            report
                .deleted
                .iter()
                .any(|p| p.to_str().unwrap().contains("mokumo-recovery-")),
            "recovery file should be deleted"
        );
        assert!(
            recovery.path().join("other-file.txt").exists(),
            "non-matching files should be untouched"
        );
    }

    #[test]
    fn handles_nonexistent_profile_dir() {
        let recovery = tempfile::tempdir().unwrap();
        let report = run(
            &TestGraft,
            Path::new("/nonexistent/profile"),
            recovery.path(),
            false,
        )
        .unwrap();

        // All DB+sidecar files should be not_found
        assert_eq!(report.not_found.len(), 4);
        assert!(report.failed.is_empty());
    }
}
