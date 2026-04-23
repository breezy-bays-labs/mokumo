//! `mokumo-server` — headless Mokumo binary.
//!
//! Zero Tauri dependencies (invariant I3, CI-enforced).
//!
//! Subcommands follow the garage pattern (Pattern 3):
//! - `serve`     — start the data plane (TCP) + admin surface (UDS)
//! - `diagnose`  — structured diagnostics (daemon or direct DB)
//! - `bootstrap` — create the first admin account (no server needed)
//! - `backup`    — database backup operations (create, list)
//! - `profile`   — profile management (list, switch)
//! - `migrate`   — migration status

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
        /// Deployment posture: `lan` (private network HTTP + mDNS),
        /// `internet` (public HTTPS, CSRF on), or `reverse-proxy` (trust
        /// X-Forwarded-* from a fronting proxy, CSRF on).
        #[arg(long, default_value = "lan")]
        deployment_mode: DeploymentModeArg,

        /// TCP port for the data plane.
        #[arg(long, default_value = "6565")]
        port: u16,

        /// Bind host override. Defaults to `0.0.0.0` for `lan`, `127.0.0.1`
        /// for the other modes (expected behind a proxy or terminator).
        #[arg(long)]
        host: Option<String>,

        /// Allowed Host header value (repeatable). Required for
        /// `internet` / `reverse-proxy`. Lan-mode deployments may add mDNS
        /// names like `<shop>.local`.
        #[arg(long = "allowed-host")]
        allowed_hosts: Vec<String>,

        /// Allowed browser Origin (repeatable; e.g. `https://shop.example.com`).
        /// Required for `internet` / `reverse-proxy`.
        #[arg(long = "allowed-origin")]
        allowed_origins: Vec<String>,

        /// Serve the SvelteKit SPA from this directory (must contain
        /// `index.html`). Omit for API-only mode; non-API paths then
        /// return a 404.
        #[arg(long)]
        spa_dir: Option<PathBuf>,
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

    /// Database backup operations.
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },

    /// Manage profiles (demo / production).
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },

    /// Show migration status.
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },

    /// Reset a user's password (no running server required).
    ResetPassword {
        /// Email address of the user to reset.
        #[arg(long)]
        email: String,

        /// Path to a file containing the new password (one line).
        #[arg(long)]
        password_file: PathBuf,

        /// Target the production profile (default: active profile).
        #[arg(long)]
        production: bool,
    },

    /// Delete the database and start fresh (no running server required).
    ResetDb {
        /// Skip confirmation prompt.
        #[arg(long)]
        force: bool,

        /// Also delete backup files.
        #[arg(long)]
        include_backups: bool,

        /// Target the production profile (default: demo).
        #[arg(long)]
        production: bool,
    },

    /// Restore database from a backup file (no running server required).
    Restore {
        /// Path to the backup file.
        backup_file: PathBuf,

        /// Target the production profile (default: active profile).
        #[arg(long)]
        production: bool,
    },
}

#[derive(Subcommand)]
enum BackupAction {
    /// Create a database backup (no running server required).
    Create {
        /// Write the backup to this path instead of a timestamped default.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Back up the production profile (default: active profile).
        #[arg(long)]
        production: bool,
    },

    /// List existing pre-migration backups.
    List {
        /// Output raw JSON instead of human-readable summary.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    /// List available profiles with status.
    List {
        /// Output raw JSON instead of human-readable summary.
        #[arg(long)]
        json: bool,
    },

    /// Switch the active profile (requires running daemon).
    Switch {
        /// Target profile: demo or production.
        profile: String,

        /// Output raw JSON instead of human-readable summary.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum MigrateAction {
    /// Show applied migration status for all profiles.
    Status {
        /// Output raw JSON instead of human-readable summary.
        #[arg(long)]
        json: bool,
    },
}

/// clap-facing facade for `kikan::DeploymentMode`. Kept in this binary
/// because clap's `ValueEnum` derive lives closer to the CLI surface than
/// the engine crate.
#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum DeploymentModeArg {
    Lan,
    Internet,
    ReverseProxy,
}

impl From<DeploymentModeArg> for kikan::DeploymentMode {
    fn from(arg: DeploymentModeArg) -> Self {
        match arg {
            DeploymentModeArg::Lan => kikan::DeploymentMode::Lan,
            DeploymentModeArg::Internet => kikan::DeploymentMode::Internet,
            DeploymentModeArg::ReverseProxy => kikan::DeploymentMode::ReverseProxy,
        }
    }
}

struct ServeArgs {
    deployment_mode: DeploymentModeArg,
    port: u16,
    host: Option<String>,
    allowed_hosts: Vec<String>,
    allowed_origins: Vec<String>,
    spa_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let data_dir = cli.data_dir.unwrap_or_else(resolve_default_data_dir);

