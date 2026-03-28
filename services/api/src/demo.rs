use std::path::Path;

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

    // Remove WAL/SHM files first to avoid stale journal issues
    let _ = std::fs::remove_file(demo_dir.join("mokumo.db-wal"));
    let _ = std::fs::remove_file(demo_dir.join("mokumo.db-shm"));

    std::fs::create_dir_all(&demo_dir)?;
    std::fs::copy(&src, &dest)?;
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
