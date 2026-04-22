use std::path::Path;

use axum::Json;
use axum::extract::State;
use kikan_types::setup::DemoResetResponse;

use crate::AppError;
use crate::PlatformState;

/// POST /api/demo/reset — reset the demo database to its original sidecar state.
///
/// Guards: demo mode only. In normal operation, authentication is enforced by the
/// `require_auth_with_demo_auto_login` route layer. Exception: when `demo_install_ok`
/// is false, the middleware bypasses the auth chain for this path so the recovery
/// mechanism can be called even when `admin@demo.local` is missing from the database.
pub async fn demo_reset(
    State(state): State<PlatformState>,
) -> Result<Json<DemoResetResponse>, AppError> {
    // Must be demo mode — stringly bridge; this entire handler is hoisted
    // to `mokumo-shop` in the Session 2b follow-up commit.
    if state.active_profile.read().as_str() != "demo" {
        return Err(AppError::Forbidden(
            kikan_types::error::ErrorCode::Forbidden,
            "Demo reset is only available in demo mode".into(),
        ));
    }

    // Close the demo database connection pool before replacing the file.
    // This releases file handles that would block std::fs::rename on Windows.
    // Other in-flight requests will get errors, but the server is about to shut down.
    let demo_db = state
        .db_for("demo")
        .cloned()
        .expect("demo profile pool present in PlatformState");
    demo_db.get_sqlite_connection_pool().close().await;

    // Force-copy fresh sidecar over the demo database.
    if let Err(e) = force_copy_sidecar(&state.data_dir) {
        tracing::error!("Demo reset: failed to copy sidecar: {e}");
        return Err(AppError::InternalError(
            "Failed to reset demo database".into(),
        ));
    }

    // Re-validate installation against the freshly-copied database so the
    // install_ok flag reflects the new state immediately (important for tests
    // and for clients that poll health before the server restarts).
    let demo_db_path = state.data_dir.join("demo").join(state.db_filename);
    let demo_url = format!("sqlite:{}?mode=rwc", demo_db_path.display());
    match state.profile_db_initializer.initialize(&demo_url).await {
        Ok(fresh_db) => {
            let ok = crate::db::validate_installation(&fresh_db).await;
            state
                .demo_install_ok
                .store(ok, std::sync::atomic::Ordering::Release);
            if let Err(e) = fresh_db.close().await {
                tracing::debug!(
                    "Demo reset: error closing re-validation pool (non-fatal, server restarting): {e}"
                );
            }
        }
        Err(e) => {
            tracing::error!("Demo reset: failed to open/migrate fresh sidecar: {e}");
            state
                .demo_install_ok
                .store(false, std::sync::atomic::Ordering::Release);
            // The connection pool is already closed and the file has been replaced.
            // The server cannot serve demo-profile requests in this state. Schedule
            // a restart so the process recovers, then return an error to the caller.
            // If the sentinel write fails, do NOT schedule shutdown — stopping the
            // server without restarting it would leave the shop with no running server.
            let sentinel = state.data_dir.join(".restart");
            if let Err(se) = std::fs::write(&sentinel, b"reset") {
                tracing::error!("Demo reset: also failed to write restart sentinel: {se}");
                return Err(AppError::InternalError(
                    "Failed to schedule automatic restart after demo reset; please restart Mokumo manually.".into(),
                ));
            }
            let shutdown = state.shutdown.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                shutdown.cancel();
            });
            return Err(AppError::InternalError(
                "Failed to initialize demo database after reset; server will restart".into(),
            ));
        }
    }

    // Write the restart sentinel BEFORE responding — if this fails, the server
    // loop won't know to restart and the server would just die. Fail the request
    // rather than returning success and leaving the user with a dead server.
    let sentinel = state.data_dir.join(".restart");
    if let Err(e) = std::fs::write(&sentinel, b"reset") {
        tracing::error!("Demo reset: failed to write restart sentinel: {e}");
        return Err(AppError::InternalError(
            "Failed to prepare server restart after reset".into(),
        ));
    }

    let shutdown = state.shutdown.clone();
    tokio::spawn(async move {
        // Small delay to allow the response to be sent
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown.cancel();
    });

    Ok(Json(DemoResetResponse {
        success: true,
        message: "Demo data reset successfully. Server will restart.".into(),
    }))
}

