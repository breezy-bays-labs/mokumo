use std::path::Path;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mokumo_types::error::{ErrorBody, ErrorCode};

use crate::SharedState;

fn error_response(status: StatusCode, code: ErrorCode, message: &str) -> Response {
    (
        status,
        Json(ErrorBody {
            code,
            message: message.into(),
            details: None,
        }),
    )
        .into_response()
}

/// POST /api/demo/reset — reset the demo database to its original sidecar state.
///
/// Guards: demo mode only. Authentication is enforced by the
/// `require_auth_with_demo_auto_login` route layer — this handler is only
/// reachable by authenticated users.
pub async fn demo_reset(State(state): State<SharedState>) -> Response {
    use mokumo_core::setup::SetupMode;

    // Must be demo mode
    if state.setup_mode != Some(SetupMode::Demo) {
        return error_response(
            StatusCode::FORBIDDEN,
            ErrorCode::Forbidden,
            "Demo reset is only available in demo mode",
        );
    }

    // Force-copy fresh sidecar over the demo database.
    // The existing connection pool still holds the old file descriptor — this is fine
    // because the server is about to shut down and restart with the fresh copy.
    if let Err(e) = force_copy_sidecar(&state.data_dir) {
        tracing::error!("Demo reset: failed to copy sidecar: {e}");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::InternalError,
            "Failed to reset demo database",
        );
    }

    // Write a restart sentinel so the server loop knows to restart (not exit).
    // Must happen before constructing the response so the message reflects reality.
    let sentinel = state.data_dir.join(".restart");
    let message = match std::fs::write(&sentinel, b"reset") {
        Ok(()) => "Demo data reset successfully. Server will restart.".to_string(),
        Err(e) => {
            tracing::error!(
                "Demo reset: sentinel write failed ({e}). \
                 Server will shut down but may NOT restart automatically."
            );
            "Demo data reset, but automatic restart may fail. \
             Please restart the server manually if it does not come back online."
                .to_string()
        }
    };

    let response = (
        StatusCode::OK,
        Json(mokumo_types::setup::DemoResetResponse {
            success: true,
            message,
        }),
    )
        .into_response();

    let shutdown = state.shutdown.clone();
    tokio::spawn(async move {
        // Grace period for Axum to flush the response before the
        // CancellationToken tears down the server.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        shutdown.cancel();
    });

    response
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
/// Uses atomic rename: copies to a temp file in the same directory, then
/// renames over the destination. This avoids corrupting an in-use SQLite
/// file if any connections are still draining during graceful shutdown.
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
    // Existing file descriptors (from draining connections) continue reading
    // the old inode; new connections open the fresh copy.
    std::fs::rename(&tmp, &dest)?;

    // Remove WAL/SHM files after the rename — they belong to the old DB
    let _ = std::fs::remove_file(demo_dir.join("mokumo.db-wal"));
    let _ = std::fs::remove_file(demo_dir.join("mokumo.db-shm"));

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
