use std::path::PathBuf;

use tauri::Manager;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use mokumo_api::{ServerConfig, build_app, ensure_data_dirs, try_bind};

/// Initialize the server: create dirs, backup, init DB, build app, bind port.
///
/// Extracted so the orchestration sequence can be tested without a window system.
async fn init_server(
    data_dir: PathBuf,
    port: u16,
    host: &str,
) -> Result<(tokio::net::TcpListener, axum::Router, u16), Box<dyn std::error::Error>> {
    let config = ServerConfig {
        port,
        host: host.to_owned(),
        data_dir: data_dir.clone(),
    };

    ensure_data_dirs(&config.data_dir)?;

    // Pre-migration backup — fatal for existing databases, skipped for first run.
    let db_path = config.data_dir.join("mokumo.db");
    let db_exists = db_path
        .try_exists()
        .map_err(|e| format!("Cannot check database at {}: {e}", db_path.display()))?;
    if db_exists {
        mokumo_db::pre_migration_backup(&db_path).await?;
    }

    // Initialize database
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = mokumo_db::initialize_database(&database_url).await?;
    tracing::info!("Database ready at {}", db_path.display());

    // Build application
    let app = build_app(&config, pool);

    // Bind to port (with fallback)
    let (listener, actual_port) = try_bind(&config.host, config.port).await?;

    if actual_port != config.port {
        tracing::warn!(
            "Requested port {} was unavailable, using port {} instead",
            config.port,
            actual_port
        );
    }

    Ok((listener, app, actual_port))
}

pub fn run() {
    // Initialize tracing
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|e| {
        if std::env::var_os("RUST_LOG").is_some() {
            eprintln!("WARNING: Invalid RUST_LOG value, falling back to 'info': {e}");
        }
        "info".into()
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let shutdown_token = CancellationToken::new();
    let exit_token = shutdown_token.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Focus the existing window when a second instance is launched
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
                let _ = window.unminimize();
            }
        }))
        .setup(move |app| {
            // Resolve data directory via Tauri's path resolver
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data directory");

            tracing::info!("Data directory: {}", data_dir.display());

            let server_token = shutdown_token.clone();

            // Initialize server synchronously in setup (DB init, port binding)
            let (listener, router, actual_port) = tauri::async_runtime::block_on(init_server(
                data_dir, 6565, "0.0.0.0",
            ))
            .map_err(|e| {
                tracing::error!("Server initialization failed: {e}");
                e
            })?;

            // Spawn the Axum server on Tauri's async runtime (NOT tokio::spawn)
            tauri::async_runtime::spawn(async move {
                if let Err(e) = axum::serve(listener, router)
                    .with_graceful_shutdown(async move {
                        server_token.cancelled().await;
                    })
                    .await
                {
                    tracing::error!("Server error: {e}");
                }
                tracing::info!("Server shut down cleanly");
            });

            // Create the main window pointing to the local server
            let url = format!("http://localhost:{actual_port}");
            tracing::info!("Opening webview at {url}");

            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(url.parse().expect("valid localhost URL")),
            )
            .title("Mokumo")
            .inner_size(1200.0, 800.0)
            .build()?;

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_app, event| {
            if let tauri::RunEvent::Exit = event {
                tracing::info!("Tauri exiting, shutting down server...");
                exit_token.cancel();
            }
        });
}
