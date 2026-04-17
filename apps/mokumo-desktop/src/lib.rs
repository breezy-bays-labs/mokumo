pub mod lifecycle;

use std::path::PathBuf;

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tokio_util::sync::CancellationToken;

use kikan_types::ServerStartupError;
use mokumo_api::discovery::MdnsHandle;
use mokumo_api::logging::init_tracing;
use mokumo_api::{
    ProfileDbError, ServerConfig, build_app_with_shutdown, discovery, prepare_database,
    try_bind_ephemeral_loopback,
};

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
    ws: std::sync::Arc<mokumo_api::ws::manager::ConnectionManager>,
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

/// Initialize the server: create dirs, backup, run migrations, build app.
///
/// Extracted so the orchestration sequence can be tested without a window system.
///
/// Uses **listener-passthrough**: the caller pre-binds via
/// [`try_bind_ephemeral_loopback`] and passes the [`TcpListener`] in.
/// This makes the port known to the caller before `init_server` runs,
/// enabling `initialization_script` and restart-loop port comparison
/// without re-binding internally.
async fn init_server(
    data_dir: PathBuf,
    listener: tokio::net::TcpListener,
    shutdown: CancellationToken,
) -> Result<ServerInit, Box<dyn std::error::Error + Send + Sync>> {
    let addr = listener.local_addr()?;
    let config = ServerConfig {
        port: addr.port(),
        host: addr.ip().to_string(), // "127.0.0.1"
        recovery_dir: mokumo_api::resolve_recovery_dir(),
        data_dir,
        #[cfg(debug_assertions)]
        ws_ping_ms: None,
    };

    // Shared startup: dirs, layout migration, sidecar copy, backup, DB init, non-active migration
    let (demo_db, production_db, active_profile) = prepare_database(&config.data_dir).await?;

    // Read LAN access consent from the active profile before the connections are moved
    // into build_app_with_shutdown. At M0 the desktop binds loopback so
    // register_mdns_with_consent is a no-op regardless; wired now so that when M1
    // enables LAN binds the user's consent already gates advertisement.
    let lan_access_db = match active_profile {
        kikan::SetupMode::Demo => demo_db.clone(),
        kikan::SetupMode::Production => production_db.clone(),
    };
    let lan_access_enabled = mokumo_api::settings::read_lan_access_enabled(&lan_access_db)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Falling back to LAN access disabled: {e:?}");
            false
        });

    // Pre-allocate mDNS status (will be populated after mDNS registration)
    let mdns_status = discovery::MdnsStatus::shared();

    let (router, setup_token, ws) = build_app_with_shutdown(
        &config,
        demo_db,
        production_db,
        active_profile,
        shutdown,
        mdns_status.clone(),
    )
    .await?;

    {
        let mut s = mdns_status.write();
        s.port = addr.port();
        s.bind_host = config.host.to_owned();
    }

    let mdns_handle = discovery::register_mdns_with_consent(
        &config.host,
        addr.port(),
        &mdns_status,
        &discovery::RealDiscovery,
        lan_access_enabled,
    );

    Ok(ServerInit {
        listener,
        router,
        port: addr.port(),
        setup_token,
        mdns_handle,
        mdns_status,
        ws,
    })
}

/// Map a human-readable startup error string to the appropriate [`ServerStartupError`] variant.
///
/// [`prepare_database`] formats errors as strings before returning them, so the desktop
/// layer must classify by inspecting the message rather than matching on error types.
/// The `backup_path` extracted from a [`ProfileDbError`] (if any) is forwarded into
/// `MigrationFailed` and `SchemaIncompatible` so the error dialog can show the restore
/// location to the shop owner.
///
/// # Limitations
/// `unknown_migrations` is always `vec![]` — the real list was available in the typed
/// `DatabaseSetupError::SchemaIncompatible` but is lost when `prepare_database` converts
/// errors to `String`.
fn classify_startup_error(
    message: &str,
    path: String,
    backup_path: Option<String>,
) -> ServerStartupError {
    if message.contains("newer version of Mokumo") {
        ServerStartupError::SchemaIncompatible {
            path,
            unknown_migrations: vec![],
            backup_path,
        }
    } else if message.contains("not a Mokumo database")
        || message.contains("not a valid Mokumo database")
    {
        // "not a valid Mokumo database" is the message from the post-reset guard path
        // (bundled sidecar failed check_application_id); "not a Mokumo database" is the
        // normal path. Both map to NotMokumoDatabase — no backup is taken before the
        // application_id check, so backup_path is not forwarded.
        ServerStartupError::NotMokumoDatabase { path }
    } else {
        ServerStartupError::MigrationFailed {
            path,
            message: message.to_owned(),
            backup_path,
        }
    }
}

