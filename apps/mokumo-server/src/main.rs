//! `mokumo-server` — headless Mokumo binary.
//!
//! Zero Tauri dependencies (invariant I3, CI-enforced).
//!
//! Subcommands follow the garage pattern (Pattern 3):
//! - `serve`     — start the data plane (TCP) + admin surface (UDS)
//! - `diagnose`  — structured diagnostics (daemon or direct DB)
//! - `bootstrap` — create the first admin account (no server needed)
//! - `backup`    — create a database backup (no server needed)

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use tokio_util::sync::CancellationToken;

/// Mokumo headless server — production management for decorated apparel shops.
#[derive(Parser)]
#[command(
    name = "mokumo-server",
    about = "Mokumo headless server — no desktop UI, no Tauri",
    version
)]
struct Cli {
    /// Data directory override (defaults to MOKUMO_DATA_DIR env, then platform default).
    #[arg(long, env = "MOKUMO_DATA_DIR", global = true)]
    data_dir: Option<PathBuf>,

    /// Increase log verbosity: -v = debug, -vv = trace.
    #[arg(short, long, action = clap::ArgAction::Count, conflicts_with = "quiet", global = true)]
    verbose: u8,

    /// Suppress all log output except errors.
    #[arg(short, long, conflicts_with = "verbose", global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the HTTP data plane and Unix admin socket (default).
    Serve {
        /// Listening mode: lan = 0.0.0.0 (all interfaces), loopback = 127.0.0.1 only.
        #[arg(long, default_value = "lan")]
        mode: ServeMode,

        /// TCP port for the data plane.
        #[arg(long, default_value = "6565")]
        port: u16,
    },

    /// Show system diagnostics. Works with or without a running daemon.
    Diagnose {
        /// Output raw JSON instead of human-readable summary.
        #[arg(long)]
        json: bool,
    },

    /// Create the first admin account (no running server required).
    Bootstrap {
        /// Admin email address.
        #[arg(long)]
        email: String,

        /// Path to a file containing the admin password (one line).
        #[arg(long)]
        password_file: PathBuf,

        /// Write the 10 recovery codes to this file (default: stdout).
        #[arg(long)]
        recovery_codes_file: Option<PathBuf>,
    },

