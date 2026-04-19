// ConnectionManager moved to mokumo_shop::ws — re-export for backward compat.
pub mod manager {
    pub use mokumo_shop::ws::ConnectionManager;
}

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt, future::OptionFuture};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::SharedState;

pub async fn ws_handler(
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> Result<impl IntoResponse, axum::http::StatusCode> {
    // Validate Origin header to prevent cross-site WebSocket hijacking (CSWSH).
    // Browser clients send an Origin header; we verify it matches the request's
    // Host header (i.e. the page was served by this same server). This works for
    // localhost, LAN IPs, custom hostnames, and reverse proxies alike.
    // Non-browser clients (curl, native apps) typically omit Origin — allow those.
    if let Some(origin) = headers.get(axum::http::header::ORIGIN) {
        let origin_str = origin.to_str().unwrap_or("");
        if !is_allowed_origin(origin_str, &headers) {
            tracing::warn!(
                origin = origin_str,
                "WebSocket upgrade rejected: origin does not match Host header"
            );
            return Err(axum::http::StatusCode::FORBIDDEN);
        }
    }

    // Debug builds: use the --ws-ping-ms flag value (allows fast test cycles).
    // Release builds: always send heartbeats at 30 s so the client liveness
    // timer (75 s = 2.5 × 30 s) fires only on genuine server death.
    #[cfg(debug_assertions)]
    let ping_ms = state.ws_ping_ms();
    #[cfg(not(debug_assertions))]
    let ping_ms: Option<u64> = Some(30_000);

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, ping_ms)))
}

/// Extract the host and port from an Origin header value.
///
/// Origin format is always `scheme://host[:port]` with no path.
/// Returns the full `host[:port]` portion after stripping the scheme.
fn origin_host_port(origin: &str) -> Option<&str> {
    origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
}

/// Check whether a WebSocket Origin header is from a trusted source.
///
/// Compares the Origin's full `host:port` against the request's Host header — if
/// they match, the request came from a page served by this same server on the same
/// port. Always allows `tauri://` for the desktop shell. Falls back to
/// localhost-only when no Host header is present.
fn is_allowed_origin(origin: &str, headers: &axum::http::HeaderMap) -> bool {
    let origin = origin.trim();

    // Tauri webview always allowed
    if origin.starts_with("tauri://") {
        return true;
    }

    let o_host_port = match origin_host_port(origin) {
        Some(h) => h,
        None => return false,
    };

    // Compare full host:port from Origin against the Host header. Browsers set
    // Host to the target server (host:port) and Origin to the page's own origin
    // (scheme://host:port). Comparing the full host:port ensures a different app
    // on the same hostname but a different port cannot hijack the WebSocket.
    if let Some(host_val) = headers.get(axum::http::header::HOST)
        && let Ok(host_str) = host_val.to_str()
    {
        return o_host_port.eq_ignore_ascii_case(host_str);
    }

    // No Host header (HTTP/1.0 or weird client) — fall back to loopback only
    let host_only = o_host_port.split(':').next().unwrap_or(o_host_port);
    host_only == "localhost" || host_only == "127.0.0.1"
}

#[cfg(debug_assertions)]
pub async fn debug_connections(State(state): State<SharedState>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "count": state.ws().connection_count()
    }))
}

#[cfg(debug_assertions)]
pub async fn debug_broadcast(
    State(state): State<SharedState>,
    axum::Json(body): axum::Json<DebugBroadcastRequest>,
) -> impl IntoResponse {
    let event = kikan_types::ws::BroadcastEvent::new(
        body.type_,
        body.payload.unwrap_or(serde_json::Value::Null),
    );
    let count = state.ws().broadcast(event);
    axum::Json(serde_json::json!({ "receivers": count }))
}

#[cfg(debug_assertions)]
#[derive(serde::Deserialize)]
pub struct DebugBroadcastRequest {
    #[serde(rename = "type")]
    pub type_: String,
    pub payload: Option<serde_json::Value>,
}