    match cli.command {
        None => {
            let serve_args = ServeArgs {
                deployment_mode: DeploymentModeArg::Lan,
                port: 6565,
                host: None,
                allowed_hosts: Vec::new(),
                allowed_origins: Vec::new(),
                spa_dir: None,
            };
            cmd_serve(data_dir, serve_args, cli.verbose, cli.quiet).await;
        }
        Some(Command::Serve {
            deployment_mode,
            port,
            host,
            allowed_hosts,
            allowed_origins,
            spa_dir,
        }) => {
            let serve_args = ServeArgs {
                deployment_mode,
                port,
                host,
                allowed_hosts,
                allowed_origins,
                spa_dir,
            };
            cmd_serve(data_dir, serve_args, cli.verbose, cli.quiet).await;
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
        Some(Command::Backup { action }) => match action {
            BackupAction::Create { output, production } => {
                cmd_backup(data_dir, output, production).await;
            }
            BackupAction::List { json } => {
                cmd_backup_list(data_dir, json).await;
            }
        },
        Some(Command::Profile { action }) => match action {
            ProfileAction::List { json } => {
                cmd_profile_list(data_dir, json).await;
            }
            ProfileAction::Switch { profile, json } => {
                cmd_profile_switch(data_dir, profile, json).await;
            }
        },
        Some(Command::Migrate { action }) => match action {
            MigrateAction::Status { json } => {
                cmd_migrate_status(data_dir, json).await;
            }
        },
        Some(Command::ResetPassword {
            email,
            password_file,
            production,
        }) => {
            cmd_reset_password(data_dir, email, password_file, production);
        }
        Some(Command::ResetDb {
            force,
            include_backups,
            production,
        }) => {
            cmd_reset_db(data_dir, force, include_backups, production);
        }
        Some(Command::Restore {
            backup_file,
            production,
        }) => {
            cmd_restore(data_dir, backup_file, production);
        }
    }
}

// ---------------------------------------------------------------------------
// serve
// ---------------------------------------------------------------------------

async fn cmd_serve(data_dir: PathBuf, args: ServeArgs, verbose: u8, quiet: bool) {
    let ServeArgs {
        deployment_mode,
        port,
        host: host_override,
        allowed_hosts: cli_allowed_hosts,
        allowed_origins: cli_allowed_origins,
        spa_dir,
    } = args;
    let deployment_mode: kikan::DeploymentMode = deployment_mode.into();

    // Bind host default follows the mode: Lan exposes the LAN, everything
    // else binds loopback and expects a proxy or terminator in front.
    let host: String = host_override.unwrap_or_else(|| match deployment_mode {
        kikan::DeploymentMode::Lan => "0.0.0.0".to_owned(),
        kikan::DeploymentMode::Internet | kikan::DeploymentMode::ReverseProxy => {
            "127.0.0.1".to_owned()
        }
    });

    // Parse + validate allowed_hosts and allowed_origins up front. Non-Lan
    // modes require both; Lan runs fine with empty lists (loopback is
    // always admitted, and mDNS names can be added by the caller).
    let parsed_hosts = match cli_allowed_hosts
        .iter()
        .map(kikan::HostPattern::parse)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Invalid --allowed-host value: {e}");
            std::process::exit(2);
        }
    };
    // Origin comparison against the CSRF allowlist is byte-exact, so normalize
    // to lowercase at parse time — browsers lowercase scheme + authority when
    // they emit the Origin header, but a sloppy operator might type
    // `HTTPS://Shop.Example.COM` and would otherwise land a silent mismatch.
    let parsed_origins = match cli_allowed_origins
        .iter()
        .map(|o| axum::http::HeaderValue::from_str(&o.to_ascii_lowercase()))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Invalid --allowed-origin value: {e}");
            std::process::exit(2);
        }
    };
    if deployment_mode != kikan::DeploymentMode::Lan
        && (parsed_hosts.is_empty() || parsed_origins.is_empty())
    {
        eprintln!(
            "--deployment-mode {deployment_mode} requires at least one \
             --allowed-host and --allowed-origin"
        );
        std::process::exit(2);
    }

    // Validate --spa-dir up front: if the caller asked for SPA serving,
    // the directory must already contain an `index.html`. Fail fast
    // rather than booting an engine that would serve 404s on every
    // non-API path. API-only mode (no --spa-dir) is advertised at info
    // level so operators can spot accidental omissions in logs.
    match spa_dir.as_ref() {
        Some(dir) => {
            let index = dir.join("index.html");
            if std::fs::metadata(&index).is_err() {
                eprintln!(
                    "spa directory {} has no index.html — did you run `pnpm build`?",
                    dir.display()
                );
                std::process::exit(2);
            }
        }
        None => {
            // Tracing isn't initialized yet — defer the log until after
            // `init_tracing` below so it actually reaches the configured
            // sinks.
        }
    }

    // Initialize tracing.
    let level = kikan::logging::console_level_from_flags(quiet, verbose);
    let _tracing_guard = kikan::logging::init_tracing(Some(&data_dir), level);

    match spa_dir.as_ref() {
        Some(dir) => {
            tracing::info!(spa_dir = %dir.display(), "mokumo-server serving SPA from disk");
        }
        None => {
            tracing::info!("mokumo-server starting in API-only mode (no --spa-dir provided)");
        }
    }

    // Ensure data directories exist.
    if let Err(e) = mokumo_shop::startup::ensure_data_dirs(&data_dir) {
        tracing::error!(
            "Cannot create data directories at {}: {e}",
            data_dir.display()
        );
        std::process::exit(1);
    }

    // Process-level lock.
    let lock_path = mokumo_shop::startup::lock_file_path(&data_dir);
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
    // Hold the lock guard for the process lifetime — dropping it releases the flock.
    let lock_guard = match flock.try_write() {
        Ok(g) => g,
        Err(_) => {
            let existing_port = mokumo_shop::startup::read_lock_info(&lock_path);
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
        match mokumo_shop::startup::prepare_database(&data_dir).await {
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

    // Session store + setup-token resolution (platform-level init that
    // precedes Engine::boot because `setup_completed` and `setup_token`
    // are PlatformState inputs).
    let session_db_path = data_dir.join("sessions.db");
    let (session_store, setup_completed, setup_token) =
        match mokumo_shop::startup::init_session_and_setup(&production_db, &session_db_path).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Session init failed: {e}");
                std::process::exit(1);
            }
        };
    let session_store_for_cleanup = session_store.clone();

    let demo_install_ok =
        mokumo_shop::startup::resolve_demo_install_ok(&demo_db, active_profile).await;

    let graft = {
        let base =
            mokumo_shop::graft::MokumoApp::new(setup_token.as_deref().map(std::sync::Arc::from));
        match spa_dir {
            Some(dir) => {
                base.with_spa_source(move || -> Box<dyn kikan::data_plane::spa::SpaSource> {
                    Box::new(kikan_spa_sveltekit::SvelteKitSpaDir::new(dir.clone()))
                })
            }
            None => base,
        }
    };
    let profile_initializer: kikan::platform_state::SharedProfileDbInitializer =
        std::sync::Arc::new(mokumo_shop::profile_db_init::MokumoProfileDbInitializer);
    let bind_addr: std::net::SocketAddr = match resolve_bind_addr(&host, port) {
        Ok(addr) => addr,
        Err(msg) => {
            eprintln!("Invalid --host/--port: {msg}");
            std::process::exit(2);
        }
    };
    let data_plane = kikan::DataPlaneConfig {
        deployment_mode,
        bind_addr,
        allowed_origins: parsed_origins,
        allowed_hosts: parsed_hosts,
    };
    let event_bus = kikan_events::BroadcastEventBus::new();
    let boot_config = kikan::BootConfig::new(data_dir.clone())
        .with_data_plane(data_plane)
        .with_subgraft(kikan_events::EventBusSubGraft::new(event_bus.clone()));
    let shutdown = CancellationToken::new();

    let mut pools: std::collections::HashMap<
        kikan::tenancy::ProfileDirName,
        sea_orm::DatabaseConnection,
    > = std::collections::HashMap::with_capacity(2);
    pools.insert(
        kikan::tenancy::ProfileDirName::from(kikan_types::SetupMode::Demo.as_dir_name()),
        demo_db,
    );
    pools.insert(
        kikan::tenancy::ProfileDirName::from(kikan_types::SetupMode::Production.as_dir_name()),
        production_db,
    );
    let active_profile_dir = kikan::tenancy::ProfileDirName::from(active_profile.as_dir_name());

    let (engine, app_state) = match kikan::Engine::<mokumo_shop::graft::MokumoApp>::boot(
        boot_config,
        &graft,
        pools,
        active_profile_dir,
        session_store,
        profile_initializer,
        setup_completed,
        demo_install_ok,
        shutdown.clone(),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Engine boot failed: {e}");
            std::process::exit(1);
        }
    };

    // mDNS status propagated from PlatformState for the later register_mdns call.
    let mdns_status = app_state.mdns_status().clone();

    // Domain background tasks (PIN sweep, PRAGMA optimize, local IP refresh).
    {
        use kikan::Graft;
        graft.spawn_background_tasks(&app_state);
    }

    // Platform background task: expire stale sessions every 60s.
    {
        use tower_sessions::session_store::ExpiredDeletion;
        let store = session_store_for_cleanup;
        let token = shutdown.clone();
        tokio::spawn(async move {
            tokio::select! {
                res = store.continuously_delete_expired(std::time::Duration::from_secs(60)) => {
                    if let Err(err) = res {
                        tracing::error!(error = %err, "session expiry cleanup task terminated");
                    }
                }
                _ = token.cancelled() => {}
            }
        });
    }

    // Compose the HTTP router with the 5-layer middleware stack.
    // mokumo-server is headless — it does NOT embed or serve the SPA.
    let router = engine.build_router(app_state.clone());

    // Bind TCP listener for the data plane.
    let (listener, actual_port) = match kikan::data_plane::bind::try_bind(&host, port).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Cannot bind to {host}:{port}: {e}");
            std::process::exit(1);
        }
    };

    // Write port info to lock file via the held fd.
    if let Err(e) = mokumo_shop::startup::write_lock_info(&lock_guard, actual_port) {
        tracing::warn!("Failed to write port info to lock file: {e}");
    }

    // Print setup token if setup is required.
    if let Some(token) = &setup_token {
        tracing::info!("Setup required — token: {token}");
        eprintln!("\n  Setup token: {token}\n");
    }

    // Build and spawn the admin UDS surface. We use a oneshot channel
    // to propagate bind errors back to the main task — if the admin
    // socket can't bind, startup fails rather than silently running
    // without the admin surface.
    let admin_socket = kikan_socket::admin_socket_path(&data_dir);
    let admin_router = mokumo_shop::admin::build_admin_router(app_state.platform_state());
    let admin_shutdown = shutdown.clone();
    let (admin_ready_tx, mut admin_ready_rx) =
        tokio::sync::oneshot::channel::<Result<(), String>>();
    let admin_handle = tokio::spawn(async move {
        match kikan_socket::serve_unix_socket(&admin_socket, admin_router, admin_shutdown).await {
            Ok(()) => {}
            Err(e) => {
                // If we haven't signalled ready yet, send the error.
                let _ = admin_ready_tx.send(Err(format!("Admin socket failed: {e}")));
            }
        }
    });
    // Give the admin socket a moment to bind, then check for early failure.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    if let Ok(Err(e)) = admin_ready_rx.try_recv() {
        tracing::error!("{e}");
        std::process::exit(1);
    }

    // Register mDNS (LAN mode only — kikan::DeploymentMode::mdns_enabled).
    let discovery = kikan::platform::discovery::RealDiscovery;
    let _mdns_handle = if deployment_mode.mdns_enabled() {
        kikan::platform::discovery::register_mdns(&host, actual_port, &mdns_status, &discovery)
    } else {
        None
    };

    tracing::info!(
        port = actual_port,
        host,
        admin_socket = %kikan_socket::admin_socket_path(&data_dir).display(),
        "mokumo-server ready"
    );

    // Serve with graceful shutdown. `into_make_service_with_connect_info::<SocketAddr>()`
    // inserts the TCP peer address into each request's extensions so the per-IP rate
    // limiter can key on it in Internet mode (no reverse proxy to inject X-Forwarded-For).
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
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
        .join(kikan_types::SetupMode::Production.as_dir_name())
        .join("mokumo.db");
    let demo_db_path = data_dir
        .join(kikan_types::SetupMode::Demo.as_dir_name())
        .join("mokumo.db");

    if !production_db_path.exists() && !demo_db_path.exists() {
        eprintln!(
            "No database found at {}. Run `mokumo-server serve` first.",
            data_dir.display()
        );
        std::process::exit(1);
    }

    let state = build_readonly_platform_state(&data_dir).await;

    match mokumo_shop::admin::diagnostics::collect(&state).await {
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
    // Read password with a 1 KiB size limit to prevent accidental memory
    // exhaustion from large files (e.g. /dev/zero). Strip only trailing
    // newlines — intentional leading/trailing spaces in the password are
    // preserved.
    let password = match std::fs::File::open(&password_file).and_then(|f| {
        use std::io::Read;
        let mut buf = vec![0u8; 1025]; // 1 KiB + 1 to detect overflow
        let n = f.take(1025).read(&mut buf)?;
        buf.truncate(n);
        String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }) {
        Ok(p) if p.len() > 1024 => {
            eprintln!("Password file exceeds 1 KiB — is this the right file?");
            std::process::exit(1);
        }
        Ok(p) => p.trim_end_matches(['\r', '\n']).to_string(),
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
    if let Err(e) = mokumo_shop::startup::ensure_data_dirs(&data_dir) {
        eprintln!("Cannot create data directories: {e}");
        std::process::exit(1);
    }

    // Prepare the production database (runs migrations).
    let (_demo_db, production_db, _active_profile) =
        match mokumo_shop::startup::prepare_database(&data_dir).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Database preparation failed: {e}");
                std::process::exit(1);
            }
        };

    // Build a minimal ControlPlaneState for bootstrap.
    let platform = build_bootstrap_platform_state(
        data_dir.clone(),
        _demo_db,
        production_db,
        kikan::tenancy::ProfileDirName::from(kikan_types::SetupMode::Production.as_dir_name()),
    );
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

            if let Some(path) = recovery_codes_file {
                // Write recovery codes to file with restrictive permissions.
                // Do NOT echo them to stdout — they are one-time secrets.
                let contents = outcome.recovery_codes.join("\n") + "\n";
                #[cfg(unix)]
                {
                    use std::io::Write;
                    use std::os::unix::fs::OpenOptionsExt;
                    match std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(&path)
                    {
                        Ok(mut f) => {
                            if let Err(e) = f.write_all(contents.as_bytes()) {
                                eprintln!(
                                    "Failed to write recovery codes to {}: {e}",
                                    path.display()
                                );
                                std::process::exit(1);
                            }
                            println!("Recovery codes written to: {} (mode 0600)", path.display());
                        }
                        Err(e) => {
                            eprintln!(
                                "Failed to create recovery codes file {}: {e}",
                                path.display()
                            );
                            std::process::exit(1);
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    if let Err(e) = std::fs::write(&path, contents) {
                        eprintln!("Failed to write recovery codes to {}: {e}", path.display());
                        std::process::exit(1);
                    }
                    println!("Recovery codes written to: {}", path.display());
                }
            } else {
                // Print to stdout only when no file path is specified.
                println!();
                println!("Recovery codes (save these — they cannot be shown again):");
                for code in &outcome.recovery_codes {
                    println!("  {code}");
                }
            }

            // Persist active_profile = production atomically (tmp-then-rename).
            let profile_path = data_dir.join("active_profile");
            let profile_tmp = data_dir.join("active_profile.tmp");
            if let Err(e) = std::fs::write(&profile_tmp, "production")
                .and_then(|()| std::fs::rename(&profile_tmp, &profile_path))
            {
                eprintln!(
                    "Warning: admin created but failed to persist active_profile: {e}. \
                     The server may start in demo mode — run `mokumo-server serve` to verify."
                );
            }
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
        kikan_types::SetupMode::Production
    } else {
        mokumo_shop::startup::resolve_active_profile(&data_dir)
    };
    let db_path = data_dir.join(profile.as_dir_name()).join("mokumo.db");

    if !db_path.exists() {
        eprintln!("No database found at {}", db_path.display());
        std::process::exit(1);
    }

    match mokumo_shop::cli::cli_backup(&db_path, output.as_deref()) {
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
// profile
// ---------------------------------------------------------------------------

async fn cmd_profile_list(data_dir: PathBuf, json: bool) {
    // Try the UDS client first (daemon running).
    let client = kikan_cli::UdsClient::for_data_dir(&data_dir);
    if client.daemon_available() {
        match kikan_cli::profile::list(&client, json).await {
            Ok(()) => return,
            Err(e) => {
                eprintln!("Warning: daemon socket exists but request failed: {e}");
                eprintln!("Falling back to direct database access...\n");
            }
        }
    }

    // Direct DB fallback — open read-only.
    let state = build_readonly_platform_state(&data_dir).await;
    match mokumo_shop::admin::profile_list::list_profiles(&state).await {
        Ok(resp) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&resp).expect("serialize")
                );
            } else {
                print_profile_list(&resp);
            }
        }
        Err(e) => {
            eprintln!("Profile listing failed: {e}");
            std::process::exit(1);
        }
    }
}

