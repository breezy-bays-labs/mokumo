use std::path::PathBuf;

use clap::Parser;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use mokumo_api::{ServerConfig, build_app, ensure_data_dirs, try_bind};

#[derive(Parser)]
#[command(name = "mokumo", about = "Mokumo Print — production management server")]
struct Cli {
    /// Port to listen on
    #[arg(short, long, default_value = "6565")]
    port: u16,

    /// Address to bind to
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// Directory for application data (database, uploads)
    #[arg(long)]
    data_dir: Option<PathBuf>,
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

    // Initialize tracing
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|e| {
        if std::env::var_os("RUST_LOG").is_some() {
            eprintln!("WARNING: Invalid RUST_LOG value, falling back to 'info': {e}");
        }
        "info".into()
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let data_dir = cli.data_dir.unwrap_or_else(resolve_default_data_dir);

    let config = ServerConfig {
        port: cli.port,
        host: cli.host,
        data_dir,
    };

    // Create data directories
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

    // Pre-migration backup — fatal for existing databases, skipped for first run.
    // We check existence before calling pre_migration_backup so that an I/O error
    // on the path itself is treated as a real failure, not "first run".
    let db_path = config.data_dir.join("mokumo.db");
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

    // Build application
    let app = build_app(&config, pool);

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

    // Graceful shutdown via CancellationToken
    let shutdown_token = CancellationToken::new();
    let signal_token = shutdown_token.clone();

    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                tracing::info!("Shutdown signal received, draining connections...");
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
