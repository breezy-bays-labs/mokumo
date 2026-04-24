//! Desktop lifecycle decision logic.
//!
//! Pure functions that drive Tauri event handler behavior, extracted from
//! handlers so they can be unit-tested without a Tauri runtime.

/// What to do when the user closes the window (X button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Hide the window and keep the server running in the tray.
    HideToTray,
    /// Show the quit confirmation dialog (tray unavailable, so close = quit).
    ShowQuitConfirmation,
}

/// What to do when the user requests to quit (Cmd+Q, tray menu "Quit Mokumo").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitBehavior {
    /// Show a confirmation dialog with client count.
    ShowDialog,
    /// Send a best-effort OS notification and proceed directly to shutdown.
    NotifyAndShutdown,
    /// Shut down immediately without dialog or notification.
    ShutdownDirect,
}

/// Which tray icon variant to display based on server state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIconVariant {
    /// Server running, mDNS active — everything healthy.
    Green,
    /// Server running, mDNS down — LAN discovery degraded.
    Yellow,
    /// Server not running — should not normally be visible.
    Red,
}

/// Decide what to do when the user clicks the window close button.
///
/// If the system tray is available, hide the window (keep server running).
/// If the tray is unavailable (e.g. Linux without tray support), show the
/// quit confirmation dialog instead — closing the window would kill the server.
pub fn on_close_requested(tray_available: bool) -> CloseAction {
    if tray_available {
        CloseAction::HideToTray
    } else {
        CloseAction::ShowQuitConfirmation
    }
}

/// Decide what to do when the user requests a quit.
///
/// If the window is visible, show a confirmation dialog (so the user knows
/// connected clients will lose access). If the window is hidden (already
/// minimized to tray), skip the dialog and send an OS notification instead.
pub fn on_quit_requested(window_visible: bool) -> QuitBehavior {
    if window_visible {
        QuitBehavior::ShowDialog
    } else {
        QuitBehavior::NotifyAndShutdown
    }
}

/// Determine which tray icon variant to show.
pub fn tray_icon_for_status(mdns_active: bool, server_running: bool) -> TrayIconVariant {
    if !server_running {
        TrayIconVariant::Red
    } else if mdns_active {
        TrayIconVariant::Green
    } else {
        TrayIconVariant::Yellow
    }
}

/// Format the port display for the tray menu.
///
/// Every ephemeral port is the authoritative OS-assigned port —
/// no fallback label needed.
pub fn format_tray_menu_port(port: u16) -> String {
    format!("Port: {port}")
}

/// Format the quit confirmation dialog message based on connected client count.
pub fn format_quit_message(connected_clients: usize) -> String {
    if connected_clients == 0 {
        "Do you want to shut down Mokumo?".to_string()
    } else {
        format!(
            "Do you want to shut down Mokumo? {} connected client(s) will lose access.",
            connected_clients
        )
    }
}

/// Format the lock conflict error message when another instance is running.
pub fn format_lock_conflict_message(port: Option<u16>) -> String {
    match port {
        Some(p) => format!(
            "Another Mokumo server is already running on port {p}.\n\
             Check your system tray, or open http://localhost:{p}"
        ),
        None => "Another Mokumo server is already running.\n\
                 Check your system tray."
            .to_string(),
    }
}

/// Whether to show the first-run nudge on the dashboard.
pub fn should_show_first_run_nudge(has_employee_sessions: bool) -> bool {
    !has_employee_sessions
}

/// Format the port exhaustion error for the desktop error dialog.
pub fn format_port_exhaustion_message(start_port: u16, end_port: u16) -> String {
    format!(
        "All ports {start_port}-{end_port} are occupied.\n\
         Close conflicting applications or use --port to specify a different port."
    )
}