    /// Create a database backup (no running server required).
    Backup {
        /// Write the backup to this path instead of a timestamped default.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Back up the production profile (default: active profile).
        #[arg(long)]
        production: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ServeMode {
    /// Bind to 0.0.0.0 — reachable from LAN.
    Lan,
    /// Bind to 127.0.0.1 — localhost only.
    Loopback,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let data_dir = cli.data_dir.unwrap_or_else(resolve_default_data_dir);

    match cli.command {
        None | Some(Command::Serve { .. }) => {
            let (mode, port) = match &cli.command {
                Some(Command::Serve { mode, port }) => (*mode, *port),
                _ => (ServeMode::Lan, 6565),
            };
            cmd_serve(data_dir, mode, port, cli.verbose, cli.quiet).await;
        }
        Some(Command::Diagnose { json }) => {
            cmd_diagnose(data_dir, json).await;
        }
        Some(Command::Bootstrap {
            email,
            password_file,
            recovery_codes_file,
        }) => {
            cmd_bootstrap(data_dir, email, password_file, recovery_codes_file).await;
        }
        Some(Command::Backup { output, production }) => {
            cmd_backup(data_dir, output, production).await;
        }
    }
}

// ---------------------------------------------------------------------------
// serve
// ---------------------------------------------------------------------------

async fn cmd_serve(data_dir: PathBuf, mode: ServeMode, port: u16, verbose: u8, quiet: bool) {
    let host = match mode {
        ServeMode::Lan => "0.0.0.0",
        ServeMode::Loopback => "127.0.0.1",
    };

    // Initialize tracing.
    let level = mokumo_api::logging::console_level_from_flags(quiet, verbose);
    let _tracing_guard = mokumo_api::logging::init_tracing(Some(&data_dir), level);

    // Ensure data directories exist.
    if let Err(e) = mokumo_api::ensure_data_dirs(&data_dir) {
        tracing::error!(
            "Cannot create data directories at {}: {e}",
            data_dir.display()
        );
        std::process::exit(1);
    }

    // Process-level lock.
    let lock_path = mokumo_api::lock_file_path(&data_dir);
    let mut flock = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => fd_lock::RwLock::new(f),
        Err(e) => {
            tracing::error!("Cannot open lock file {}: {e}", lock_path.display());
            std::process::exit(1);
        }
    };
    let _lock_guard = match flock.try_write() {
        Ok(g) => g,
        Err(_) => {
            let existing_port = mokumo_api::read_lock_info(&lock_path);
            eprintln!(
                "Another mokumo process is running{}.",
                existing_port
                    .map(|p| format!(" (port {p})"))
                    .unwrap_or_default()
            );
            std::process::exit(1);
        }
    };

    // Prepare databases (guard chain: application_id, backup, auto_vacuum,
    // schema compat, pool init, migrations).
    let (demo_db, production_db, active_profile) =
        match mokumo_api::prepare_database(&data_dir).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Database preparation failed: {e}");
                if let Some(backup) = &e.backup_path {
                    eprintln!(
                        "A pre-migration backup is available at: {}",
                        backup.display()
                    );
                }
                std::process::exit(1);
            }
        };

    tracing::info!(
        active_profile = ?active_profile,
        data_dir = %data_dir.display(),
        "mokumo-server starting"
    );

    let shutdown = CancellationToken::new();
    let mdns_status = kikan::MdnsStatus::shared();

    let config = mokumo_api::ServerConfig {
        port,
        host: host.to_string(),
        data_dir: data_dir.clone(),
        recovery_dir: mokumo_api::resolve_recovery_dir(),
        #[cfg(debug_assertions)]
        ws_ping_ms: None,
    };

    let (router, setup_token, _ws, app_state) = match mokumo_api::build_app_with_shutdown(
        &config,
        demo_db,
        production_db,
        active_profile,
        shutdown.clone(),
        mdns_status.clone(),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to build application: {e}");
            std::process::exit(1);
        }
    };

    // Bind TCP listener for the data plane.
    let (listener, actual_port) = match mokumo_api::try_bind(host, port).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Cannot bind to {host}:{port}: {e}");
            std::process::exit(1);
        }
    };

    // Write port info to lock file. Open a separate handle — the flock
    // doesn't block same-process writes.
    match std::fs::OpenOptions::new().write(true).open(&lock_path) {
        Ok(f) => {
            if let Err(e) = mokumo_api::write_lock_info(&f, actual_port) {
                tracing::warn!("Failed to write port info to lock file: {e}");
            }
        }
        Err(e) => tracing::warn!("Failed to open lock file for port info: {e}"),
    }

    // Print setup token if setup is required.
    if let Some(token) = &setup_token {
        tracing::info!("Setup required — token: {token}");
        eprintln!("\n  Setup token: {token}\n");
    }

    // Build and spawn the admin UDS surface.
    let admin_socket = kikan_socket::admin_socket_path(&data_dir);
    let admin_router = mokumo_api::admin_uds::build_admin_uds_router(app_state.platform_state());
    let admin_shutdown = shutdown.clone();
    let admin_handle = tokio::spawn(async move {
        if let Err(e) =
            kikan_socket::serve_unix_socket(&admin_socket, admin_router, admin_shutdown).await
        {
            tracing::error!("Admin socket failed: {e}");
        }
    });

    // Register mDNS (LAN mode only).
    let discovery = kikan::platform::discovery::RealDiscovery;
    let _mdns_handle = if matches!(mode, ServeMode::Lan) {
        kikan::platform::discovery::register_mdns(host, actual_port, &mdns_status, &discovery)
    } else {
        None
    };

    tracing::info!(
        port = actual_port,
        host,
        admin_socket = %kikan_socket::admin_socket_path(&data_dir).display(),
        "mokumo-server ready"
    );

    // Serve with graceful shutdown.
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        // Wait for SIGTERM or SIGINT.
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => {},
                _ = sigterm.recv() => {},
            }
        }
        #[cfg(not(unix))]
        ctrl_c.await.ok();

        tracing::info!("Shutdown signal received — draining...");
        shutdown.cancel();
    });

    if let Err(e) = server.await {
        tracing::error!("Server error: {e}");
    }

    // Wait for admin socket to drain.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), admin_handle).await;
}

// ---------------------------------------------------------------------------
// diagnose
// ---------------------------------------------------------------------------

