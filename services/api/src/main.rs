use std::path::PathBuf;

use clap::Parser;
use tokio_util::sync::CancellationToken;

use mokumo_api::{
    DB_SIDECAR_SUFFIXES, ServerConfig, build_app_with_shutdown, cli_backup, cli_migrate_status,
    cli_reset_db, cli_reset_password, cli_restore, discovery, ensure_data_dirs,
    format_lock_conflict_message, format_reset_db_conflict_message, lock_file_path,
    logging::{console_level_from_flags, init_tracing},
    prepare_database, read_lock_info, resolve_active_profile, try_bind, write_lock_info,
};
use mokumo_core::setup::SetupMode;

#[derive(Debug, Parser)]
#[command(
    name = "mokumo",
    about = "Mokumo Print — production management server",
    version,
    long_version = long_version()
)]
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

    /// WebSocket heartbeat interval in milliseconds (debug builds only)
    #[cfg(debug_assertions)]
    #[arg(long, hide = true)]
    ws_ping_ms: Option<u64>,

    /// Increase log verbosity: -v = debug, -vv = trace. Overrides RUST_LOG.
    #[arg(short, long, action = clap::ArgAction::Count, conflicts_with = "quiet", global = true)]
    verbose: u8,

    /// Suppress all log output except errors. Overrides RUST_LOG.
    #[arg(short, long, conflicts_with = "verbose", global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, clap::Subcommand)]
enum Commands {
    /// Print version and build information
    Version,
    /// Reset a user's password directly (no running server required)
    ResetPassword {
        /// Email address of the user to reset
        #[arg(long)]
        email: String,
    },
    /// Run database health checks and optional maintenance
    Doctor {
        /// Attempt automatic repairs (incremental vacuum, enable auto_vacuum)
        #[arg(long)]
        fix: bool,
        /// Reset the production profile instead of the default demo profile.
        #[arg(long)]
        production: bool,
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
    /// Create a manual backup of the database
    Backup {
        /// Write the backup to this path instead of the default timestamped name
        #[arg(long)]
        output: Option<PathBuf>,
        /// Back up the production profile instead of the default demo profile
        #[arg(long)]
        production: bool,
    },
    /// Restore the database from a backup file
    Restore {
        /// Path to the backup file to restore from
        path: PathBuf,
        /// Restore to the production profile instead of the default demo profile
        #[arg(long)]
        production: bool,
    },
    /// Database migration commands
    Migrate {
        #[command(subcommand)]
        action: MigrateCommands,
    },
}

#[derive(Debug, clap::Subcommand)]
enum MigrateCommands {
    /// Show current schema version, applied migrations, and any pending migrations
    Status {
        /// Check the production profile instead of the default demo profile
        #[arg(long)]
        production: bool,
    },
}

/// Build extended version string from compile-time environment variables.
///
/// Returns a static string with version, git hash, build date, platform, and
/// Rust version — all injected by vergen-gitcl at build time.
fn long_version() -> &'static str {
    concat!(
        env!("CARGO_PKG_VERSION"),
        "\n",
        "git hash:   ",
        env!("VERGEN_GIT_SHA"),
        "\n",
        "built:      ",
        env!("VERGEN_BUILD_TIMESTAMP"),
        "\n",
        "target:     ",
        env!("VERGEN_CARGO_TARGET_TRIPLE"),
        "\n",
        "rustc:      ",
        env!("VERGEN_RUSTC_SEMVER"),
    )
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

    /// Resolve the profile directory for a --production flag.
    fn profile_dir(data_dir: &std::path::Path, production: bool) -> PathBuf {
        let mode = if production {
            SetupMode::Production
        } else {
            SetupMode::Demo
        };
        data_dir.join(mode.as_dir_name())
    }

