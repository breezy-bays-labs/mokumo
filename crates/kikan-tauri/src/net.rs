//! Tauri-shell-specific networking helpers.
//!
//! The bind strategy here ("ephemeral loopback for desktop webview") is specific
//! to the Tauri shell — it does NOT belong in `kikan::net`, which is
//! platform-neutral (I2 adapter-boundary hygiene).

/// Bind an ephemeral loopback port for the desktop shell.
///
/// Binds `"127.0.0.1:0"` — the OS picks a free port. Returns `SocketAddr`
/// (not bare `u16`) so callers can format `"http://{addr}"` directly via
/// `SocketAddr`'s `Display` impl.
///
/// Per `adr-kikan-binary-topology §7`: desktop binds `:0` (ephemeral, never
/// a fixed port); `mokumo-server` keeps the 6565–6575 range via
/// [`kikan::data_plane::bind::try_bind`].
pub async fn try_bind_ephemeral_loopback()
-> Result<(tokio::net::TcpListener, std::net::SocketAddr), std::io::Error> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tracing::info!("Ephemeral loopback: listening on {addr}");
    Ok((listener, addr))
}