async fn cmd_diagnose(data_dir: PathBuf, json: bool) {
    // Try the UDS client first (daemon running).
    let client = kikan_cli::UdsClient::for_data_dir(&data_dir);
    if client.daemon_available() {
        match kikan_cli::diagnose::run(&client, json).await {
            Ok(()) => return,
            Err(e) => {
                eprintln!("Warning: daemon socket exists but request failed: {e}");
                eprintln!("Falling back to direct database access...\n");
            }
        }
    }

    // Direct DB fallback — open read-only, no migrations, no server.
    let production_db_path = data_dir
        .join(kikan::SetupMode::Production.as_dir_name())
        .join("mokumo.db");
    let demo_db_path = data_dir
        .join(kikan::SetupMode::Demo.as_dir_name())
        .join("mokumo.db");

    if !production_db_path.exists() && !demo_db_path.exists() {
        eprintln!(
            "No database found at {}. Run `mokumo-server serve` first.",
            data_dir.display()
        );
        std::process::exit(1);
    }

    // Build a minimal PlatformState for diagnostics::collect().
    let active_profile = mokumo_api::resolve_active_profile(&data_dir);
    let demo_db = open_readonly_db(&demo_db_path).await;
    let production_db = open_readonly_db(&production_db_path).await;

    let state = kikan::PlatformState {
        data_dir: data_dir.clone(),
        demo_db,
        production_db,
        active_profile: std::sync::Arc::new(parking_lot::RwLock::new(active_profile)),
        shutdown: CancellationToken::new(),
        started_at: std::time::Instant::now(),
        mdns_status: kikan::MdnsStatus::shared(),
        demo_install_ok: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        is_first_launch: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        setup_completed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        profile_db_initializer: std::sync::Arc::new(NoOpProfileDbInitializer),
    };

    match kikan::control_plane::diagnostics::collect(&state).await {
        Ok(diag) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&diag).expect("serialize diagnostics")
                );
            } else {
                print_diagnostics(&diag);
            }
        }
        Err(e) => {
            eprintln!("Diagnostics collection failed: {e}");
            std::process::exit(1);
        }
    }
}

fn print_diagnostics(diag: &kikan_types::diagnostics::DiagnosticsResponse) {
    println!(
        "{} v{} ({})",
        diag.app.name,
        diag.app.version,
        diag.app.build_commit.as_deref().unwrap_or("unknown commit")
    );
    println!();
    println!("Runtime");
    println!("  profile:       {:?}", diag.runtime.active_profile);
    println!(
        "  setup:         {}",
        if diag.runtime.setup_complete {
            "complete"
        } else {
            "pending"
        }
    );
    println!();
    println!("Database (production)");
    print_profile_db(&diag.database.production);
    println!("Database (demo)");
    print_profile_db(&diag.database.demo);
    println!("System");
    if let Some(host) = &diag.system.hostname {
        println!("  hostname:      {host}");
    }
    println!("  OS:            {} ({})", diag.os.family, diag.os.arch);
    println!(
        "  memory:        {} / {} MB",
        diag.system.used_memory_bytes / 1_048_576,
        diag.system.total_memory_bytes / 1_048_576
    );
    if let (Some(total), Some(free)) = (diag.system.disk_total_bytes, diag.system.disk_free_bytes) {
        println!(
            "  disk:          {} / {} MB free{}",
            free / 1_048_576,
            total / 1_048_576,
            if diag.system.disk_warning { " LOW" } else { "" }
        );
    }
}

fn print_profile_db(db: &kikan_types::diagnostics::ProfileDbDiagnostics) {
    println!("  schema:        v{}", db.schema_version);
    if let Some(size) = db.file_size_bytes {
        println!("  size:          {} KB", size / 1024);
    }
    println!(
        "  WAL:           {}",
        if db.wal_mode { "enabled" } else { "disabled" }
    );
    if db.vacuum_needed {
        println!("  vacuum:        needed");
    }
    println!();
}

// ---------------------------------------------------------------------------
// bootstrap
// ---------------------------------------------------------------------------