fn print_profile_list(resp: &kikan_types::admin::ProfileListResponse) {
    println!(
        "{:<14} {:<8} {:<10} {:<12}",
        "Profile", "Active", "Schema", "Size"
    );
    println!("{}", "\u{2500}".repeat(46));
    for p in &resp.profiles {
        let active = if p.active { "*" } else { "" };
        let size = match p.file_size_bytes {
            Some(bytes) if bytes >= 1_048_576 => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
            Some(bytes) if bytes >= 1024 => format!("{:.1} KB", bytes as f64 / 1024.0),
            Some(bytes) => format!("{bytes} B"),
            None => "n/a".to_string(),
        };
        println!(
            "{:<14} {:<8} v{:<9} {:<12}",
            p.name, active, p.schema_version, size
        );
    }
}

async fn cmd_profile_switch(data_dir: PathBuf, target: String, json: bool) {
    // Profile switch requires the daemon — no fallback.
    let client = kikan_cli::UdsClient::for_data_dir(&data_dir);
    if !client.daemon_available() {
        eprintln!(
            "Daemon not running. Profile switch requires a running server \
             because it changes in-memory state."
        );
        std::process::exit(10);
    }
    match kikan_cli::profile::switch(&client, &target, json).await {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Profile switch failed: {e}");
            std::process::exit(e.exit_code());
        }
    }
}

