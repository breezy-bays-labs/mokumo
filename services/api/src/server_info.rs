use axum::{Json, extract::State};
use mokumo_types::ServerInfoResponse;

use crate::SharedState;

pub async fn handler(State(state): State<SharedState>) -> Json<ServerInfoResponse> {
    let status = state.mdns_status.read().expect("MdnsStatus lock poisoned");

    let lan_url = if status.active {
        status
            .hostname
            .as_ref()
            .map(|h| format!("http://{}:{}", h, status.port))
    } else {
        None
    };

    let ip_url = match local_ip_address::local_ip() {
        Ok(ip) => format!("http://{}:{}", ip, status.port),
        Err(e) => {
            tracing::warn!("Failed to detect LAN IP: {e}, falling back to loopback");
            format!("http://127.0.0.1:{}", status.port)
        }
    };

    let host = status
        .hostname
        .clone()
        .unwrap_or_else(|| "localhost".to_string());

    Json(ServerInfoResponse {
        lan_url,
        ip_url,
        mdns_active: status.active,
        host,
        port: status.port,
    })
}
