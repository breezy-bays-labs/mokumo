//! Shared startup helpers — database preparation, session store init,
//! setup-token generation, and directory/port bootstrap.
//!
//! Lifted from `mokumo-api` in PR 4b (#512). These helpers call into
//! `mokumo_shop::db` functions (schema compatibility, pool init) which
//! would be an I4 violation if they lived in `kikan`, so `mokumo-shop`
//! is their correct home.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use kikan_types::SetupMode;
use sea_orm::DatabaseConnection;
use tower_sessions_sqlx_store::SqliteStore;

/// Error returned by [`setup_profile_db`] and [`prepare_database`].
///
/// Carries the human-readable error message and the path to the pre-migration
/// backup (if one was created before the failure). The backup path lets callers
/// surface the restore location to the shop owner in error dialogs and startup
/// events.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ProfileDbError {
    pub message: String,
    pub backup_path: Option<PathBuf>,
}

/// Create the required data directories: `data_dir`, `demo/`, `production/`,
/// and `logs/`.
pub fn ensure_data_dirs(data_dir: &Path) -> Result<(), std::io::Error> {
    for dir in [
        data_dir.to_path_buf(),
        data_dir.join(SetupMode::Demo.as_dir_name()),
        data_dir.join(SetupMode::Production.as_dir_name()),
        data_dir.join("logs"),
    ] {
        std::fs::create_dir_all(&dir).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("Failed to create directory {}: {}", dir.display(), e),
            )
        })?;
    }
    Ok(())
}

/// Read the `active_profile` file from the data directory.
///
/// Returns `SetupMode::Demo` if the file does not exist, is empty, or contains
/// an unrecognised value (first launch defaults to demo).
pub fn resolve_active_profile(data_dir: &Path) -> SetupMode {
    let profile_path = data_dir.join("active_profile");
    match std::fs::read_to_string(&profile_path) {
        Ok(contents) => contents.trim().parse().unwrap_or(SetupMode::Demo),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SetupMode::Demo,
        Err(e) => {
            tracing::error!(
                path = %profile_path.display(),
                "Failed to read active_profile file: {e}; defaulting to demo"
            );
            SetupMode::Demo
        }
    }
}

/// Migrate a flat data directory layout to the dual-profile structure.
///
/// Idempotent: safe to call on every startup.
///
/// Steps:
/// 1. If `production/mokumo.db` does NOT exist AND flat `mokumo.db` DOES exist:
///    copy flat -> production/mokumo.db
/// 2. If `active_profile` does NOT exist: write "production"
///    (existing users who had a flat layout are production users)
/// 3. If BOTH `production/mokumo.db` AND flat `mokumo.db` exist: remove flat
pub fn migrate_flat_layout(data_dir: &Path) -> Result<(), std::io::Error> {
    let flat_db = data_dir.join("mokumo.db");
    let production_db = data_dir
        .join(SetupMode::Production.as_dir_name())
        .join("mokumo.db");
    let profile_path = data_dir.join("active_profile");

    let flat_exists = flat_db.try_exists()?;
    let production_exists = production_db.try_exists()?;

    if !production_exists && flat_exists {
        std::fs::create_dir_all(data_dir.join(SetupMode::Production.as_dir_name()))?;
        // Best-effort WAL checkpoint before copying: ensures committed but
        // un-checkpointed transactions are included in the destination file.
        if let Ok(conn) = rusqlite::Connection::open(&flat_db)
            && let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        {
            tracing::warn!(
                "WAL checkpoint failed during flat DB migration (proceeding with copy): {e}"
            );
        }
        std::fs::copy(&flat_db, &production_db)?;
        tracing::info!("Migrated flat database to {}", production_db.display());
    }

    if !profile_path.try_exists()? && flat_exists {
        std::fs::write(&profile_path, SetupMode::Production.as_str())?;
        tracing::info!("Set active profile to 'production' (migrated from flat layout)");
    }

    if flat_exists {
        std::fs::remove_file(&flat_db)?;
        tracing::info!("Removed flat database after migration");
        let _ = std::fs::remove_file(data_dir.join("mokumo.db-wal"));
        let _ = std::fs::remove_file(data_dir.join("mokumo.db-shm"));
    }

    Ok(())
}