// ---------------------------------------------------------------------------
// migrate
// ---------------------------------------------------------------------------

async fn cmd_migrate_status(data_dir: PathBuf, json: bool) {
    // Try the UDS client first (daemon running).
    let client = kikan_cli::UdsClient::for_data_dir(&data_dir);
    if client.daemon_available() {
        match kikan_cli::migrate::status(&client, json).await {
            Ok(()) => return,
            Err(e) => {
                eprintln!("Warning: daemon socket exists but request failed: {e}");
                eprintln!("Falling back to direct database access...\n");
            }
        }
    }

    // Direct DB fallback — open read-only.
    let state = build_readonly_platform_state(&data_dir).await;
    match mokumo_shop::admin::migration_status::collect_migration_status(&state).await {
        Ok(resp) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&resp).expect("serialize")
                );
            } else {
                print_migration_status(&resp);
            }
        }
        Err(e) => {
            eprintln!("Migration status failed: {e}");
            std::process::exit(1);
        }
    }
}

fn print_migration_status(resp: &kikan_types::admin::MigrationStatusResponse) {
    print_profile_migrations("production", &resp.production);
    println!();
    print_profile_migrations("demo", &resp.demo);
}

fn print_profile_migrations(label: &str, status: &kikan_types::admin::ProfileMigrationStatus) {
    println!(
        "Migrations ({label}) \u{2014} {} applied, schema v{}",
        status.applied.len(),
        status.schema_version
    );
    if status.applied.is_empty() {
        println!("  (none)");
        return;
    }
    for m in &status.applied {
        println!("  {}::{}", m.graft_id, m.name);
    }
}

