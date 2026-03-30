use std::path::PathBuf;

use tauri::Manager;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use mokumo_api::discovery::MdnsHandle;
use mokumo_api::{ServerConfig, build_app_with_shutdown, discovery, prepare_database, try_bind};

const DEFAULT_PORT: u16 = 6565;
const DEFAULT_HOST: &str = "0.0.0.0";

/// Holds the server task handle so `ExitRequested` can await a clean drain.
struct ServerHandle(std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>);

/// Holds the mDNS handle + status so `ExitRequested` can deregister gracefully.
struct MdnsState {
    handle: std::sync::Mutex<Option<MdnsHandle>>,
    status: discovery::SharedMdnsStatus,
}

/// Holds the live shutdown token so `ExitRequested` always cancels the current server,
/// even after a demo-reset restart has replaced the original token.
struct ShutdownState(std::sync::Mutex<CancellationToken>);

/// Resources produced by `init_server`, consumed by Tauri setup.
struct ServerInit {
    listener: tokio::net::TcpListener,
    router: axum::Router,
    port: u16,
    setup_token: Option<String>,
    mdns_handle: Option<MdnsHandle>,
    mdns_status: discovery::SharedMdnsStatus,
}

/// Map the bind host to a routable address for the webview.
///
/// `0.0.0.0` means "all interfaces" — valid for `bind()` but not routable.
/// The webview always runs on the same machine, so rewrite to loopback.
fn webview_host(bind_host: &str) -> &str {
    if bind_host == "0.0.0.0" || bind_host == "::" {
        "127.0.0.1"
    } else {
        bind_host
    }
}

fn initial_webview_url(host: &str, port: u16, setup_token: Option<&str>) -> String {
    let host = webview_host(host);
    let path = match setup_token {
        Some(token) => format!("/setup?setup_token={token}"),
        None => "/".to_string(),
    };
    format!("http://{host}:{port}{path}")
}

/// Initialize the server: create dirs, backup, run migrations, build app, bind port.
///
/// Extracted so the orchestration sequence can be tested without a window system.
async fn init_server(
    data_dir: PathBuf,
    port: u16,
    host: &str,
    shutdown: CancellationToken,
) -> Result<ServerInit, Box<dyn std::error::Error>> {
    let config = ServerConfig {
        port,
        host: host.to_owned(),
        recovery_dir: mokumo_api::resolve_recovery_dir(),
        data_dir,
    };

    // Shared startup: dirs, layout migration, sidecar copy, backup, DB init, non-active migration
    let (demo_db, production_db, active_profile) = prepare_database(&config.data_dir).await?;

    // Pre-allocate mDNS status (will be populated after mDNS registration)
    let mdns_status = discovery::MdnsStatus::shared();

    let (router, setup_token) = build_app_with_shutdown(
        &config,
        demo_db,
        production_db,
        active_profile,
        shutdown,
        mdns_status.clone(),
    )
    .await?;

    // Bind to port (with fallback)
    let (listener, actual_port) = try_bind(&config.host, config.port).await?;

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
        s.bind_host = config.host.to_owned();
    }

    // Register mDNS (skipped if bound to loopback, active on 0.0.0.0)
    let mdns_handle = discovery::register_mdns(
        &config.host,
        actual_port,
        &mdns_status,
        &discovery::RealDiscovery,
    );

    Ok(ServerInit {
        listener,
        router,
        port: actual_port,
        setup_token,
        mdns_handle,
        mdns_status,
    })
}

