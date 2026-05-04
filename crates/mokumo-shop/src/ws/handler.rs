//! WebSocket HTTP handler — upgrade, origin validation, heartbeat, broadcast relay.

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::state::SharedMokumoState as SharedState;

pub async fn ws_handler(
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> Result<impl IntoResponse, axum::http::StatusCode> {
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

    #[cfg(debug_assertions)]
    let ping_ms = state.ws_ping_ms();
    #[cfg(not(debug_assertions))]
    let ping_ms: Option<u64> = Some(30_000);

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, ping_ms)))
}

fn origin_host_port(origin: &str) -> Option<&str> {
    origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
}

fn is_allowed_origin(origin: &str, headers: &axum::http::HeaderMap) -> bool {
    let origin = origin.trim();

    if origin.starts_with("tauri://") {
        return true;
    }

    let Some(o_host_port) = origin_host_port(origin) else {
        return false;
    };

    if let Some(host_val) = headers.get(axum::http::header::HOST)
        && let Ok(host_str) = host_val.to_str()
    {
        return o_host_port.eq_ignore_ascii_case(host_str);
    }

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

    let mut ping_interval =
        ping_ms.map(|ms| tokio::time::interval(std::time::Duration::from_millis(ms)));

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
                        }
                    }
                }
                () = sender_shutdown.cancelled() => {
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
                () = async {
                    match ping_interval.as_mut() {
                        Some(i) => { i.tick().await; }
                        None => std::future::pending::<()>().await,
                    }
                } => {
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
                    if ws_sender.send(Message::Ping(axum::body::Bytes::new())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Pong(_))) => {
                        tracing::trace!(conn_id = %conn_id, "received pong");
                    }
                    Some(Ok(_)) => {}
                    _ => break,
                }
            }
            () = shutdown.cancelled() => break,
        }
    }

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