// ---------------------------------------------------------------------------
// backup list
// ---------------------------------------------------------------------------

async fn cmd_backup_list(data_dir: PathBuf, json: bool) {
    // Try the UDS client first (daemon running).
    let client = kikan_cli::UdsClient::for_data_dir(&data_dir);
    if client.daemon_available() {
        match kikan_cli::backup_cli::list(&client, json).await {
            Ok(()) => return,
            Err(e) => {
                eprintln!("Warning: daemon socket exists but request failed: {e}");
                eprintln!("Falling back to direct database access...\n");
            }
        }
    }

    // Direct fallback — scan for backup files on disk.
    let production_db = data_dir
        .join(kikan_types::SetupMode::Production.as_dir_name())
        .join("mokumo.db");
    let demo_db = data_dir
        .join(kikan_types::SetupMode::Demo.as_dir_name())
        .join("mokumo.db");

    let production = match collect_backup_entries(&production_db).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Warning: cannot scan production backups: {e}");
            kikan_types::ProfileBackups { backups: vec![] }
        }
    };
    let demo = match collect_backup_entries(&demo_db).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Warning: cannot scan demo backups: {e}");
            kikan_types::ProfileBackups { backups: vec![] }
        }
    };
    let resp = kikan_types::BackupStatusResponse { production, demo };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&resp).expect("serialize")
        );
    } else {
        print_backup_list(&resp);
    }
}