async fn cmd_bootstrap(
    data_dir: PathBuf,
    email: String,
    password_file: PathBuf,
    recovery_codes_file: Option<PathBuf>,
) {
    let password = match std::fs::read_to_string(&password_file) {
        Ok(p) => p.trim().to_string(),
        Err(e) => {
            eprintln!("Cannot read password file {}: {e}", password_file.display());
            std::process::exit(1);
        }
    };

    if password.len() < 8 {
        eprintln!("Password must be at least 8 characters");
        std::process::exit(1);
    }

    // Ensure data directories exist.
    if let Err(e) = mokumo_api::ensure_data_dirs(&data_dir) {
        eprintln!("Cannot create data directories: {e}");
        std::process::exit(1);
    }

    // Prepare the production database (runs migrations).
    let (_demo_db, production_db, _active_profile) =
        match mokumo_api::prepare_database(&data_dir).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Database preparation failed: {e}");
                std::process::exit(1);
            }
        };

    // Build a minimal ControlPlaneState for bootstrap.
    let platform = kikan::PlatformState {
        data_dir: data_dir.clone(),
        demo_db: _demo_db,
        production_db,
        active_profile: std::sync::Arc::new(parking_lot::RwLock::new(kikan::SetupMode::Production)),
        shutdown: CancellationToken::new(),
        started_at: std::time::Instant::now(),
        mdns_status: kikan::MdnsStatus::shared(),
        demo_install_ok: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        is_first_launch: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        setup_completed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        profile_db_initializer: std::sync::Arc::new(NoOpProfileDbInitializer),
    };
    let control_plane = kikan::ControlPlaneState {
        platform,
        login_limiter: std::sync::Arc::new(kikan::rate_limit::RateLimiter::new(
            10,
            std::time::Duration::from_secs(900),
        )),
        recovery_limiter: std::sync::Arc::new(kikan::rate_limit::RateLimiter::new(
            5,
            std::time::Duration::from_secs(900),
        )),
        regen_limiter: std::sync::Arc::new(kikan::rate_limit::RateLimiter::new(
            3,
            std::time::Duration::from_secs(3600),
        )),
        switch_limiter: std::sync::Arc::new(kikan::rate_limit::RateLimiter::new(
            3,
            std::time::Duration::from_secs(900),
        )),
        reset_pins: std::sync::Arc::new(dashmap::DashMap::new()),
        recovery_dir: mokumo_api::resolve_recovery_dir(),
        setup_token: None,
        setup_in_progress: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        activity_writer: std::sync::Arc::new(kikan::SqliteActivityWriter::new()),
    };

    let input = kikan::control_plane::users::BootstrapInput {
        email: email.clone(),
        name: "Admin".to_string(),
        password,
    };

    match kikan::control_plane::users::bootstrap_first_admin(&control_plane, input).await {
        Ok(outcome) => {
            println!("Admin account created: {email}");
            println!();
            println!("Recovery codes (save these — they cannot be shown again):");
            for code in &outcome.recovery_codes {
                println!("  {code}");
            }

            if let Some(path) = recovery_codes_file {
                let contents = outcome.recovery_codes.join("\n") + "\n";
                if let Err(e) = std::fs::write(&path, contents) {
                    eprintln!(
                        "Warning: failed to write recovery codes to {}: {e}",
                        path.display()
                    );
                } else {
                    println!("\nRecovery codes also written to: {}", path.display());
                }
            }

            // Persist active_profile = production.
            let profile_path = data_dir.join("active_profile");
            let _ = std::fs::write(&profile_path, "production");
        }
        Err(e) => {
            eprintln!("Bootstrap failed: {e}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// backup
// ---------------------------------------------------------------------------

async fn cmd_backup(data_dir: PathBuf, output: Option<PathBuf>, production: bool) {
    let profile = if production {
        kikan::SetupMode::Production
    } else {
        mokumo_api::resolve_active_profile(&data_dir)
    };
    let db_path = data_dir.join(profile.as_dir_name()).join("mokumo.db");

    if !db_path.exists() {
        eprintln!("No database found at {}", db_path.display());
        std::process::exit(1);
    }

    match mokumo_api::cli_backup(&db_path, output.as_deref()) {
        Ok(result) => {
            println!("Backup created: {}", result.path.display());
            println!("Size: {} bytes", result.size);
        }
        Err(e) => {
            eprintln!("Backup failed: {e}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Resolve the default data directory using platform conventions.
fn resolve_default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "breezybayslabs", "mokumo")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| {
            eprintln!(
                "WARNING: Could not determine platform data directory. \
                 Set --data-dir or MOKUMO_DATA_DIR."
            );
            PathBuf::from("./data")
        })
}

/// Open a SQLite database in read-only mode for diagnostics.
async fn open_readonly_db(path: &std::path::Path) -> sea_orm::DatabaseConnection {
    if !path.exists() {
        // Return an in-memory stub so diagnostics can still run for
        // the other profile.
        return kikan::db::initialize_database("sqlite::memory:")
            .await
            .expect("in-memory DB for diagnostics");
    }
    let url = format!("sqlite:{}?mode=ro", path.display());
    match kikan::db::initialize_database(&url).await {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Warning: cannot open {} read-only: {e}", path.display());
            kikan::db::initialize_database("sqlite::memory:")
                .await
                .expect("in-memory DB fallback")
        }
    }
}

/// No-op profile DB initializer for CLI contexts where demo reset
/// is never invoked.
struct NoOpProfileDbInitializer;

impl kikan::platform_state::ProfileDbInitializer for NoOpProfileDbInitializer {
    fn initialize<'a>(
        &'a self,
        _database_url: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<sea_orm::DatabaseConnection, kikan::db::DatabaseSetupError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(kikan::db::DatabaseSetupError::Migration(
                sea_orm::DbErr::Custom(
                    "profile re-init not supported in headless CLI mode".to_string(),
                ),
            ))
        })
    }
}