/// Copy the demo sidecar database to `data_dir/demo/mokumo.db` if it doesn't already exist.
///
/// Sidecar lookup order:
/// 1. `MOKUMO_DEMO_SIDECAR` env var
/// 2. `demo.db` next to the current executable
///
/// Returns `Ok(true)` if a copy was made, `Ok(false)` if already present or sidecar not found.
pub fn copy_sidecar_if_needed(data_dir: &Path) -> Result<bool, std::io::Error> {
    let demo_dir = data_dir.join("demo");
    let dest = demo_dir.join("mokumo.db");
    if dest.try_exists()? {
        tracing::debug!("Demo database already exists at {}", dest.display());
        return Ok(false);
    }

    if let Some(src) = find_sidecar() {
        std::fs::create_dir_all(&demo_dir)?;
        std::fs::copy(&src, &dest)?;
        tracing::info!(
            "Copied demo sidecar from {} to {}",
            src.display(),
            dest.display()
        );
        Ok(true)
    } else {
        tracing::warn!("No demo.db sidecar found — starting without demo data");
        Ok(false)
    }
}

/// Force-copy the demo sidecar database to `data_dir/demo/mokumo.db`,
/// replacing any existing file. Used by the reset endpoint.
///
/// Strategy: copies to a temp file, then atomic-renames over the destination.
/// On Windows (where rename fails with open handles), falls back to
/// remove + rename. Callers should close the connection pool first.
///
/// Returns an error if no sidecar can be found.
pub fn force_copy_sidecar(data_dir: &Path) -> Result<(), std::io::Error> {
    let demo_dir = data_dir.join("demo");
    let dest = demo_dir.join("mokumo.db");
    let src = find_sidecar().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No demo.db sidecar available for reset",
        )
    })?;

    std::fs::create_dir_all(&demo_dir)?;

    // Copy to a temp file in the same directory (same filesystem = atomic rename)
    let tmp = demo_dir.join("mokumo.db.tmp");
    std::fs::copy(&src, &tmp)?;

    // Atomic rename replaces the destination without truncating the live file.
    // On Unix, existing file descriptors continue reading the old inode.
    // On Windows, rename may fail if any handle is still open — fall back to
    // remove + rename which works once the pool is closed by the caller.
    if let Err(rename_err) = std::fs::rename(&tmp, &dest) {
        tracing::warn!("Atomic rename failed ({rename_err}), trying remove + rename fallback");
        // Remove the destination first (Windows requires this), then retry rename.
        if let Err(remove_err) = std::fs::remove_file(&dest)
            && remove_err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("Failed to remove old database: {remove_err}");
        }
        std::fs::rename(&tmp, &dest).map_err(|e| {
            // Clean up the temp file on failure
            let _ = std::fs::remove_file(&tmp);
            std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to replace demo database (rename failed after fallback): {e}. \
                     Ensure no other processes have the database open."
                ),
            )
        })?;
    }

    // Remove WAL/SHM files — they belong to the old DB
    for suffix in &["mokumo.db-wal", "mokumo.db-shm"] {
        let path = demo_dir.join(suffix);
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("Failed to remove {}: {e}", path.display());
        }
    }

    tracing::info!(
        "Force-copied demo sidecar from {} to {}",
        src.display(),
        dest.display()
    );
    Ok(())
}