/// Format the tray tooltip text showing server status.
pub fn format_tray_tooltip(
    ip: Option<std::net::IpAddr>,
    port: u16,
    mdns_hostname: Option<&str>,
    client_count: usize,
) -> String {
    let url = match ip {
        Some(ip) => format!("http://{ip}:{port}"),
        None => format!("http://127.0.0.1:{port}"),
    };
    let clients = match client_count {
        0 => "No clients connected".to_string(),
        1 => "1 client connected".to_string(),
        n => format!("{n} clients connected"),
    };
    match mdns_hostname {
        Some(h) => format!("Mokumo — {url} ({h}.local) — {clients}"),
        None => format!("Mokumo — {url} — {clients}"),
    }
}

/// Format the mDNS display text for the tray menu info item.
pub fn format_tray_menu_mdns(hostname: Option<&str>) -> String {
    match hostname {
        Some(h) => format!("mDNS: {h}.local"),
        None => "mDNS: unavailable".to_string(),
    }
}

/// Format the IP display text for the tray menu info item.
pub fn format_tray_menu_ip(ip: Option<std::net::IpAddr>) -> String {
    match ip {
        Some(ip) => format!("IP: {ip}"),
        None => "IP: detecting...".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Scenario 1: Closing the window hides to tray --
    #[test]
    fn closing_window_hides_to_tray() {
        assert_eq!(on_close_requested(true), CloseAction::HideToTray);
    }

    // -- Scenario 2: macOS dock icon hides (tray available → HideToTray) --
    #[test]
    fn macos_close_hides_to_tray() {
        // macOS always has tray support
        assert_eq!(on_close_requested(true), CloseAction::HideToTray);
    }

    // -- Scenario 3: Tray icon shows green when mDNS active --
    #[test]
    fn tray_icon_green_when_mdns_active() {
        assert_eq!(tray_icon_for_status(true, true), TrayIconVariant::Green);
    }

    // -- Scenario 4: Tray icon shows yellow when mDNS down --
    #[test]
    fn tray_icon_yellow_when_mdns_down() {
        assert_eq!(tray_icon_for_status(false, true), TrayIconVariant::Yellow);
    }

    // -- Scenario 5: Tray menu shows connection info --
    #[test]
    fn tray_menu_port_default() {
        assert_eq!(format_tray_menu_port(6565), "Port: 6565");
    }

    // -- Scenario 6: Reopen from tray (behavioral — covered by Tauri handler) --
    // Logic is "show + focus window" — no decision function needed.

    // -- Scenario 7: Open in Browser (behavioral — covered by Tauri handler) --

    // -- Scenario 8: Left-click tray reopens window (behavioral) --

    // -- Scenario 9: Quit from tray when window visible → show dialog --
    #[test]
    fn quit_from_tray_window_visible_shows_dialog() {
        assert_eq!(on_quit_requested(true), QuitBehavior::ShowDialog);
    }

    // -- Scenario 10: Cmd+Q triggers quit with confirmation --
    #[test]
    fn cmd_q_triggers_quit_confirmation() {
        // Cmd+Q when window is visible = same as quit with window visible
        assert_eq!(on_quit_requested(true), QuitBehavior::ShowDialog);
    }

    // -- Scenario 10: Quit message with no clients --
    #[test]
    fn quit_message_no_clients() {
        assert_eq!(format_quit_message(0), "Do you want to shut down Mokumo?");
    }

    // -- Scenario 10: Quit message with clients --
    #[test]
    fn quit_message_with_clients() {
        let msg = format_quit_message(3);
        assert!(msg.contains("3 connected client(s)"));
        assert!(msg.contains("lose access"));
    }

    // -- Scenario 11: Shutdown dialog shows progress (behavioral) --

    // -- Scenario 12: Shutdown completes within 10 seconds (covered by Session 0.2) --

    // -- Scenario 13: Cancel quit returns to normal --
    #[test]
    fn cancel_quit_returns_to_normal() {
        // After cancelling, on_quit_requested would be called again next time
        // The dialog dismiss is behavioral; the logic just returns ShowDialog
        assert_eq!(on_quit_requested(true), QuitBehavior::ShowDialog);
    }

    // -- Scenario 14: Quit from tray when window hidden → notify + shutdown --
    #[test]
    fn quit_from_tray_window_hidden_notifies() {
        assert_eq!(on_quit_requested(false), QuitBehavior::NotifyAndShutdown);
    }

    // -- Scenario 16: Port exhaustion error dialog --
    #[test]
    fn port_exhaustion_error_message() {
        let msg = format_port_exhaustion_message(6565, 6575);
        assert!(msg.contains("6565-6575"));
        assert!(msg.contains("occupied"));
    }

    // -- Scenario 17: Close behaves as quit when tray unavailable --
    #[test]
    fn close_behaves_as_quit_when_tray_unavailable() {
        assert_eq!(on_close_requested(false), CloseAction::ShowQuitConfirmation);
    }

    // -- Additional edge cases --

    #[test]
    fn tray_icon_red_when_server_not_running() {
        assert_eq!(tray_icon_for_status(false, false), TrayIconVariant::Red);
    }

    #[test]
    fn tray_icon_red_even_with_mdns_when_server_down() {
        assert_eq!(tray_icon_for_status(true, false), TrayIconVariant::Red);
    }

    #[test]
    fn lock_conflict_message_with_port() {
        let msg = format_lock_conflict_message(Some(6565));
        assert!(msg.contains("port 6565"));
        assert!(msg.contains("system tray"));
        assert!(msg.contains("http://localhost:6565"));
    }

    #[test]
    fn lock_conflict_message_without_port() {
        let msg = format_lock_conflict_message(None);
        assert!(msg.contains("already running"));
        assert!(msg.contains("system tray"));
        assert!(!msg.contains("http://"));
    }

    #[test]
    fn first_run_nudge_shown_when_no_employees() {
        assert!(should_show_first_run_nudge(false));
    }

    #[test]
    fn first_run_nudge_hidden_when_employees_exist() {
        assert!(!should_show_first_run_nudge(true));
    }

    // -- Tray tooltip tests --

    #[test]
    fn tray_tooltip_with_ip_and_mdns_and_clients() {
        let ip = Some(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
            192, 168, 1, 50,
        )));
        let tooltip = format_tray_tooltip(ip, 6565, Some("mokumo"), 3);
        assert!(tooltip.contains("http://192.168.1.50:6565"));
        assert!(tooltip.contains("mokumo.local"));
        assert!(tooltip.contains("3 clients connected"));
    }

    #[test]
    fn tray_tooltip_no_mdns_no_clients() {
        let ip = Some(std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 1)));
        let tooltip = format_tray_tooltip(ip, 6567, None, 0);
        assert!(tooltip.contains("http://10.0.0.1:6567"));
        assert!(!tooltip.contains(".local"));
        assert!(tooltip.contains("No clients connected"));
    }

    #[test]
    fn tray_tooltip_one_client_singular() {
        let ip = Some(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
            192, 168, 1, 50,
        )));
        let tooltip = format_tray_tooltip(ip, 6565, None, 1);
        assert!(tooltip.contains("1 client connected"));
        assert!(!tooltip.contains("clients"));
    }

    #[test]
    fn tray_tooltip_no_ip_falls_back_to_loopback() {
        let tooltip = format_tray_tooltip(None, 6565, None, 0);
        assert!(tooltip.contains("http://127.0.0.1:6565"));
    }

    // -- Tray menu info tests --

    #[test]
    fn tray_menu_mdns_active() {
        assert_eq!(format_tray_menu_mdns(Some("mokumo")), "mDNS: mokumo.local");
    }

    #[test]
    fn tray_menu_mdns_inactive() {
        assert_eq!(format_tray_menu_mdns(None), "mDNS: unavailable");
    }

    #[test]
    fn tray_menu_ip_with_address() {
        let ip = Some(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
            192, 168, 1, 50,
        )));
        assert_eq!(format_tray_menu_ip(ip), "IP: 192.168.1.50");
    }

    #[test]
    fn tray_menu_ip_detecting() {
        assert_eq!(format_tray_menu_ip(None), "IP: detecting...");
    }
}
