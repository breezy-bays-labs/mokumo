pub mod manager;

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use crate::SharedState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

#[cfg(debug_assertions)]
pub async fn debug_connections(State(state): State<SharedState>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "count": state.ws.connection_count()
    }))
}

#[cfg(debug_assertions)]
pub async fn debug_broadcast(
    State(state): State<SharedState>,
    axum::Json(body): axum::Json<DebugBroadcastRequest>,
) -> impl IntoResponse {
    let event = mokumo_types::ws::BroadcastEvent {
        v: 1,
        type_: body.type_,
        topic: body.topic,
        payload: body.payload.unwrap_or(serde_json::Value::Null),
    };
    let count = state.ws.broadcast(event);
    axum::Json(serde_json::json!({ "receivers": count }))
}

#[cfg(debug_assertions)]
#[derive(serde::Deserialize)]
pub struct DebugBroadcastRequest {
    #[serde(rename = "type")]
    pub type_: String,
    pub topic: String,
    pub payload: Option<serde_json::Value>,
}

async fn handle_socket(socket: WebSocket, state: SharedState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (conn_id, mut broadcast_rx) = state.ws.add();

    let shutdown = state.shutdown.clone();
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
                        Err(RecvError::Lagged(_)) => continue,
                    }
                }
                () = shutdown.cancelled() => {
                    let close = Message::Close(Some(axum::extract::ws::CloseFrame {
                        code: 1001,
                        reason: "server shutting down".into(),
                    }));
                    let _ = ws_sender.send(close).await;
                    break;
                }
            }
        }
    });

    // Receiver loop: drain incoming messages (server ignores client messages)
    while let Some(Ok(_msg)) = ws_receiver.next().await {
        // Keep the connection alive by reading frames
    }

    // Client disconnected — clean up
    sender.abort();
    state.ws.remove(conn_id);
}