/// Locate the demo.db sidecar file.
///
/// Priority: MOKUMO_DEMO_SIDECAR env var > co-located demo.db next to binary.
fn find_sidecar() -> Option<std::path::PathBuf> {
    // 1. Env var
    if let Ok(path) = std::env::var("MOKUMO_DEMO_SIDECAR") {
        let p = std::path::PathBuf::from(&path);
        if p.try_exists().unwrap_or(false) {
            return Some(p);
        }
        tracing::debug!(
            "MOKUMO_DEMO_SIDECAR={} does not exist, trying co-located",
            path
        );
    }

    // 2. Co-located with binary
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let co_located = dir.join("demo.db");
        if co_located.try_exists().unwrap_or(false) {
            return Some(co_located);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    fn copy_sidecar_no_op_when_dest_exists() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path();
        std::fs::create_dir_all(data_dir.join("demo")).unwrap();
        std::fs::write(data_dir.join("demo").join("mokumo.db"), b"existing").unwrap();

        let copied = copy_sidecar_if_needed(data_dir).unwrap();
        assert!(!copied);
        // Content unchanged
        let content = std::fs::read(data_dir.join("demo").join("mokumo.db")).unwrap();
        assert_eq!(content, b"existing");
    }

    #[test]
    #[serial]
    fn copy_sidecar_uses_env_var() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        let sidecar_path = tmp.path().join("test_sidecar.db");
        std::fs::write(&sidecar_path, b"sidecar-data").unwrap();

        // Temporarily set the env var
        let _guard = EnvVarGuard::set("MOKUMO_DEMO_SIDECAR", sidecar_path.to_str().unwrap());

        let copied = copy_sidecar_if_needed(&data_dir).unwrap();
        assert!(copied);

        let content = std::fs::read(data_dir.join("demo").join("mokumo.db")).unwrap();
        assert_eq!(content, b"sidecar-data");
    }

    #[test]
    #[serial]
    fn copy_sidecar_returns_false_when_no_sidecar() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Ensure env var is NOT set
        let _guard = EnvVarGuard::remove("MOKUMO_DEMO_SIDECAR");

        let copied = copy_sidecar_if_needed(&data_dir).unwrap();
        assert!(!copied);
        assert!(!data_dir.join("demo").join("mokumo.db").exists());
    }

    #[test]
    #[serial]
    fn force_copy_replaces_existing() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(data_dir.join("demo")).unwrap();
        std::fs::write(data_dir.join("demo").join("mokumo.db"), b"old-data").unwrap();

        let sidecar_path = tmp.path().join("test_sidecar.db");
        std::fs::write(&sidecar_path, b"fresh-data").unwrap();

        let _guard = EnvVarGuard::set("MOKUMO_DEMO_SIDECAR", sidecar_path.to_str().unwrap());

        force_copy_sidecar(&data_dir).unwrap();

        let content = std::fs::read(data_dir.join("demo").join("mokumo.db")).unwrap();
        assert_eq!(content, b"fresh-data");
    }

    #[test]
    #[serial]
    fn force_copy_fails_when_no_sidecar() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        let _guard = EnvVarGuard::remove("MOKUMO_DEMO_SIDECAR");

        let result = force_copy_sidecar(&data_dir);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    #[serial]
    fn force_copy_cleans_wal_shm() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(data_dir.join("demo")).unwrap();
        std::fs::write(data_dir.join("demo").join("mokumo.db"), b"old").unwrap();
        std::fs::write(data_dir.join("demo").join("mokumo.db-wal"), b"wal").unwrap();
        std::fs::write(data_dir.join("demo").join("mokumo.db-shm"), b"shm").unwrap();

        let sidecar_path = tmp.path().join("test_sidecar.db");
        std::fs::write(&sidecar_path, b"fresh").unwrap();

        let _guard = EnvVarGuard::set("MOKUMO_DEMO_SIDECAR", sidecar_path.to_str().unwrap());

        force_copy_sidecar(&data_dir).unwrap();

        assert!(!data_dir.join("demo").join("mokumo.db-wal").exists());
        assert!(!data_dir.join("demo").join("mokumo.db-shm").exists());
    }

    /// RAII guard for temporarily setting/unsetting env vars in tests.
    struct EnvVarGuard {
        key: String,
        original: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key: key.into(),
                original,
            }
        }

        fn remove(key: &str) -> Self {
            let original = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
            Self {
                key: key.into(),
                original,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(val) => unsafe { std::env::set_var(&self.key, val) },
                None => unsafe { std::env::remove_var(&self.key) },
            }
        }
    }
}
