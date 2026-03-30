use std::path::PathBuf;

use clap::Parser;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use mokumo_api::{
    DB_SIDECAR_SUFFIXES, ServerConfig, build_app_with_shutdown, cli_reset_db, cli_reset_password,
    discovery, ensure_data_dirs, lock_file_path, prepare_database, resolve_active_profile,
    try_bind,
};

#[derive(Parser)]
#[command(name = "mokumo", about = "Mokumo Print — production management server")]
struct Cli {
    /// Port to listen on
    #[arg(short, long, default_value = "6565")]
    port: u16,

    /// Address to bind to (defaults to all interfaces for LAN access; use 127.0.0.1 for local-only)
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// Directory for application data (database, uploads)
    #[arg(long)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Reset a user's password directly (no running server required)
    ResetPassword {
        /// Email address of the user to reset
        #[arg(long)]
        email: String,
    },
    /// Delete the database and start fresh (dev/testing)
    ResetDb {
        /// Skip the confirmation prompt
        #[arg(long)]
        force: bool,
        /// Also delete pre-migration backup files
        #[arg(long)]
        include_backups: bool,
        /// Reset the production profile instead of the default demo profile.
        /// Requires typing "reset production" to confirm (irreversible).
        #[arg(long)]
        production: bool,
    },
}