/// Shared startup sequence: create directories, migrate layout, copy sidecar,
/// resolve profile, run guard chain and initialize both databases.
///
/// Guard chain per profile (active first, then non-active):
///   1. `check_application_id` — reject non-Mokumo SQLite files
///   2. `pre_migration_backup` — WAL-safe backup before any migration runs
///   3. `ensure_auto_vacuum`
///   4. `check_schema_compatibility` — reject databases from newer Mokumo versions
///      (demo: silent recreate from sidecar; production: hard abort)
///   5. `initialize_database` — pool + PRAGMAs + `Migrator::up()`
///
/// Returns `(demo_db, production_db, profile)` on success.
pub async fn prepare_database(
    data_dir: &Path,
) -> Result<(DatabaseConnection, DatabaseConnection, SetupMode), ProfileDbError> {
    ensure_data_dirs(data_dir).map_err(|e| ProfileDbError {
        message: format!("Failed to create data directories: {e}"),
        backup_path: None,
    })?;
    migrate_flat_layout(data_dir).map_err(|e| ProfileDbError {
        message: format!("Failed to migrate flat layout: {e}"),
        backup_path: None,
    })?;

    if let Err(e) = kikan::platform::demo::copy_sidecar_if_needed(data_dir) {
        tracing::warn!(
            "Failed to copy demo sidecar: {e}; \
             demo will start with empty database (no pre-seeded data)"
        );
    }

    let profile = resolve_active_profile(data_dir);
    let other_profile = match profile {
        SetupMode::Demo => SetupMode::Production,
        SetupMode::Production => SetupMode::Demo,
    };

    let active_db_path = data_dir.join(profile.as_str()).join("mokumo.db");
    let other_db_path = data_dir.join(other_profile.as_str()).join("mokumo.db");

    let active_db =
        setup_profile_db(&active_db_path, profile == SetupMode::Production, data_dir).await?;
    tracing::info!(
        "Active database ({profile}) ready at {}",
        active_db_path.display()
    );

    let other_db = setup_profile_db(
        &other_db_path,
        other_profile == SetupMode::Production,
        data_dir,
    )
    .await?;
    tracing::info!(
        "Non-active database ({other_profile}) ready at {}",
        other_db_path.display()
    );

    let (demo_db, production_db) = match profile {
        SetupMode::Demo => (active_db, other_db),
        SetupMode::Production => (other_db, active_db),
    };

    Ok((demo_db, production_db, profile))
}

async fn run_auto_vacuum_guard(
    db_path: &Path,
    backup_path: Option<PathBuf>,
) -> Result<(), ProfileDbError> {
    let db_path_owned = db_path.to_path_buf();
    let display = db_path.display().to_string();
    tokio::task::spawn_blocking(move || kikan::db::ensure_auto_vacuum(&db_path_owned))
        .await
        .map_err(|e| ProfileDbError {
            message: format!("auto_vacuum guard panicked for {display}: {e}"),
            backup_path: backup_path.clone(),
        })?
        .map_err(|e| ProfileDbError {
            message: format!(
                "Failed to enable auto_vacuum on {display}: {e}. \
                 Check disk space (VACUUM requires ~2x database size).",
            ),
            backup_path,
        })
}