/// Build a tray menu with info items and action items.
fn build_tray_menu(
    app: &tauri::AppHandle,
    port_text: &str,
    ip_text: &str,
    mdns_text: &str,
) -> Result<tauri::menu::Menu<tauri::Wry>, tauri::Error> {
    let port_item = MenuItemBuilder::with_id("info-port", port_text)
        .enabled(false)
        .build(app)?;
    let ip_item = MenuItemBuilder::with_id("info-ip", ip_text)
        .enabled(false)
        .build(app)?;
    let mdns_item = MenuItemBuilder::with_id("info-mdns", mdns_text)
        .enabled(false)
        .build(app)?;
    let sep1 = tauri::menu::PredefinedMenuItem::separator(app)?;
    let open_browser = MenuItemBuilder::with_id("open-browser", "Open in Browser").build(app)?;
    let reopen = MenuItemBuilder::with_id("reopen", "Reopen Desktop App").build(app)?;
    let sep2 = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Mokumo").build(app)?;

    MenuBuilder::new(app)
        .items(&[
            &port_item,
            &ip_item,
            &mdns_item,
            &sep1,
            &open_browser,
            &reopen,
            &sep2,
            &quit,
        ])
        .build()
}

/// Load the tray icon PNG for the given status variant.
fn load_tray_icon(variant: lifecycle::TrayIconVariant) -> tauri::image::Image<'static> {
    let bytes: &[u8] = match variant {
        lifecycle::TrayIconVariant::Green => include_bytes!("../icons/tray-green@2x.png"),
        lifecycle::TrayIconVariant::Yellow => include_bytes!("../icons/tray-yellow@2x.png"),
        lifecycle::TrayIconVariant::Red => include_bytes!("../icons/tray-red@2x.png"),
    };
    tauri::image::Image::from_bytes(bytes)
        .expect("embedded tray icon is valid PNG")
        .to_owned()
}

/// Show and focus the main window, restoring the dock icon on macOS.
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Regular);
}

/// Handle quit request: show confirmation dialog or send notification based on window visibility.
fn handle_quit(app: &tauri::AppHandle) {
    let window_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);

    let client_count = app
        .try_state::<std::sync::Arc<mokumo_api::ws::manager::ConnectionManager>>()
        .map(|h| h.connection_count())
        .unwrap_or(0);

    match lifecycle::on_quit_requested(window_visible) {
        lifecycle::QuitBehavior::ShowDialog => {
            let message = lifecycle::format_quit_message(client_count);
            let app_clone = app.clone();
            app.dialog()
                .message(message)
                .title("Quit Mokumo")
                .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancelCustom(
                    "Yes".into(),
                    "No".into(),
                ))
                .show(move |confirmed| {
                    if confirmed {
                        app_clone.exit(0);
                    }
                });
        }
        lifecycle::QuitBehavior::NotifyAndShutdown => {
            // Best-effort OS notification when quitting from hidden window
            use tauri_plugin_notification::NotificationExt;
            let _ = app
                .notification()
                .builder()
                .title("Mokumo")
                .body("Mokumo is shutting down")
                .show();
            app.exit(0);
        }
        lifecycle::QuitBehavior::ShutdownDirect => {
            app.exit(0);
        }
    }
}