fn print_backup_list(resp: &kikan_types::BackupStatusResponse) {
    print_profile_backups("production", &resp.production);
    println!();
    print_profile_backups("demo", &resp.demo);
}

fn print_profile_backups(label: &str, backups: &kikan_types::ProfileBackups) {
    println!("Backups ({label}) \u{2014} {} found", backups.backups.len());
    if backups.backups.is_empty() {
        println!("  (none)");
        return;
    }
    for b in &backups.backups {
        println!("  {} ({})", b.version, b.backed_up_at);
    }
}

async fn collect_backup_entries(
    db_path: &std::path::Path,
) -> Result<kikan_types::ProfileBackups, String> {
    let backups = kikan::backup::collect_existing_backups(db_path)
        .await
        .map_err(|e| format!("cannot scan backups for {}: {e}", db_path.display()))?;

    let entries: Vec<kikan_types::BackupEntry> = backups
        .into_iter()
        .rev()
        .map(|(path, mtime)| {
            let version = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.rsplit_once(".backup-v"))
                .map(|(_, v)| v.to_owned())
                .unwrap_or_default();
            let backed_up_at = {
                use chrono::{DateTime, Utc};
                DateTime::<Utc>::from(mtime).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            };
            kikan_types::BackupEntry {
                path: path.display().to_string(),
                version,
                backed_up_at,
            }
        })
        .collect();

    Ok(kikan_types::ProfileBackups { backups: entries })
}