async fn setup_profile_db(
    db_path: &Path,
    is_production: bool,
    data_dir: &Path,
) -> Result<DatabaseConnection, ProfileDbError> {
    use kikan::db::DatabaseSetupError;

    let backup_taken = db_path.exists();
    let mut backup_path: Option<PathBuf> = None;

    if backup_taken {
        kikan::db::check_application_id(db_path).map_err(|e| match e {
            DatabaseSetupError::NotKikanDatabase { ref path } => ProfileDbError {
                message: format!(
                    "The database at {} is not a Mokumo database. \
                     Check your --data-dir setting.",
                    path.display()
                ),
                backup_path: None,
            },
            _ => ProfileDbError {
                message: format!("application_id check failed for {}: {e}", db_path.display()),
                backup_path: None,
            },
        })?;

        backup_path = kikan::backup::pre_migration_backup(db_path)
            .await
            .map_err(|e| ProfileDbError {
                message: format!(
                    "Pre-migration backup failed for {}: {e}. \
                     Refusing to run migrations without a backup. \
                     Check disk space and permissions.",
                    db_path.display()
                ),
                backup_path: None,
            })?;

        run_auto_vacuum_guard(db_path, backup_path.clone()).await?;

        match crate::db::check_schema_compatibility(db_path) {
            Ok(()) => {}
            Err(DatabaseSetupError::SchemaIncompatible {
                ref path,
                ref unknown_migrations,
            }) => {
                if is_production {
                    return Err(ProfileDbError {
                        message: format!(
                            "The production database at {} was created by a newer version of Mokumo. \
                             Please upgrade Mokumo to the latest version, or restore from a backup. \
                             Do not delete the database — your production data is there.",
                            path.display()
                        ),
                        backup_path: backup_path.clone(),
                    });
                }
                tracing::warn!(
                    ?unknown_migrations,
                    "Demo database has unknown migrations from newer Mokumo version; \
                     resetting to fresh demo data."
                );
                kikan::platform::demo::force_copy_sidecar(data_dir).map_err(|e| {
                    ProfileDbError {
                        message: format!("Failed to reset demo database: {e}"),
                        backup_path: backup_path.clone(),
                    }
                })?;
                if db_path.exists() {
                    kikan::db::check_application_id(db_path).map_err(|e| match e {
                        DatabaseSetupError::NotKikanDatabase { ref path } => ProfileDbError {
                            message: format!(
                                "The bundled demo database is not a valid Mokumo database: {}. \
                                 Please reinstall Mokumo.",
                                path.display()
                            ),
                            backup_path: None,
                        },
                        _ => ProfileDbError {
                            message: format!(
                                "application_id check failed for demo database after reset: {e}"
                            ),
                            backup_path: None,
                        },
                    })?;
                    let _sidecar_backup = kikan::backup::pre_migration_backup(db_path)
                        .await
                        .map_err(|e| ProfileDbError {
                            message: format!(
                                "Pre-migration backup failed for demo database after reset: {e}"
                            ),
                            backup_path: None,
                        })?;
                    run_auto_vacuum_guard(db_path, None).await?;
                    if let Err(e) = crate::db::check_schema_compatibility(db_path) {
                        return Err(ProfileDbError {
                            message: format!(
                                "Demo database failed compatibility check after reset: {e}"
                            ),
                            backup_path: None,
                        });
                    }
                }
                let url = format!("sqlite:{}?mode=rwc", db_path.display());
                return crate::db::initialize_database(&url)
                    .await
                    .map_err(|e| ProfileDbError {
                        message: format_db_setup_error(e, db_path, true),
                        backup_path: None,
                    });
            }
            Err(e) => {
                return Err(ProfileDbError {
                    message: format!(
                        "Schema compatibility check failed for {}: {e}",
                        db_path.display()
                    ),
                    backup_path: backup_path.clone(),
                });
            }
        }
    } else {
        run_auto_vacuum_guard(db_path, None).await?;
    }

    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    crate::db::initialize_database(&url)
        .await
        .map_err(|e| ProfileDbError {
            message: format_db_setup_error(e, db_path, backup_taken),
            backup_path: backup_path.clone(),
        })
}

fn format_db_setup_error(
    e: kikan::db::DatabaseSetupError,
    db_path: &Path,
    backup_taken: bool,
) -> String {
    use kikan::db::DatabaseSetupError;
    tracing::error!("Database setup error for {}: {:?}", db_path.display(), e);
    match e {
        DatabaseSetupError::Migration(_) => {
            let backup_note = if backup_taken {
                " Your data was backed up before the attempt."
            } else {
                ""
            };
            format!(
                "Mokumo could not apply a database migration to {}.{backup_note} \
                 Contact support if this persists.",
                db_path.display()
            )
        }
        DatabaseSetupError::SchemaIncompatible { ref path, .. } => format!(
            "The database at {} was created by a newer version of Mokumo. \
             Please upgrade Mokumo to the latest version, or restore from a backup.",
            path.display()
        ),
        DatabaseSetupError::NotKikanDatabase { ref path } => format!(
            "The database at {} is not a Mokumo database. \
             Check your --data-dir setting.",
            path.display()
        ),
        DatabaseSetupError::Pool(_) => format!(
            "Failed to open database connection pool at {}. \
             Check disk space and file permissions.",
            db_path.display()
        ),
        DatabaseSetupError::Query(_) => format!(
            "A database query failed during initialization of {}. \
             Your data was not modified.",
            db_path.display()
        ),
        DatabaseSetupError::Rusqlite(_) => format!(
            "A low-level database error occurred while checking {}. \
             Check disk space and file permissions.",
            db_path.display()
        ),
    }
}

