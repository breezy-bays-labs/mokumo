//! CLI restore command — restore database from a backup file.
//!
//! Dispatches `Graft::on_pre_restore` for domain validation, calls
//! `kikan::backup::restore_from_backup` for the platform-generic restore,
//! then dispatches `Graft::on_post_restore` for domain-specific artifacts
//! (e.g. logo files).

use std::path::Path;

use kikan::Graft;
use kikan::backup::RestoreResult;

use crate::CliError;

/// SQLite sidecar suffixes — forwarded to `restore_from_backup`.
const DB_SIDECAR_SUFFIXES: &[&str] = &["", "-wal", "-shm", "-journal"];

/// Restore the database from a backup file.
///
/// 1. Dispatches `graft.on_pre_restore()` for domain validation
/// 2. Calls `kikan::backup::restore_from_backup` (integrity check, safety backup, overwrite)
/// 3. Dispatches `graft.on_post_restore()` for domain artifact restoration (e.g. logos)
pub fn run<G: Graft>(
    graft: &G,
    db_path: &Path,
    backup_path: &Path,
) -> Result<RestoreResult, CliError> {
    graft
        .on_pre_restore(db_path, backup_path)
        .map_err(|e| CliError::Other(format!("pre-restore hook failed: {e}")))?;

    let result = kikan::backup::restore_from_backup(db_path, backup_path, DB_SIDECAR_SUFFIXES)
        .map_err(|e| CliError::Other(format!("{e}")))?;

    graft
        .on_post_restore(db_path, backup_path)
        .map_err(|e| CliError::Other(format!("post-restore hook failed: {e}")))?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        fn compose_state(_c: kikan::ControlPlaneState, _d: ()) {}
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
    fn returns_error_for_missing_backup() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("mokumo.db");
        let backup_path = dir.path().join("nonexistent.db");

        let err = run(&TestGraft, &db_path, &backup_path).unwrap_err();
        assert!(
            err.to_string().contains("does not exist")
                || err.to_string().contains("not found")
                || err.to_string().contains("No such file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn restores_valid_backup() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("mokumo.db");
        let backup_path = dir.path().join("backup.db");

        // Create a valid SQLite backup file
        let conn = rusqlite::Connection::open(&backup_path).unwrap();
        conn.execute_batch("CREATE TABLE test_table (id INTEGER PRIMARY KEY)")
            .unwrap();
        drop(conn);

        let result = run(&TestGraft, &db_path, &backup_path).unwrap();
        assert_eq!(result.restored_from, backup_path);
        assert!(db_path.exists(), "database file should exist after restore");
    }
}
