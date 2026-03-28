use std::path::PathBuf;

use clap::Parser;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use mokumo_api::{
    ServerConfig, build_app_with_shutdown, cli_reset_password, discovery, ensure_data_dirs,
    migrate_flat_layout, resolve_active_profile, try_bind,
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
    if let Some(Commands::ResetPassword { email }) = cli.command {
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

    // Migrate flat layout to dual-directory structure (idempotent)
    if let Err(e) = migrate_flat_layout(&config.data_dir) {
        eprintln!("Failed to migrate data directory layout: {e}");
        tracing::error!("Failed to migrate data directory layout: {e}");
        std::process::exit(1);
    }

    // Resolve which profile to use
    let profile = resolve_active_profile(&config.data_dir);
    let db_path = config.data_dir.join(profile.as_str()).join("mokumo.db");

    // Pre-migration backup on ACTIVE PROFILE DB
    let db_exists = match db_path.try_exists() {
        Ok(exists) => exists,
        Err(e) => {
            eprintln!("Cannot check database at {}: {e}", db_path.display());
            tracing::error!("Cannot check database at {}: {e}", db_path.display());
            std::process::exit(1);
        }
    };
    if db_exists && let Err(e) = mokumo_db::pre_migration_backup(&db_path).await {
        eprintln!(
            "Pre-migration backup failed for {}: {e}. \
             Refusing to run migrations without a backup. \
             Check disk space and permissions.",
            db_path.display()
        );
        tracing::error!("Pre-migration backup failed for existing database: {e}");
        std::process::exit(1);
    }

    // Initialize database
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = match mokumo_db::initialize_database(&database_url).await {
        Ok(pool) => {
            tracing::info!("Database ready at {}", db_path.display());
            pool
        }
        Err(e) => {
            tracing::error!("Failed to initialize database: {e}");
            std::process::exit(1);
        }
    };

    // Graceful shutdown via CancellationToken
    let shutdown_token = CancellationToken::new();

    // Pre-allocate mDNS status (default: inactive)
    let mdns_status = discovery::MdnsStatus::shared();

    // Build application (now async — initializes session store)
    let (app, _setup_token) =
        build_app_with_shutdown(&config, pool, shutdown_token.clone(), mdns_status.clone()).await;

    // Bind to port (with fallback)
    let (listener, actual_port) = match try_bind(&config.host, config.port).await {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{e}");
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };

    if actual_port != config.port {
        tracing::warn!(
            "Requested port {} was unavailable, using port {} instead",
            config.port,
            actual_port
        );
    }

    // Record the bound port and bind host so /api/server-info always knows them
    {
        let mut s = mdns_status.write().expect("MdnsStatus lock poisoned");
        s.port = actual_port;
        s.bind_host = config.host.clone();
    }

    // Register mDNS after binding (uses actual bound port)
    let mdns_handle = discovery::register_mdns(
        &config.host,
        actual_port,
        &mdns_status,
        &discovery::RealDiscovery,
    );

    let signal_token = shutdown_token.clone();
    let shutdown_status = mdns_status.clone();

    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                tracing::info!("Shutdown signal received, draining connections...");
                // Deregister mDNS BEFORE cancelling the token
                if let Some(handle) = mdns_handle {
                    discovery::deregister_mdns(handle, &shutdown_status);
                }
                signal_token.cancel();
            }
            Err(e) => {
                tracing::error!(
                    "Failed to listen for ctrl+c: {e}. \
                     Server will continue running but graceful shutdown via Ctrl+C is unavailable."
                );
                // Do NOT cancel — let the server keep running.
            }
        }
    });

    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_token.cancelled().await;
        })
        .await
    {
        tracing::error!("Server error: {e}");
        std::process::exit(1);
    }

    tracing::info!("Server shut down cleanly");
}

#[cfg(test)]
mod tests {
    use super::*;
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