/// Attempt to bind a TCP listener, trying ports from `port` through `port + 10`.
pub async fn try_bind(
    host: &str,
    port: u16,
) -> Result<(tokio::net::TcpListener, u16), std::io::Error> {
    let end_port = port.saturating_add(10);
    for p in port..=end_port {
        let addr = format!("{host}:{p}");
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                let actual_port = listener.local_addr()?.port();
                tracing::info!("Listening on {host}:{actual_port}");
                return Ok((listener, actual_port));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                tracing::debug!("Port {p} in use, trying next");
            }
            Err(e) => {
                return Err(std::io::Error::new(
                    e.kind(),
                    format!("Cannot bind to {host}:{p}: {e}"),
                ));
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        format!(
            "All ports {port}-{end_port} are occupied. \
             Use --port to specify a different port, or close conflicting applications."
        ),
    ))
}

/// Generate a random setup token (UUID v4).
pub fn generate_setup_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Init session store + setup state. Opens a separate SQLite pool for sessions
/// (kept independent of the active profile database), runs session store
/// migrations, and generates a setup token if the production database is not
/// yet set up.
pub async fn init_session_and_setup(
    production_db: &DatabaseConnection,
    session_db_path: &Path,
) -> Result<(SqliteStore, Arc<AtomicBool>, Option<String>), Box<dyn std::error::Error + Send + Sync>>
{
    let is_complete = crate::db::is_setup_complete(production_db).await?;
    let setup_completed = Arc::new(AtomicBool::new(is_complete));
    let setup_token = if is_complete {
        None
    } else {
        Some(generate_setup_token())
    };

    let session_url = format!("sqlite:{}?mode=rwc", session_db_path.display());
    let session_pool = kikan::db::open_raw_sqlite_pool(&session_url)
        .await
        .map_err(|e| {
            format!(
                "Failed to open session database at {}: {e}",
                session_db_path.display()
            )
        })?;
    let session_store = SqliteStore::new(session_pool);
    session_store
        .migrate()
        .await
        .map_err(|e| format!("Session store migration failed: {e}"))?;

    Ok((session_store, setup_completed, setup_token))
}

/// Resolve the `demo_install_ok` flag at startup.
///
/// Runs `validate_installation` against the demo DB when the active profile is
/// Demo; always returns `true` for Production (an empty production DB is valid
/// — setup is pending, not broken).
pub async fn resolve_demo_install_ok(
    demo_db: &DatabaseConnection,
    active_profile: SetupMode,
) -> Arc<AtomicBool> {
    let ok = if active_profile == SetupMode::Demo {
        let ok = kikan::db::validate_installation(demo_db).await;
        tracing::info!(
            demo_install_ok = ok,
            "demo installation validation complete"
        );
        ok
    } else {
        true
    };
    Arc::new(AtomicBool::new(ok))
}