// ---------------------------------------------------------------------------
// reset-password
// ---------------------------------------------------------------------------

fn cmd_reset_password(data_dir: PathBuf, email: String, password_file: PathBuf, production: bool) {
    let profile = if production {
        kikan_types::SetupMode::Production
    } else {
        mokumo_shop::startup::resolve_active_profile(&data_dir)
    };
    let db_path = data_dir.join(profile.as_dir_name()).join("mokumo.db");

    if !db_path.exists() {
        eprintln!("No database found at {}", db_path.display());
        std::process::exit(1);
    }

    // Read password from file with a 1 KiB size limit (same pattern as bootstrap).
    let password = match std::fs::File::open(&password_file).and_then(|f| {
        use std::io::Read;
        let mut buf = vec![0u8; 1025];
        let n = f.take(1025).read(&mut buf)?;
        buf.truncate(n);
        String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }) {
        Ok(p) if p.len() > 1024 => {
            eprintln!("Password file exceeds 1 KiB: {}", password_file.display());
            std::process::exit(1);
        }
        Ok(p) => {
            let trimmed = p.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                eprintln!("Password file is empty: {}", password_file.display());
                std::process::exit(1);
            }
            trimmed.to_string()
        }
        Err(e) => {
            eprintln!("Cannot read password file {}: {e}", password_file.display());
            std::process::exit(1);
        }
    };

    match kikan_cli::reset_password::run(&db_path, &email, &password) {
        Ok(()) => {
            println!("Password reset successfully for {email}");
        }
        Err(e) => {
            eprintln!("Reset password failed: {e}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// reset-db
// ---------------------------------------------------------------------------

fn cmd_reset_db(data_dir: PathBuf, force: bool, include_backups: bool, production: bool) {
    let profile = if production {
        kikan_types::SetupMode::Production
    } else {
        kikan_types::SetupMode::Demo
    };
    let profile_dir = data_dir.join(profile.as_dir_name());

    // Flock guard — held through the entire reset to prevent concurrent server startup.
    let lock_path = mokumo_shop::startup::lock_file_path(&data_dir);
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
            std::process::exit(1);
        }
    };
    let mut flock = fd_lock::RwLock::new(lock_file);
    let _lock_guard = match flock.try_write() {
        Ok(guard) => guard,
        Err(_) => {
            eprintln!("Cannot reset database while the server is running.");
            eprintln!("Stop the server first, then retry.");
            std::process::exit(1);
        }
    };

    if !force {
        eprintln!("Use --force to skip the confirmation prompt.");
        std::process::exit(1);
    }

    let graft = mokumo_shop::graft::MokumoApp::default();

    match kikan_cli::reset_db::run(&graft, &profile_dir, include_backups) {
        Ok(report) => {
            if let Some((path, e)) = &report.backup_dir_error {
                eprintln!("Warning: could not scan backup dir {}: {e}", path.display());
            }

            if report.deleted.is_empty() && report.not_found.len() == 4 {
                println!("No database found for the {} profile", profile.as_str());
                println!("Nothing to reset.");
            } else {
                println!(
                    "Reset complete: {} files deleted, {} not found, {} failed",
                    report.deleted.len(),
                    report.not_found.len(),
                    report.failed.len()
                );
                for path in &report.deleted {
                    println!("  deleted: {}", path.display());
                }
                for (path, e) in &report.failed {
                    eprintln!("  FAILED: {}: {e}", path.display());
                }
            }

            if !report.failed.is_empty() {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Reset failed: {e}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// restore
// ---------------------------------------------------------------------------

fn cmd_restore(data_dir: PathBuf, backup_file: PathBuf, production: bool) {
    let profile = if production {
        kikan_types::SetupMode::Production
    } else {
        mokumo_shop::startup::resolve_active_profile(&data_dir)
    };
    let db_path = data_dir.join(profile.as_dir_name()).join("mokumo.db");

    // Flock guard — held through the entire restore to prevent concurrent server startup.
    let lock_path = mokumo_shop::startup::lock_file_path(&data_dir);
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
            std::process::exit(1);
        }
    };
    let mut flock = fd_lock::RwLock::new(lock_file);
    let _lock_guard = match flock.try_write() {
        Ok(guard) => guard,
        Err(_) => {
            eprintln!(
                "Cannot restore while the server is running — data directory is in use by a running server."
            );
            eprintln!("Stop the server first, then retry.");
            std::process::exit(1);
        }
    };

    let graft = mokumo_shop::graft::MokumoApp::default();

    match kikan_cli::restore::run(&graft, &db_path, &backup_file) {
        Ok(result) => {
            println!("Restored from: {}", result.restored_from.display());
            if let Some(safety) = &result.safety_backup_path {
                println!("Safety backup: {}", safety.display());
            }
            println!("Restore complete.");
        }
        Err(e) => {
            eprintln!("Restore failed: {e}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Build a read-only `PlatformState` for CLI fallback paths.
async fn build_readonly_platform_state(data_dir: &std::path::Path) -> kikan::PlatformState {
    let production_db_path = data_dir
        .join(kikan_types::SetupMode::Production.as_dir_name())
        .join("mokumo.db");
    let demo_db_path = data_dir
        .join(kikan_types::SetupMode::Demo.as_dir_name())
        .join("mokumo.db");
    let active_profile = mokumo_shop::startup::resolve_active_profile(data_dir);
    let demo_db = open_readonly_db(&demo_db_path).await;
    let production_db = open_readonly_db(&production_db_path).await;

    build_bootstrap_platform_state(
        data_dir.to_path_buf(),
        demo_db,
        production_db,
        kikan::tenancy::ProfileDirName::from(active_profile.as_dir_name()),
    )
}

/// Assemble a minimal `PlatformState` for CLI fallback / bootstrap paths —
/// where we do not run the full `Engine::boot` but still need a
/// PlatformState slice to reach pure control-plane fns.
fn build_bootstrap_platform_state(
    data_dir: PathBuf,
    demo_db: sea_orm::DatabaseConnection,
    production_db: sea_orm::DatabaseConnection,
    active_profile: kikan::tenancy::ProfileDirName,
) -> kikan::PlatformState {
    let demo_dir = kikan::tenancy::ProfileDirName::from(kikan_types::SetupMode::Demo.as_dir_name());
    let production_dir =
        kikan::tenancy::ProfileDirName::from(kikan_types::SetupMode::Production.as_dir_name());

    let mut pools = std::collections::HashMap::with_capacity(2);
    pools.insert(demo_dir.clone(), demo_db);
    pools.insert(production_dir.clone(), production_db);

    let profile_dir_names: std::sync::Arc<[kikan::tenancy::ProfileDirName]> =
        vec![production_dir.clone(), demo_dir.clone()].into();

    let mut requires_setup_by_dir = std::collections::HashMap::with_capacity(2);
    requires_setup_by_dir.insert(production_dir.clone(), true);
    requires_setup_by_dir.insert(demo_dir, false);

    kikan::PlatformState {
        data_dir,
        db_filename: "mokumo.db",
        pools: std::sync::Arc::new(pools),
        active_profile: std::sync::Arc::new(parking_lot::RwLock::new(active_profile)),
        profile_dir_names,
        requires_setup_by_dir: std::sync::Arc::new(requires_setup_by_dir),
        auth_profile_kind_dir: production_dir,
        shutdown: CancellationToken::new(),
        started_at: std::time::Instant::now(),
        mdns_status: kikan::MdnsStatus::shared(),
        demo_install_ok: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        is_first_launch: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        setup_completed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        profile_db_initializer: std::sync::Arc::new(NoOpProfileDbInitializer),
    }
}

/// Resolve `--host` + `--port` to a single [`SocketAddr`]. Accepts IPv4 /
/// IPv6 literals and hostnames (blocking DNS via [`ToSocketAddrs`]). This
/// runs once at startup before we begin serving — blocking is acceptable.
fn resolve_bind_addr(host: &str, port: u16) -> Result<std::net::SocketAddr, String> {
    use std::net::ToSocketAddrs;
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err("--host is empty".to_owned());
    }
    // Unbracket a bare IPv6 literal so `(host, port).to_socket_addrs()` can parse it.
    let host_for_lookup = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(trimmed);
    let mut iter = (host_for_lookup, port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {host}:{port}: {e}"))?;
    iter.next()
        .ok_or_else(|| format!("{host}:{port} resolved to no addresses"))
}

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