pub fn run() {
    // Console-only tracing for now — desktop file logging will be added when
    // Tauri's app_data_dir path is wired into init_tracing after .setup().
    let _log_guard = init_tracing(None, None);

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![])
        // Opens target="_blank" links in the system browser (webview blocks them by default)
        .plugin(tauri_plugin_opener::init())
        // Native OS dialogs — used to show startup errors before the webview opens
        .plugin(tauri_plugin_dialog::init())
        // OS notifications — used for best-effort shutdown notification from hidden window
        .plugin(tauri_plugin_notification::init())
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

            // Clone the app handle before the blocking call so the dialog can use it
            // inside the map_err closure (app is &mut App, not movable into a closure).
            let dialog_handle = app.handle().clone();

            // Bind the ephemeral loopback port before calling init_server.
            // Listener-passthrough design: caller owns the bind so the SocketAddr is
            // known here for initialization_script composition.
            let (boot_listener, boot_addr) = tauri::async_runtime::block_on(
                try_bind_ephemeral_loopback(),
            )
            .map_err(|e| -> Box<dyn std::error::Error> {
                tracing::error!("Failed to bind ephemeral loopback: {e}");
                dialog_handle
                    .dialog()
                    .message(e.to_string())
                    .title("Mokumo — Startup Error")
                    .kind(MessageDialogKind::Error)
                    .blocking_show();
                Box::new(e) as Box<dyn std::error::Error>
            })?;

            let ServerInit {
                listener,
                router,
                port: actual_port,
                setup_token,
                mdns_handle,
                mdns_status,
                ws,
            } = tauri::async_runtime::block_on(init_server(
                data_dir,
                boot_listener,
                shutdown_token.clone(),
            ))
            .map_err(|e| -> Box<dyn std::error::Error> {
                tracing::error!("Server initialization failed: {e}");
                // Show a native OS error dialog before Tauri propagates the error and
                // exits. This fires before the webview opens, so blocking_show() is safe.
                dialog_handle
                    .dialog()
                    .message(e.to_string())
                    .title("Mokumo — Startup Error")
                    .kind(MessageDialogKind::Error)
                    .blocking_show();
                e
            })?;

            // Spawn the Axum server on Tauri's async runtime with restart loop.
            // On demo reset, the handler writes a ".restart" sentinel and cancels
            // the shutdown token. The loop detects the sentinel, re-initializes the
            // server with a fresh database and a new ephemeral loopback port.
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

                    // Listener-passthrough: bind before init_server so port is
                    // known before the server starts (enables port-change detect).
                    // STALE-GLOBAL INVARIANT: after navigate(), initialization_script
                    // re-fires with the ORIGINAL port (baked at webview-build time).
                    // Safe today because SPA uses same-origin relative paths only.
                    // DO NOT call apiBase() from active fetch paths until the restart
                    // loop reconstructs the WebviewWindow.
                    // Cross-ref: apps/web/src/lib/api/base.ts apiBase() export.
                    let (new_listener, new_addr) = match try_bind_ephemeral_loopback().await {
                        Ok(result) => result,
                        Err(e) => {
                            tracing::error!("Failed to bind ephemeral loopback on restart: {e}");
                            break;
                        }
                    };

                    let new_port = new_addr.port();
                    if new_port != port {
                        if let Some(window) = app_handle_for_server.get_webview_window("main") {
                            let new_url = format!("http://127.0.0.1:{new_port}");
                            if let Err(e) =
                                window.navigate(new_url.parse().expect("valid loopback URL"))
                            {
                                tracing::warn!("Failed to navigate webview after port change: {e}");
                            }
                        }
                    }

                    match init_server(data_dir.clone(), new_listener, new_shutdown).await {
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
                            // Extract the backup path from ProfileDbError if present so the
                            // error dialog can show the shop owner where their data is backed up.
                            let backup_path = e
                                .downcast_ref::<ProfileDbError>()
                                .and_then(|pde| pde.backup_path.as_ref())
                                .map(|p| p.display().to_string());
                            // The restart loop is only triggered by demo reset, so the relevant
                            // database is always the demo profile database.
                            let demo_db_path = data_dir
                                .join("demo")
                                .join("mokumo.db")
                                .display()
                                .to_string();
                            let error =
                                classify_startup_error(&e.to_string(), demo_db_path, backup_path);
                            app_handle_for_server.emit("server-error", error).ok();
                            break;
                        }
                    }
                }
            });

            // Store handles so ExitRequested can deregister mDNS and await server drain
            app.manage(ServerHandle(std::sync::Mutex::new(Some(server_handle))));
            let mdns_state = MdnsState {
                handle: std::sync::Mutex::new(mdns_handle),
                status: mdns_status.clone(),
            };
            app.manage(mdns_state);
            app.manage(ws.clone());

            let url = initial_webview_url("127.0.0.1", actual_port, setup_token.as_deref());
            let log_url = initial_webview_url(
                "127.0.0.1",
                actual_port,
                setup_token.as_ref().map(|_| "[redacted]"),
            );
            tracing::info!("Opening webview at {log_url}");

            // Inject the server address before SvelteKit mounts. boot_addr is the
            // SocketAddr from try_bind_ephemeral_loopback() — Display formats as
            // "127.0.0.1:{port}" so format!("http://{boot_addr}") is the full base URL.
            let init_script = format!("window.__MOKUMO_API_BASE__ = 'http://{}';", boot_addr);

            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(url.parse().expect("valid localhost URL")),
            )
            .title("Mokumo")
            .inner_size(1200.0, 800.0)
            .initialization_script(&init_script)
            .build()?;

            let port_text = lifecycle::format_tray_menu_port(actual_port);
            let ip = local_ip_address::local_ip().ok();
            let ip_text = lifecycle::format_tray_menu_ip(ip);
            let mdns_text = {
                let s = mdns_status.read();
                lifecycle::format_tray_menu_mdns(s.hostname.as_deref())
            };

            let tray_menu = build_tray_menu(app.handle(), &port_text, &ip_text, &mdns_text)?;

            let tooltip = lifecycle::format_tray_tooltip(ip, actual_port, None, 0);

            let initial_mdns_active = mdns_status.read().active;
            let initial_variant = lifecycle::tray_icon_for_status(initial_mdns_active, true);
            let initial_icon = load_tray_icon(initial_variant);

            let server_port = actual_port;
            let tray = TrayIconBuilder::with_id("main-tray")
                .icon(initial_icon)
                .tooltip(&tooltip)
                .menu(&tray_menu)
                .on_menu_event(
                    move |app: &tauri::AppHandle, event: tauri::menu::MenuEvent| match event
                        .id()
                        .as_ref()
                    {
                        "open-browser" => {
                            let url = format!("http://127.0.0.1:{server_port}");
                            if let Err(e) = tauri_plugin_opener::open_url(&url, None::<&str>) {
                                tracing::warn!("Failed to open browser: {e}");
                            }
                        }
                        "reopen" => {
                            show_main_window(app);
                        }
                        "quit" => {
                            handle_quit(app);
                        }
                        _ => {}
                    },
                )
                .on_tray_icon_event(|tray: &tauri::tray::TrayIcon, event: TrayIconEvent| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            let poll_mdns = mdns_status.clone();
            let poll_ws = ws;
            let poll_shutdown = shutdown_token.clone();
            let poll_tray = tray.clone();
            let poll_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut prev_variant = initial_variant;
                let mut prev_tooltip = tooltip;
                let mut prev_ip_text = ip_text;
                let mut prev_mdns_text = mdns_text;
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {}
                        _ = poll_shutdown.cancelled() => break,
                    }

                    let (mdns_active, hostname) = {
                        let s = poll_mdns.read();
                        (s.active, s.hostname.clone())
                    };
                    let client_count = poll_ws.connection_count();
                    let ip = local_ip_address::local_ip().ok();

                    let new_tooltip = lifecycle::format_tray_tooltip(
                        ip,
                        actual_port,
                        hostname.as_deref(),
                        client_count,
                    );
                    if new_tooltip != prev_tooltip {
                        let _ = poll_tray.set_tooltip(Some(&new_tooltip));
                        prev_tooltip = new_tooltip;
                    }

                    let variant = lifecycle::tray_icon_for_status(mdns_active, true);
                    if variant != prev_variant {
                        let _ = poll_tray.set_icon(Some(load_tray_icon(variant)));
                        prev_variant = variant;
                    }

                    let new_ip_text = lifecycle::format_tray_menu_ip(ip);
                    let new_mdns_text = lifecycle::format_tray_menu_mdns(hostname.as_deref());
                    if new_ip_text != prev_ip_text || new_mdns_text != prev_mdns_text {
                        if let Ok(menu) =
                            build_tray_menu(&poll_app, &port_text, &new_ip_text, &new_mdns_text)
                        {
                            let _ = poll_tray.set_menu(Some(menu));
                        }
                        prev_ip_text = new_ip_text;
                        prev_mdns_text = new_mdns_text;
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                // Check tray availability by trying to get the tray icon
                let tray_available = app.tray_by_id("main-tray").is_some();
                match lifecycle::on_close_requested(tray_available) {
                    lifecycle::CloseAction::HideToTray => {
                        api.prevent_close();
                        let _ = window.hide();
                        #[cfg(target_os = "macos")]
                        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                    }
                    lifecycle::CloseAction::ShowQuitConfirmation => {
                        api.prevent_close();
                        handle_quit(app);
                    }
                }
            }
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

                // Take the server handle and await drain with 10s timeout
                if let Some(handle) = app
                    .try_state::<ServerHandle>()
                    .and_then(|sh| sh.0.lock().ok()?.take())
                {
                    // Prevent immediate exit while we drain
                    api.prevent_exit();

                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(mokumo_api::DRAIN_TIMEOUT_SECS),
                            handle,
                        )
                        .await
                        {
                            Ok(_) => tracing::info!("Server drained, exiting"),
                            Err(_) => tracing::warn!("Drain timeout elapsed (10s), forcing exit"),
                        }
                        app_handle.exit(0);
                    });
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use kikan_types::ServerStartupError;

    use super::{classify_startup_error, initial_webview_url, webview_host};

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

    #[test]
    fn migration_failed_forwards_backup_path() {
        let err = classify_startup_error(
            "Migration error: something went wrong",
            "/data/demo/mokumo.db".to_string(),
            Some("/data/backups/mokumo.db.bak".to_string()),
        );
        assert_eq!(
            err,
            ServerStartupError::MigrationFailed {
                path: "/data/demo/mokumo.db".to_string(),
                message: "Migration error: something went wrong".to_string(),
                backup_path: Some("/data/backups/mokumo.db.bak".to_string()),
            }
        );
    }

    #[test]
    fn migration_failed_with_no_backup_path() {
        let err = classify_startup_error(
            "Migration error: something went wrong",
            "/data/demo/mokumo.db".to_string(),
            None,
        );
        assert_eq!(
            err,
            ServerStartupError::MigrationFailed {
                path: "/data/demo/mokumo.db".to_string(),
                message: "Migration error: something went wrong".to_string(),
                backup_path: None,
            }
        );
    }

    #[test]
    fn schema_incompatible_forwards_backup_path() {
        let err = classify_startup_error(
            "Database was created by a newer version of Mokumo",
            "/data/demo/mokumo.db".to_string(),
            Some("/data/backups/mokumo.db.bak".to_string()),
        );
        assert_eq!(
            err,
            ServerStartupError::SchemaIncompatible {
                path: "/data/demo/mokumo.db".to_string(),
                unknown_migrations: vec![],
                backup_path: Some("/data/backups/mokumo.db.bak".to_string()),
            }
        );
    }

    #[test]
    fn not_mokumo_database_ignores_backup_path() {
        // No backup is created before the application_id check, so backup_path is
        // never forwarded to this variant regardless of what the caller provides.
        let err = classify_startup_error(
            "not a Mokumo database",
            "/data/demo/mokumo.db".to_string(),
            Some("/data/backups/mokumo.db.bak".to_string()),
        );
        assert_eq!(
            err,
            ServerStartupError::NotMokumoDatabase {
                path: "/data/demo/mokumo.db".to_string(),
            }
        );
    }

    #[test]
    fn post_reset_guard_path_maps_to_not_mokumo_database() {
        let err = classify_startup_error(
            "not a valid Mokumo database",
            "/data/demo/mokumo.db".to_string(),
            None,
        );
        assert_eq!(
            err,
            ServerStartupError::NotMokumoDatabase {
                path: "/data/demo/mokumo.db".to_string(),
            }
        );
    }
}