/// Resolve the directory for password-reset recovery files.
///
/// Priority: `MOKUMO_RECOVERY_DIR` env var > user's Desktop (macOS/Windows) > cwd.
/// On Linux, Desktop may not be available (XDG Desktop is optional), so the
/// effective priority is env var > cwd.
pub fn resolve_recovery_dir() -> PathBuf {
    std::env::var("MOKUMO_RECOVERY_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(dirs::desktop_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Maximum seconds to wait for in-flight requests to drain before forcing shutdown.
pub const DRAIN_TIMEOUT_SECS: u64 = 10;

// ---------------------------------------------------------------------------
// Process-level lock — prevents concurrent server startup and CLI mutations
// (reset-db, restore) against the same data directory. The lock file lives
// at `<data_dir>/mokumo.lock` and is held via `fd_lock::RwLock`. Writers also
// record the listening port so conflict messages can report it.
// ---------------------------------------------------------------------------

/// Path to the process-level lock file within the data directory.
pub fn lock_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join("mokumo.lock")
}

/// Write port info to the lock file so conflict messages can report the port.
///
/// Writes `port=NNNN\n` at the start of the file, truncating any previous content.
pub fn write_lock_info(file: &std::fs::File, port: u16) -> std::io::Result<()> {
    use std::io::{Seek, Write};
    let mut f = file;
    f.seek(std::io::SeekFrom::Start(0))?;
    f.set_len(0)?;
    writeln!(f, "port={port}")
}

/// Read port info from a lock file. Returns `None` if the file can't be read or parsed.
pub fn read_lock_info(path: &Path) -> Option<u16> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::debug!("Could not read lock file {}: {e}", path.display());
            return None;
        }
    };
    for line in content.lines() {
        if let Some(val) = line.strip_prefix("port=") {
            if let Ok(port) = val.trim().parse() {
                return Some(port);
            }
            tracing::debug!("Lock file has unparseable port value: {val:?}");
            return None;
        }
    }
    None
}

/// Format a conflict message when another server is already running.
pub fn format_lock_conflict_message(port: Option<u16>) -> String {
    match port {
        Some(p) => format!(
            "Another Mokumo server is already running on port {p}.\n\
             Check your system tray, or open http://localhost:{p}"
        ),
        None => "Another Mokumo server appears to be running.\n\
                 Stop the other instance first."
            .to_string(),
    }
}

/// Format a conflict message when reset-db is blocked by a running server.
pub fn format_reset_db_conflict_message(port: Option<u16>) -> String {
    match port {
        Some(p) => format!(
            "Cannot reset database while the server is running on port {p}.\n\
             Stop the server first, then try again."
        ),
        None => "Cannot reset database while the server is running.\n\
                 Stop the server first, then try again."
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_lock_info_writes_port_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mokumo.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        write_lock_info(&file, 6565).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "port=6565\n");
    }

    #[test]
    fn write_lock_info_overwrites_previous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mokumo.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        write_lock_info(&file, 6565).unwrap();
        write_lock_info(&file, 6570).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "port=6570\n");
    }

    #[test]
    fn read_lock_info_parses_port() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mokumo.lock");
        std::fs::write(&path, "port=6567\n").unwrap();
        assert_eq!(read_lock_info(&path), Some(6567));
    }

    #[test]
    fn read_lock_info_returns_none_for_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mokumo.lock");
        std::fs::write(&path, "").unwrap();
        assert_eq!(read_lock_info(&path), None);
    }

    #[test]
    fn read_lock_info_returns_none_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-such-lock-file");
        assert_eq!(read_lock_info(&path), None);
    }

    #[test]
    fn read_lock_info_returns_none_for_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mokumo.lock");
        std::fs::write(&path, "not a port line\n").unwrap();
        assert_eq!(read_lock_info(&path), None);
    }

    #[test]
    fn format_lock_conflict_with_port() {
        let msg = format_lock_conflict_message(Some(6565));
        assert!(msg.contains("already running on port 6565"));
        assert!(msg.contains("system tray"));
        assert!(msg.contains("http://localhost:6565"));
    }

    #[test]
    fn format_lock_conflict_without_port() {
        let msg = format_lock_conflict_message(None);
        assert!(msg.contains("appears to be running"));
    }

    #[test]
    fn format_reset_db_conflict_with_port() {
        let msg = format_reset_db_conflict_message(Some(6565));
        assert!(msg.contains("Cannot reset database"));
        assert!(msg.contains("port 6565"));
        assert!(msg.contains("Stop the server first"));
    }

    #[test]
    fn format_reset_db_conflict_without_port() {
        let msg = format_reset_db_conflict_message(None);
        assert!(msg.contains("Cannot reset database"));
        assert!(msg.contains("Stop the server first"));
    }
}