async fn handle_socket(socket: WebSocket, state: SharedState, ping_ms: Option<u64>) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (conn_id, mut broadcast_rx) = state.ws().add();

    let shutdown = state.shutdown().clone();
    let sender_shutdown = shutdown.clone();
    let sender_conn_id = conn_id;

    // Optional heartbeat interval — only active when ping_ms is Some.
    // `OptionFuture` wraps the Option<Interval> so the select! arm is a
    // no-op (never fires) when None, without a separate conditional.
    let mut ping_interval =
        ping_ms.map(|ms| tokio::time::interval(std::time::Duration::from_millis(ms)));

    // Notify the sender task to stop when the receiver loop exits
    let sender_cancel = CancellationToken::new();
    let sender_cancel_token = sender_cancel.clone();

    let sender = tokio::spawn(async move {
        loop {
            tokio::select! {
                result = broadcast_rx.recv() => {
                    match result {
                        Ok(json) => {
                            if ws_sender.send(Message::Text((*json).into())).await.is_err() {
                                break;
                            }
                        }
                        Err(RecvError::Closed) => break,
                        Err(RecvError::Lagged(count)) => {
                            tracing::warn!(
                                conn_id = %sender_conn_id,
                                dropped = count,
                                "broadcast receiver lagged, messages dropped"
                            );
                            continue;
                        }
                    }
                }
                () = sender_shutdown.cancelled() => {
                    // Send server_shutting_down event before the close frame
                    // so clients know the server is going away intentionally.
                    let event = kikan_types::ws::BroadcastEvent::new(
                        "server_shutting_down",
                        serde_json::json!({}),
                    );
                    let json = serde_json::to_string(&event)
                        .expect("BroadcastEvent serialization cannot fail");
                    if let Err(e) = ws_sender.send(Message::Text(json.into())).await {
                        tracing::debug!(conn_id = %sender_conn_id, "Failed to send shutdown event: {e}");
                    }

                    let close = Message::Close(Some(axum::extract::ws::CloseFrame {
                        code: 1001,
                        reason: "server shutting down".into(),
                    }));
                    if let Err(e) = ws_sender.send(close).await {
                        tracing::debug!(conn_id = %sender_conn_id, "Failed to send close frame: {e}");
                    }
                    break;
                }
                () = sender_cancel_token.cancelled() => {
                    break;
                }
                // Heartbeat: JS-observable application-level ping + protocol-level Ping.
                // OptionFuture is a no-op (never fires) when ping_interval is None.
                _ = OptionFuture::from(ping_interval.as_mut().map(|i| i.tick())) => {
                    let Ok(hb) = serde_json::to_string(
                        &kikan_types::ws::BroadcastEvent::new(
                            "heartbeat",
                            serde_json::json!({}),
                        )
                    ) else {
                        tracing::error!(conn_id = %sender_conn_id, "heartbeat serialize failed — closing connection");
                        break;
                    };
                    if ws_sender.send(Message::Text(hb.into())).await.is_err() {
                        break;
                    }
                    if ws_sender.send(Message::Ping(Default::default())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Receiver loop: drain incoming messages, exit on shutdown or disconnect
    loop {
        tokio::select! {
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Pong(_))) => {
                        tracing::trace!(conn_id = %conn_id, "received pong");
                    }
                    Some(Ok(_)) => {} // ignore other client messages
                    _ => break,       // disconnected or error
                }
            }
            () = shutdown.cancelled() => break,
        }
    }

    // Clean up: if shutting down, let sender handle the close frame on its own.
    // If client disconnected, tell the sender to stop.
    if !shutdown.is_cancelled() {
        sender_cancel.cancel();
    }
    let _ = sender.await;
    state.ws().remove(conn_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_host(host: &str) -> axum::http::HeaderMap {
        let mut h = axum::http::HeaderMap::new();
        h.insert(axum::http::header::HOST, host.parse().unwrap());
        h
    }

    #[test]
    fn tauri_always_allowed() {
        let h = axum::http::HeaderMap::new();
        assert!(is_allowed_origin("tauri://localhost", &h));
    }

    #[test]
    fn origin_matches_host_header() {
        let h = headers_with_host("192.168.1.50:6565");
        assert!(is_allowed_origin("http://192.168.1.50:6565", &h));

        let h = headers_with_host("mokumo.local:6565");
        assert!(is_allowed_origin("http://mokumo.local:6565", &h));

        let h = headers_with_host("localhost:6565");
        assert!(is_allowed_origin("http://localhost:6565", &h));
    }

    #[test]
    fn different_port_rejected() {
        // A different app on the same host but different port must be rejected
        let h = headers_with_host("localhost:6565");
        assert!(!is_allowed_origin("http://localhost:3000", &h));

        let h = headers_with_host("mokumo.local:6565");
        assert!(!is_allowed_origin("http://mokumo.local:3000", &h));
    }

    #[test]
    fn cross_origin_rejected() {
        let h = headers_with_host("192.168.1.50:6565");
        assert!(!is_allowed_origin("http://evil.com", &h));
        assert!(!is_allowed_origin("http://localhost:6565", &h));
    }

    #[test]
    fn no_host_header_falls_back_to_loopback() {
        let h = axum::http::HeaderMap::new();
        assert!(is_allowed_origin("http://localhost:6565", &h));
        assert!(is_allowed_origin("http://127.0.0.1:6565", &h));
        assert!(!is_allowed_origin("http://192.168.1.50:6565", &h));
        assert!(!is_allowed_origin("http://evil.com", &h));
    }

    #[test]
    fn malformed_origins_rejected() {
        let h = headers_with_host("localhost:6565");
        assert!(!is_allowed_origin("", &h));
        assert!(!is_allowed_origin("not-a-url", &h));
        assert!(!is_allowed_origin("ftp://localhost", &h));
    }
}