pub fn run() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|e| {
        if std::env::var_os("RUST_LOG").is_some() {
            eprintln!("WARNING: Invalid RUST_LOG value, falling back to 'info': {e}");
        }
        "info".into()
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![])
        // Opens target="_blank" links in the system browser (webview blocks them by default)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Focus the existing window when a second instance is launched
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = window.set_focus() {
                    tracing::warn!("Failed to focus existing window: {e}");
                }
                if let Err(e) = window.unminimize() {
                    tracing::warn!("Failed to unminimize existing window: {e}");
                }
            }
        }))
        .setup(move |app| {
            // Resolve data directory via Tauri's path resolver
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data directory");

            tracing::info!("Data directory: {}", data_dir.display());

            // Point the API server to the bundled demo.db sidecar via resource_dir.
            // SAFETY: called once during single-threaded Tauri setup, before spawning
            // the server task. No other threads read this env var concurrently.
            if let Ok(resource_dir) = app.path().resource_dir() {
                let sidecar_path = resource_dir.join("demo.db");
                if sidecar_path.exists() {
                    unsafe { std::env::set_var("MOKUMO_DEMO_SIDECAR", &sidecar_path) };
                    tracing::info!("Demo sidecar: {}", sidecar_path.display());
                }
            }

            let shutdown_token = CancellationToken::new();
            app.manage(ShutdownState(std::sync::Mutex::new(shutdown_token.clone())));

            let server_token = shutdown_token.clone();
            let restart_data_dir = data_dir.clone();
            let app_handle_for_server = app.handle().clone();

            let ServerInit {
                listener,
                router,
                port: actual_port,
                setup_token,
                mdns_handle,
                mdns_status,
            } = tauri::async_runtime::block_on(init_server(
                data_dir,
                DEFAULT_PORT,
                DEFAULT_HOST,
                shutdown_token.clone(),
            ))
            .map_err(|e| {
                tracing::error!("Server initialization failed: {e}");
                e
            })?;

            // Spawn the Axum server on Tauri's async runtime with restart loop.
            // On demo reset, the handler writes a ".restart" sentinel and cancels
            // the shutdown token. The loop detects the sentinel, re-initializes the
            // server with the fresh database, and re-binds to the same port.
            let server_handle = tauri::async_runtime::spawn(async move {
                let data_dir = restart_data_dir;
                let mut port = actual_port;

                // First iteration uses the already-initialized server
                if let Err(e) = axum::serve(listener, router)
                    .with_graceful_shutdown(async move {
                        server_token.cancelled().await;
                    })
                    .await
                {
                    tracing::error!("Server error: {e}");
                    return;
                }

                // Check for restart sentinel after each shutdown
                loop {
                    let sentinel = data_dir.join(".restart");
                    if !sentinel.exists() {
                        tracing::info!("Server shut down cleanly");
                        break;
                    }
                    let _ = std::fs::remove_file(&sentinel);
                    tracing::info!("Restart requested — reinitializing server with fresh database");

                    // Deregister stale mDNS before restarting
                    if let Some(mdns) = app_handle_for_server.try_state::<MdnsState>() {
                        if let Some(handle) = mdns.handle.lock().ok().and_then(|mut h| h.take()) {
                            discovery::deregister_mdns(handle, &mdns.status);
                        }
                    }

                    let new_shutdown = CancellationToken::new();
                    let new_server_token = new_shutdown.clone();

                    // Expose the live token so ExitRequested cancels the right server
                    if let Some(state) = app_handle_for_server.try_state::<ShutdownState>() {
                        if let Ok(mut token) = state.0.lock() {
                            *token = new_shutdown.clone();
                        }
                    }

                    match init_server(data_dir.clone(), port, DEFAULT_HOST, new_shutdown)
                        .await
                        .map_err(|e| e.to_string())
                    {
                        Ok(init) => {
                            port = init.port;

                            // Store the new mDNS handle for cleanup on exit
                            if let Some(mdns) = app_handle_for_server.try_state::<MdnsState>() {
                                if let Ok(mut h) = mdns.handle.lock() {
                                    *h = init.mdns_handle;
                                }
                            }

                            if let Err(e) = axum::serve(init.listener, init.router)
                                .with_graceful_shutdown(async move {
                                    new_server_token.cancelled().await;
                                })
                                .await
                            {
                                tracing::error!("Server error after restart: {e}");
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to reinitialize server after reset: {e}");
                            break;
                        }
                    }
                }
            });

            // Store handles so ExitRequested can deregister mDNS and await server drain
            app.manage(ServerHandle(std::sync::Mutex::new(Some(server_handle))));
            app.manage(MdnsState {
                handle: std::sync::Mutex::new(mdns_handle),
                status: mdns_status,
            });

            let url = initial_webview_url(DEFAULT_HOST, actual_port, setup_token.as_deref());
            let log_url = initial_webview_url(
                DEFAULT_HOST,
                actual_port,
                setup_token.as_ref().map(|_| "[redacted]"),
            );
            tracing::info!("Opening webview at {log_url}");

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
        .run(move |app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = &event {
                tracing::info!("Exit requested, draining server...");

                // Deregister mDNS BEFORE cancelling the token (matches CLI behavior)
                if let Some(mdns) = app.try_state::<MdnsState>() {
                    if let Some(handle) = mdns.handle.lock().ok().and_then(|mut h| h.take()) {
                        discovery::deregister_mdns(handle, &mdns.status);
                    }
                }

                // Cancel the LIVE shutdown token (updated by restart loop)
                if let Some(state) = app.try_state::<ShutdownState>() {
                    if let Ok(token) = state.0.lock() {
                        token.cancel();
                    }
                }

                // Take the server handle and await drain before allowing exit
                if let Some(handle) = app
                    .try_state::<ServerHandle>()
                    .and_then(|sh| sh.0.lock().ok()?.take())
                {
                    // Prevent immediate exit while we drain
                    api.prevent_exit();

                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = handle.await;
                        tracing::info!("Server drained, exiting");
                        app_handle.exit(0);
                    });
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{initial_webview_url, webview_host};

    #[test]
    fn setup_url_prefills_setup_token() {
        assert_eq!(
            initial_webview_url("127.0.0.1", 6565, Some("test-token")),
            "http://127.0.0.1:6565/setup?setup_token=test-token"
        );
    }

    #[test]
    fn setup_complete_url_opens_dashboard_root() {
        assert_eq!(
            initial_webview_url("127.0.0.1", 6565, None),
            "http://127.0.0.1:6565/"
        );
    }

    #[test]
    fn wildcard_bind_rewrites_to_loopback_for_webview() {
        assert_eq!(webview_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(webview_host("::"), "127.0.0.1");
    }

    #[test]
    fn explicit_host_passes_through() {
        assert_eq!(webview_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(webview_host("192.168.1.50"), "192.168.1.50");
    }

    #[test]
    fn wildcard_bind_webview_url_uses_loopback() {
        assert_eq!(
            initial_webview_url("0.0.0.0", 6565, None),
            "http://127.0.0.1:6565/"
        );
        assert_eq!(
            initial_webview_url("0.0.0.0", 6565, Some("tok")),
            "http://127.0.0.1:6565/setup?setup_token=tok"
        );
    }
}