/// Resolve the default data directory using platform conventions.
///
/// Falls back to `./data` if the platform directory cannot be determined.
fn resolve_default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "breezybayslabs", "mokumo")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| {
            let fallback = PathBuf::from("./data");
            // tracing may not be initialized yet, so use eprintln
            eprintln!(
                "WARNING: Could not determine platform data directory, using {:?}. \
                 Set --data-dir explicitly to control where data is stored.",
                fallback
            );
            fallback
        })
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let data_dir = cli.data_dir.unwrap_or_else(resolve_default_data_dir);

    // Handle subcommands before server startup
    match cli.command {
        Some(Commands::ResetPassword { email }) => {
            let profile = resolve_active_profile(&data_dir);
            let db_path = data_dir.join(profile.as_str()).join("mokumo.db");
            let password = rpassword::prompt_password("New password: ").unwrap_or_else(|e| {
                eprintln!("Failed to read password: {e}");
                std::process::exit(1);
            });
            if password.len() < 8 {
                eprintln!("Password must be at least 8 characters");
                std::process::exit(1);
            }
            let confirm = rpassword::prompt_password("Confirm password: ").unwrap_or_else(|e| {
                eprintln!("Failed to read password: {e}");
                std::process::exit(1);
            });
            if password != confirm {
                eprintln!("Passwords do not match");
                std::process::exit(1);
            }
            match cli_reset_password(&db_path, &email, &password) {
                Ok(()) => {
                    println!("Password reset successfully for {email}");
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some(Commands::ResetDb {
            force,
            include_backups,
            production,
        }) => {
            // Determine which profile to target.
            // Default: demo (safe). Production requires explicit --production flag.
            let profile_dir = if production {
                data_dir.join("production")
            } else {
                data_dir.join("demo")
            };
            let db_path = profile_dir.join("mokumo.db");

            // Ensure data directories exist so the lock file can be created if needed.
            if let Err(e) = ensure_data_dirs(&data_dir) {
                eprintln!("Cannot create data directory {}: {e}", data_dir.display());
                std::process::exit(1);
            }

            // Early exit when neither profile database exists (idempotent, exit 0).
            // Use explicit match on each path so I/O errors surface rather than silently
            // becoming "not found" via unwrap_or(false).
            let demo_db = data_dir.join("demo").join("mokumo.db");
            let production_db = data_dir.join("production").join("mokumo.db");
            let demo_exists = match demo_db.try_exists() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "Cannot access demo database path {}: {e}",
                        demo_db.display()
                    );
                    std::process::exit(1);
                }
            };
            let production_exists_check = match production_db.try_exists() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "Cannot access production database path {}: {e}",
                        production_db.display()
                    );
                    std::process::exit(1);
                }
            };
            let any_db_exists = demo_exists || production_exists_check;

            match db_path.try_exists() {
                Ok(false) if !any_db_exists => {
                    println!("No database found at {}.", data_dir.display());
                    return;
                }
                Ok(false) => {
                    // The other profile has a DB but not the targeted one
                    let profile_name = if production { "production" } else { "demo" };
                    println!("No database found for the {profile_name} profile.");
                    return;
                }
                Err(e) => {
                    eprintln!("Cannot access data directory {}: {e}", data_dir.display());
                    std::process::exit(1);
                }
                Ok(true) => {} // proceed
            }

            // Acquire process-level flock — definitively detects a running server
            // (including idle connections that BEGIN EXCLUSIVE would miss).
            // Held through preview, confirmation, AND deletion.
            let lock_path = lock_file_path(&data_dir);
            let mut flock = match std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&lock_path)
            {
                Ok(f) => fd_lock::RwLock::new(f),
                Err(e) => {
                    eprintln!("Cannot open lock file {}: {e}", lock_path.display());
                    std::process::exit(1);
                }
            };
            let _lock_guard = match flock.try_write() {
                Ok(guard) => guard,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    eprintln!(
                        "The database appears to be in use by a running server.\n\
                         Stop the server first, then try again."
                    );
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Cannot acquire process lock: {e}");
                    std::process::exit(1);
                }
            };

            // File inventory preview
            let mut preview_files: Vec<PathBuf> = Vec::new();
            for suffix in DB_SIDECAR_SUFFIXES {
                let path = profile_dir.join(format!("mokumo.db{suffix}"));
                if path.try_exists().unwrap_or(false) {
                    preview_files.push(path);
                }
            }
            if include_backups {
                match std::fs::read_dir(&profile_dir) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            if let Some(name) = entry.file_name().to_str()
                                && name.starts_with("mokumo.db.backup-v")
                            {
                                preview_files.push(entry.path());
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: cannot scan {} for backups: {e}",
                            profile_dir.display()
                        );
                    }
                }
            }

            println!("The following files will be deleted:\n");
            for f in &preview_files {
                println!("  {}", f.display());
            }

            let recovery_dir = mokumo_api::resolve_recovery_dir();
            match std::fs::read_dir(&recovery_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str()
                            && name.starts_with("mokumo-recovery-")
                            && name.ends_with(".html")
                        {
                            println!("  {}", entry.path().display());
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    eprintln!(
                        "Warning: cannot scan recovery directory {}: {e}",
                        recovery_dir.display()
                    );
                }
            }
            println!();

            // Confirmation gate.
            // --production requires an additional explicit confirmation step
            // because wiping production data is irreversible.
            if !force {
                use std::io::Write;
                if production {
                    eprintln!(
                        "WARNING: You are about to permanently delete the PRODUCTION database.\n\
                         This cannot be undone. All production data will be lost.\n"
                    );
                    print!("Type \"reset production\" to confirm: ");
                    if let Err(e) = std::io::stdout().flush() {
                        eprintln!("Cannot write to terminal: {e}");
                        std::process::exit(1);
                    }
                    let mut input = String::new();
                    if std::io::stdin().read_line(&mut input).is_err()
                        || input.trim() != "reset production"
                    {
                        println!("Cancelled.");
                        return;
                    }
                } else {
                    print!("Type \"reset\" to confirm: ");
                    if let Err(e) = std::io::stdout().flush() {
                        eprintln!("Cannot write to terminal: {e}");
                        std::process::exit(1);
                    }
                    let mut input = String::new();
                    if std::io::stdin().read_line(&mut input).is_err() || input.trim() != "reset" {
                        println!("Cancelled.");
                        return;
                    }
                }
            }

            // Execute the reset while flock is held. The flock is on a
            // separate sentinel file (not the db), so it does not interfere
            // with file deletion on any platform.
            let report = match cli_reset_db(&profile_dir, &recovery_dir, include_backups) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Reset failed: {e}");
                    std::process::exit(1);
                }
            };

            if let Some((dir, err)) = &report.recovery_dir_error {
                eprintln!(
                    "Warning: could not scan recovery directory {}: {err}\n\
                     Recovery files were not cleaned up. \
                     You may need to remove them manually.",
                    dir.display()
                );
            }
            if let Some((dir, err)) = &report.backup_dir_error {
                eprintln!(
                    "Warning: could not scan {} for backups: {err}\n\
                     Backup files may not have been deleted.",
                    dir.display()
                );
            }

            if report.failed.is_empty() {
                println!(
                    "\nDatabase reset complete ({} files deleted). \
                     Start the server to begin fresh setup:\n\n  mokumo",
                    report.deleted.len()
                );
            } else {
                eprintln!("\nSome files could not be deleted:");
                for (path, err) in &report.failed {
                    eprintln!("  {}: {err}", path.display());
                }
                if !report.deleted.is_empty() {
                    eprintln!(
                        "\n{} files were deleted successfully.",
                        report.deleted.len()
                    );
                }
                std::process::exit(1);
            }
            return;
        }
        None => {} // No subcommand — fall through to server startup
    }

    // Initialize tracing (server mode only)
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|e| {
        if std::env::var_os("RUST_LOG").is_some() {
            eprintln!("WARNING: Invalid RUST_LOG value, falling back to 'info': {e}");
        }
        "info".into()
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let recovery_dir = mokumo_api::resolve_recovery_dir();
    let config = ServerConfig {
        port: cli.port,
        host: cli.host,
        data_dir,
        recovery_dir,
    };

    // Create data directories (including demo/ and production/)
    if let Err(e) = ensure_data_dirs(&config.data_dir) {
        eprintln!(
            "Cannot create data directory {}: {e}",
            config.data_dir.display()
        );
        tracing::error!(
            "Cannot create data directory {}: {e}",
            config.data_dir.display()
        );
        std::process::exit(1);
    }

    // Acquire process-level flock — prevents concurrent server instances and
    // signals to `reset-db` that this process is running. Held for the entire
    // server lifetime; the OS releases it automatically on exit or crash.
    let lock_path = lock_file_path(&config.data_dir);
    let lock_file = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Cannot open lock file {}: {e}", lock_path.display());
            tracing::error!("Cannot open lock file {}: {e}", lock_path.display());
            std::process::exit(1);
        }
    };
    let mut flock = fd_lock::RwLock::new(lock_file);
    let _server_lock = match flock.try_write() {
        Ok(guard) => guard,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            eprintln!(
                "Another Mokumo server appears to be running (lock held on {}).\n\
                 Stop the other instance first.",
                lock_path.display()
            );
            tracing::error!("Process lock already held — another server is running");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Cannot acquire process lock: {e}");
            tracing::error!("Cannot acquire process lock: {e}");
            std::process::exit(1);
        }
    };

    // Shared startup: dirs, layout migration, sidecar copy, backup, DB init, non-active migration
    let (_initial_db, profile) = match prepare_database(&config.data_dir).await {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Startup failed: {e}");
            tracing::error!("Startup failed: {e}");
            std::process::exit(1);
        }
    };
    let db_path = config.data_dir.join(profile.as_str()).join("mokumo.db");

    // Master shutdown token — Ctrl+C cancels this once. Each loop iteration creates
    // a child token so individual restarts don't tear down the master signal.
    let master_shutdown = CancellationToken::new();

    // Server loop: runs once normally, restarts on demo reset.
    // Each iteration gets a fresh shutdown token, DB pool, and app state.
    {
        let token = master_shutdown.clone();
        tokio::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    tracing::info!("Shutdown signal received, draining connections...");
                    token.cancel();
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to listen for ctrl+c: {e}. \
                         Server will continue running but graceful shutdown via Ctrl+C is unavailable."
                    );
                }
            }
        });
    }

    let mut bound_port: Option<u16> = None;

    loop {
        // Re-open the database on each iteration (demo reset may have replaced the file)
        let db_pool =
            match mokumo_db::initialize_database(&format!("sqlite:{}?mode=rwc", db_path.display()))
                .await
            {
                Ok(pool) => {
                    tracing::info!("Database ready at {}", db_path.display());
                    pool
                }
                Err(e) => {
                    tracing::error!("Failed to initialize database: {e}");
                    std::process::exit(1);
                }
            };

        let shutdown_token = master_shutdown.child_token();
        let mdns_status = discovery::MdnsStatus::shared();

        let (app, _setup_token) = build_app_with_shutdown(
            &config,
            db_pool,
            shutdown_token.clone(),
            mdns_status.clone(),
        )
        .await;

        // Bind to port (reuse the same port on restart)
        let port = bound_port.unwrap_or(config.port);
        let (listener, actual_port) = match try_bind(&config.host, port).await {
            Ok(result) => result,
            Err(e) => {
                eprintln!("{e}");
                tracing::error!("{e}");
                std::process::exit(1);
            }
        };
        bound_port = Some(actual_port);

        if actual_port != config.port {
            tracing::warn!(
                "Requested port {} was unavailable, using port {} instead",
                config.port,
                actual_port
            );
        }

        {
            let mut s = mdns_status.write().expect("MdnsStatus lock poisoned");
            s.port = actual_port;
            s.bind_host = config.host.clone();
        }

        let mdns_handle = discovery::register_mdns(
            &config.host,
            actual_port,
            &mdns_status,
            &discovery::RealDiscovery,
        );

        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown_token.cancelled().await;
            })
            .await
        {
            tracing::error!("Server error: {e}");
            std::process::exit(1);
        }

        // Deregister mDNS after server stops (both Ctrl+C and restart paths)
        if let Some(handle) = mdns_handle {
            discovery::deregister_mdns(handle, &mdns_status);
        }

        // Check if restart was requested (e.g., demo reset)
        let restart_sentinel = config.data_dir.join(".restart");
        if restart_sentinel.exists() {
            let _ = std::fs::remove_file(&restart_sentinel);
            tracing::info!("Restart requested — reinitializing server with fresh database");
            continue;
        }

        tracing::info!("Server shut down cleanly");
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mokumo_api::migrate_flat_layout;
    use mokumo_core::setup::SetupMode;
    use tempfile::tempdir;

    #[test]
    fn resolve_active_profile_missing_file_defaults_to_demo() {
        let tmp = tempdir().unwrap();
        assert_eq!(resolve_active_profile(tmp.path()), SetupMode::Demo);
    }

    #[test]
    fn resolve_active_profile_reads_content() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("active_profile"), "production").unwrap();
        assert_eq!(resolve_active_profile(tmp.path()), SetupMode::Production);
    }

    #[test]
    fn resolve_active_profile_trims_whitespace() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("active_profile"), "  demo\n").unwrap();
        assert_eq!(resolve_active_profile(tmp.path()), SetupMode::Demo);
    }

    #[test]
    fn resolve_active_profile_empty_file_defaults_to_demo() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("active_profile"), "").unwrap();
        assert_eq!(resolve_active_profile(tmp.path()), SetupMode::Demo);
    }

    #[test]
    fn resolve_active_profile_invalid_value_defaults_to_demo() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("active_profile"), "../../escape").unwrap();
        assert_eq!(resolve_active_profile(tmp.path()), SetupMode::Demo);
    }

    #[test]
    fn migrate_flat_layout_fresh_directory_is_noop() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("production")).unwrap();
        migrate_flat_layout(tmp.path()).unwrap();
        // No flat DB, no production DB — nothing to do
        assert!(!tmp.path().join("production").join("mokumo.db").exists());
        assert!(!tmp.path().join("active_profile").exists());
    }

    #[test]
    fn migrate_flat_layout_copies_flat_to_production() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("production")).unwrap();
        std::fs::write(tmp.path().join("mokumo.db"), b"test-data").unwrap();

        migrate_flat_layout(tmp.path()).unwrap();

        // Production DB should exist with same content
        let prod_content = std::fs::read(tmp.path().join("production").join("mokumo.db")).unwrap();
        assert_eq!(prod_content, b"test-data");
        // Flat DB should be removed
        assert!(!tmp.path().join("mokumo.db").exists());
        // active_profile should be "production"
        let profile = std::fs::read_to_string(tmp.path().join("active_profile")).unwrap();
        assert_eq!(profile, "production");
    }

    #[test]
    fn migrate_flat_layout_already_migrated_is_noop() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("production")).unwrap();
        std::fs::write(
            tmp.path().join("production").join("mokumo.db"),
            b"prod-data",
        )
        .unwrap();
        std::fs::write(tmp.path().join("active_profile"), "production").unwrap();

        migrate_flat_layout(tmp.path()).unwrap();

        // Production DB unchanged
        let content = std::fs::read(tmp.path().join("production").join("mokumo.db")).unwrap();
        assert_eq!(content, b"prod-data");
    }

    #[test]
    fn migrate_flat_layout_crash_recovery_removes_flat() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("production")).unwrap();
        // Simulate crash: both flat and production exist
        std::fs::write(tmp.path().join("mokumo.db"), b"flat-data").unwrap();
        std::fs::write(
            tmp.path().join("production").join("mokumo.db"),
            b"prod-data",
        )
        .unwrap();

        migrate_flat_layout(tmp.path()).unwrap();

        // Flat should be removed, production preserved
        assert!(!tmp.path().join("mokumo.db").exists());
        let content = std::fs::read(tmp.path().join("production").join("mokumo.db")).unwrap();
        assert_eq!(content, b"prod-data");
    }
}
