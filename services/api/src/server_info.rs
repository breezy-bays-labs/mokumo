use std::net::IpAddr;

use axum::{Json, extract::State};
use mokumo_types::ServerInfoResponse;

use crate::SharedState;

fn format_host(ip: &IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    }
}

pub async fn handler(State(state): State<SharedState>) -> Json<ServerInfoResponse> {
    let status = state.mdns_status.read().expect("MdnsStatus lock poisoned");
    let on_loopback = crate::discovery::is_loopback(&status.bind_host);

    let lan_url = if status.active {
        status
            .hostname
            .as_ref()
            .map(|h| format!("http://{}:{}", h, status.port))
    } else {
        None
    };

    let ip_url = if on_loopback {
        None
    } else {
        match local_ip_address::local_ip() {
            Ok(ip) => Some(format!("http://{}:{}", format_host(&ip), status.port)),
            Err(e) => {
                tracing::warn!("Failed to detect LAN IP: {e}");
                None
            }
        }
    };

    let host = status
        .hostname
        .clone()
        .unwrap_or_else(|| status.bind_host.clone());

    Json(ServerInfoResponse {
        lan_url,
        ip_url,
        mdns_active: status.active,
        host,
        port: status.port,
    })
}
