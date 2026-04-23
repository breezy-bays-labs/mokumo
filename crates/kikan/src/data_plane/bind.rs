//! TCP port-range bind helper for the data plane.
//!
//! Tries the requested port and the ten ports above it, returning the
//! first successful bind. Other bind errors fail fast — only `AddrInUse`
//! triggers a fall-through to the next port.
//!
//! Per `adr-kikan-binary-topology §7`, the headless `mokumo-server`
//! reserves the range `6565..=6575` for its data-plane HTTP listener.
//! The Tauri desktop shell uses [`kikan_tauri::try_bind_ephemeral_loopback`]
//! instead — that helper binds `127.0.0.1:0` and is shell-specific (I2).

use tokio::net::TcpListener;

/// Bind a TCP listener, walking ports `port..=port+10` until one succeeds.
///
/// Returns the bound listener together with the port that actually took.
/// `AddrInUse` on a candidate is logged at debug and the loop advances;
/// any other I/O error short-circuits and is returned to the caller with
/// the offending host:port included in the message. Exhausting the range
/// returns `AddrInUse` with operator-facing remediation guidance.
pub async fn try_bind(host: &str, port: u16) -> Result<(TcpListener, u16), std::io::Error> {
    let end_port = port.saturating_add(10);
    for p in port..=end_port {
        let addr = format!("{host}:{p}");
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                let actual_port = listener.local_addr()?.port();
                tracing::info!("Listening on {host}:{actual_port}");
                return Ok((listener, actual_port));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                tracing::debug!("Port {p} in use, trying next");
            }
            Err(e) => {
                return Err(std::io::Error::new(
                    e.kind(),
                    format!("Cannot bind to {host}:{p}: {e}"),
                ));
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        format!(
            "All ports {port}-{end_port} are occupied. \
             Use --port to specify a different port, or close conflicting applications."
        ),
    ))
}