    // Handle subcommands before server startup
    match cli.command {
        Some(Commands::Version) => {
            println!("mokumo {}", long_version());
            return;
        }
        Some(Commands::Doctor { fix, production }) => {
            let profile_dir = profile_dir(&data_dir, production);
            let db_path = profile_dir.join("mokumo.db");

            if !db_path.exists() {
                let mode = if production { "production" } else { "demo" };
                eprintln!(
                    "No database found for the {mode} profile at {}",
                    db_path.display()
                );
                std::process::exit(1);
            }

            // Acquire process lock when --fix is requested to prevent concurrent
            // access with a running server (VACUUM + incremental_vacuum are unsafe
            // with concurrent writers). The lock is held for the entire Doctor arm.
            let mut _flock_storage;
            let _lock_guard;
            if fix {
                let lock_path = lock_file_path(&data_dir);
                _flock_storage = match std::fs::OpenOptions::new()
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
                _lock_guard = match _flock_storage.try_write() {
                    Ok(guard) => Some(guard),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        eprintln!(
                            "The database appears to be in use by a running server.\n\
                             Stop the server first, then try again with --fix."
                        );
                        std::process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("Cannot acquire process lock: {e}");
                        std::process::exit(1);
                    }
                };
            } else {
                // Satisfy the compiler — no lock needed for read-only diagnostics.
                _lock_guard = None;
            }

            let diag = match mokumo_db::diagnose_database(&db_path) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Cannot diagnose database at {}: {e}", db_path.display());
                    std::process::exit(1);
                }
            };

            let auto_vacuum_label = match diag.auto_vacuum {
                0 => "NONE",
                1 => "FULL",
                2 => "INCREMENTAL",
                _ => "UNKNOWN",
            };

            let db_size_bytes = diag.page_count * diag.page_size;
            let freelist_bytes = diag.freelist_count * diag.page_size;
            let fragmentation_pct = if diag.page_count > 0 {
                (diag.freelist_count as f64 / diag.page_count as f64) * 100.0
            } else {
                0.0
            };
            let wal_kb = diag.wal_size_bytes / 1024;
            let vacuum_needed =
                diag.page_count > 0 && (diag.freelist_count as f64 / diag.page_count as f64) > 0.20;

            println!("Database: {}", db_path.display());
            println!("  auto_vacuum:  {auto_vacuum_label} ({})", diag.auto_vacuum);
            println!("  page_size:    {} bytes", diag.page_size);
            println!(
                "  page_count:   {} ({} KB)",
                diag.page_count,
                db_size_bytes / 1024
            );
            println!(
                "  freelist:     {} pages ({} KB, {fragmentation_pct:.1}%)",
                diag.freelist_count,
                freelist_bytes / 1024
            );
            println!("  wal_size:     {wal_kb} KB");
            println!("  vacuum_needed:{vacuum_needed}");

            let mut issues_found = false;

            if diag.auto_vacuum != 2 {
                println!(
                    "\n  [WARN] auto_vacuum is not INCREMENTAL — database file will not shrink after deletions"
                );
                issues_found = true;
            }

            if fragmentation_pct > 10.0 {
                println!(
                    "\n  [WARN] freelist is {fragmentation_pct:.1}% of total pages — consider running with --fix"
                );
                issues_found = true;
            }

            if fix {
                println!();

                let needs_vacuum_upgrade = diag.auto_vacuum != 2;

                if needs_vacuum_upgrade {
                    println!("  Enabling auto_vacuum = INCREMENTAL...");
                    match mokumo_db::ensure_auto_vacuum(&db_path) {
                        Ok(()) => println!("  auto_vacuum upgraded successfully."),
                        Err(e) => {
                            eprintln!("  Failed to enable auto_vacuum: {e}");
                            std::process::exit(1);
                        }
                    }
                }

                // Re-diagnose to get a fresh freelist count after potential VACUUM.
                let diag2 = match mokumo_db::diagnose_database(&db_path) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("Cannot reopen database after fixes: {e}");
                        std::process::exit(1);
                    }
                };

                if diag2.freelist_count > 0 {
                    println!(
                        "  Running incremental_vacuum (reclaiming {} free pages)...",
                        diag2.freelist_count
                    );
                    let conn = match rusqlite::Connection::open(&db_path) {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("Cannot reopen database for incremental_vacuum: {e}");
                            std::process::exit(1);
                        }
                    };
                    match conn.execute_batch("PRAGMA incremental_vacuum") {
                        Ok(()) => {
                            let diag3 = mokumo_db::diagnose_database(&db_path).unwrap_or(diag2);
                            let reclaimed = diag2.freelist_count - diag3.freelist_count;
                            println!(
                                "  Reclaimed {reclaimed} pages ({} KB).",
                                reclaimed * diag.page_size / 1024
                            );
                        }
                        Err(e) => {
                            eprintln!("  incremental_vacuum failed: {e}");
                            std::process::exit(1);
                        }
                    }
                } else {
                    println!("  No free pages to reclaim.");
                }

                println!("\n  Doctor complete (fixes applied).");
            } else if issues_found {
                println!("\n  Run with --fix to attempt repairs.");
            } else {
                println!("\n  All checks passed.");
            }

            return;
        }
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
            let profile_dir = profile_dir(&data_dir, production);
            let db_path = profile_dir.join("mokumo.db");

            // Ensure data directories exist so the lock file can be created if needed.
            if let Err(e) = ensure_data_dirs(&data_dir) {
                eprintln!("Cannot create data directory {}: {e}", data_dir.display());
                std::process::exit(1);
            }

            // Early exit when neither profile database exists (idempotent, exit 0).
            // Use explicit match on each path so I/O errors surface rather than silently
            // becoming "not found" via unwrap_or(false).
            let demo_db = data_dir
                .join(SetupMode::Demo.as_dir_name())
                .join("mokumo.db");
            let production_db = data_dir
                .join(SetupMode::Production.as_dir_name())
                .join("mokumo.db");
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
                    let profile_name = if production {
                        SetupMode::Production.as_dir_name()
                    } else {
                        SetupMode::Demo.as_dir_name()
                    };
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
                    let port = read_lock_info(&lock_path);
                    let msg = format_reset_db_conflict_message(port);
                    eprintln!("{msg}");
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
        Some(Commands::Backup { output, production }) => {
            let profile = if production {
                SetupMode::Production
            } else {
                resolve_active_profile(&data_dir)
            };
            let db_path = data_dir.join(profile.as_dir_name()).join("mokumo.db");

            match db_path.try_exists() {
                Ok(false) => {
                    eprintln!("No database found at {}", db_path.display());
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Cannot access database path {}: {e}", db_path.display());
                    std::process::exit(1);
                }
                Ok(true) => {}
            }

            match cli_backup(&db_path, output.as_deref()) {
                Ok(result) => {
                    println!("Backup created: {}", result.path.display());
                    println!("Size: {} bytes", result.size);
                }
                Err(e) => {
                    eprintln!("Backup failed: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some(Commands::Restore { path, production }) => {
            let profile = if production {
                SetupMode::Production
            } else {
                resolve_active_profile(&data_dir)
            };
            let db_path = data_dir.join(profile.as_dir_name()).join("mokumo.db");

            if let Err(e) = ensure_data_dirs(&data_dir) {
                eprintln!("Cannot create data directory {}: {e}", data_dir.display());
                std::process::exit(1);
            }

            // Acquire process lock — refuse to restore while server is running
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

            match cli_restore(&db_path, &path) {
                Ok(result) => {
                    println!("Restored from: {}", result.restored_from.display());
                    if let Some(ref safety_path) = result.safety_backup_path {
                        println!(
                            "Safety backup of previous database: {}",
                            safety_path.display()
                        );
                    }
                    println!("Restore complete.");
                }
                Err(e) => {
                    eprintln!("Restore failed: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some(Commands::Migrate {
            action: MigrateCommands::Status { production },
        }) => {
            // -v/-q flags are accepted (global = true) but have no effect here:
            // output is plain println!/eprintln!, not tracing. init_tracing is
            // only called for the server startup path below.
            let profile = if production {
                SetupMode::Production
            } else {
                resolve_active_profile(&data_dir)
            };
            let db_path = data_dir.join(profile.as_dir_name()).join("mokumo.db");

            match db_path.try_exists() {
                Ok(true) => {}
                Ok(false) => {
                    eprintln!("No database found at {}", db_path.display());
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Cannot access database path {}: {e}", db_path.display());
                    std::process::exit(1);
                }
            }

            match cli_migrate_status(&db_path) {
                Ok(report) => {
                    let total = report.applied.len() + report.pending.len();
                    match &report.current_version {
                        Some(v) => println!("Current schema version: {v}"),
                        None => println!("Current schema version: (none — no migrations applied)"),
                    }
                    println!(
                        "Status: {}/{total} migrations applied",
                        report.applied.len()
                    );
                    println!();

                    if !report.unknown.is_empty() {
                        eprintln!(
                            "WARNING: {} migration(s) in DB not recognized by this binary \
                             (binary downgrade?)",
                            report.unknown.len()
                        );
                        for name in &report.unknown {
                            eprintln!("  [?] {name}");
                        }
                        eprintln!();
                    }

                    if !report.applied.is_empty() {
                        println!("Applied migrations:");
                        for m in &report.applied {
                            let ts = match m.applied_at {
                                Some(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                                None => "unknown timestamp".to_string(),
                            };
                            println!("  [x] {:<50} ({})", m.name, ts);
                        }
                        println!();
                    }

                    if report.pending.is_empty() {
                        println!("Pending migrations: none");
                    } else {
                        println!("Pending migrations:");
                        for name in &report.pending {
                            println!("  [ ] {name}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("migrate status failed: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        None => {} // No subcommand — fall through to server startup
    }

    let recovery_dir = mokumo_api::resolve_recovery_dir();
    let config = ServerConfig {
        port: cli.port,
        host: cli.host,
        data_dir,
        recovery_dir,
        #[cfg(debug_assertions)]
        ws_ping_ms: cli.ws_ping_ms,
    };

    // Create data directories (including demo/ and production/) before
    // initializing tracing — the file appender needs the logs/ dir to exist.
    if let Err(e) = ensure_data_dirs(&config.data_dir) {
        eprintln!(
            "Cannot create data directory {}: {e}",
            config.data_dir.display()
        );
        std::process::exit(1);
    }

    // Initialize tracing: human-readable console + JSON file output with daily
    // rotation and 7-day retention. The guard must live for the process lifetime
    // to ensure buffered log entries are flushed on shutdown.
    let console_level = console_level_from_flags(cli.quiet, cli.verbose);
    let _log_guard = init_tracing(Some(&config.data_dir.join("logs")), console_level);

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
            let port = read_lock_info(&lock_path);
            let msg = format_lock_conflict_message(port);
            eprintln!("{msg}");
            tracing::error!("Process lock already held — another server is running");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Cannot acquire process lock: {e}");
            tracing::error!("Cannot acquire process lock: {e}");
            std::process::exit(1);
        }
    };

    // Master shutdown token — signal handler cancels this once. Each loop iteration
    // creates a child token so individual restarts don't tear down the master signal.
    let master_shutdown = CancellationToken::new();

    // Server loop: runs once normally, restarts on demo reset.
    // Each iteration gets a fresh shutdown token, DB pool, and app state.
    {
        let token = master_shutdown.clone();
        tokio::spawn(async move {
            let shutdown_signal = async {
                #[cfg(unix)]
                {
                    let mut sigterm =
                        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                            .expect("failed to install SIGTERM handler");

                    tokio::select! {
                        result = tokio::signal::ctrl_c() => {
                            if let Err(e) = result {
                                tracing::error!(
                                    "Failed to listen for ctrl+c: {e}. \
                                     Server will continue running but graceful shutdown via Ctrl+C is unavailable."
                                );
                                // ctrl_c failed but SIGTERM may still work — wait for it
                                sigterm.recv().await;
                            }
                        }
                        _ = sigterm.recv() => {}
                    }
                }

                #[cfg(not(unix))]
                {
                    if let Err(e) = tokio::signal::ctrl_c().await {
                        tracing::error!(
                            "Failed to listen for ctrl+c: {e}. \
                             Server will continue running but graceful shutdown via Ctrl+C is unavailable."
                        );
                        std::future::pending::<()>().await;
                    }
                }
            };

            shutdown_signal.await;
            tracing::info!("Shutdown signal received, draining connections...");
            token.cancel();

            // Hard-stop timer: if drain takes longer than 10 seconds, force exit.
            // This starts AFTER the shutdown signal fires, not around the serve future.
            tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_secs(
                    mokumo_api::DRAIN_TIMEOUT_SECS,
                ))
                .await;
                tracing::warn!(
                    "Drain timeout elapsed ({}s), forcing shutdown",
                    mokumo_api::DRAIN_TIMEOUT_SECS
                );
                std::process::exit(0);
            });
        });
    }

    let mut bound_port: Option<u16> = None;

    loop {
        // Re-initialize databases on each iteration (demo reset may have replaced the demo DB file)
        let (demo_db, production_db, active_profile) =
            match prepare_database(&config.data_dir).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!("Database initialization failed: {e}");
                    std::process::exit(1);
                }
            };

        let shutdown_token = master_shutdown.child_token();
        let mdns_status = discovery::MdnsStatus::shared();

        let (app, _setup_token, _ws) = match build_app_with_shutdown(
            &config,
            demo_db,
            production_db,
            active_profile,
            shutdown_token.clone(),
            mdns_status.clone(),
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Failed to initialise application: {e}");
                std::process::exit(1);
            }
        };

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

        // Write port info to lock file so conflict messages are actionable.
        // Open a separate handle — the flock doesn't block same-process writes.
        match std::fs::OpenOptions::new().write(true).open(&lock_path) {
            Ok(f) => {
                if let Err(e) = write_lock_info(&f, actual_port) {
                    tracing::warn!("Failed to write port info to lock file: {e}");
                }
            }
            Err(e) => {
                tracing::warn!("Failed to open lock file for port info: {e}");
            }
        }

        if actual_port != config.port {
            tracing::warn!(
                "Requested port {} was unavailable, using port {} instead",
                config.port,
                actual_port
            );
        }

        {
            let mut s = mdns_status.write();
            s.port = actual_port;
            s.bind_host = config.host.clone();
        }

        let mdns_handle = discovery::register_mdns(
            &config.host,
            actual_port,
            &mdns_status,
            &discovery::RealDiscovery,
        );

        // If initial mDNS registration failed and we're on a LAN-facing address,
        // start background retry with backoff (60s, 120s, 300s cap).
        let mdns_retry = if mdns_handle.is_none() && !discovery::is_loopback(&config.host) {
            Some(discovery::spawn_mdns_retry(
                config.host.clone(),
                actual_port,
                mdns_status.clone(),
                std::sync::Arc::new(discovery::RealDiscovery),
                shutdown_token.clone(),
            ))
        } else {
            None
        };

        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown_token.cancelled().await;
            })
            .await
        {
            tracing::error!("Server error: {e}");
            std::process::exit(1);
        }

        // Cancel mDNS retry task if running — if it succeeded, deregister its handle too.
        if let Some(retry) = mdns_retry
            && let Some(retry_handle) = retry.cancel().await
        {
            discovery::deregister_mdns(retry_handle, &mdns_status);
        }
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
    fn long_version_contains_version_number() {
        let version = long_version();
        assert!(
            version.contains(env!("CARGO_PKG_VERSION")),
            "long_version should contain the package version"
        );
    }

    #[test]
    fn long_version_contains_git_hash() {
        let version = long_version();
        assert!(
            version.contains("git hash:"),
            "long_version should contain git hash label"
        );
    }

    #[test]
    fn long_version_contains_build_metadata() {
        let version = long_version();
        assert!(version.contains("built:"), "should contain build timestamp");
        assert!(version.contains("target:"), "should contain target triple");
        assert!(version.contains("rustc:"), "should contain rustc version");
    }

    #[test]
    fn cli_parses_version_subcommand() {
        use clap::Parser;
        let cli = Cli::try_parse_from(["mokumo", "version"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Version)));
    }

    #[test]
    fn cli_parses_version_flag() {
        use clap::Parser;
        let result = Cli::try_parse_from(["mokumo", "--version"]);
        // --version causes Clap to return an error with DisplayVersion kind
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

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
