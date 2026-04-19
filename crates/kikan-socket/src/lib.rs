//! Unix domain socket (UDS) serving primitives for the kikan admin surface.
//!
//! The headless binary (`mokumo-server serve`) binds a Unix socket at
//! `{data_dir}/admin.sock` with mode 0600 so only the owning user can
//! send admin commands. An Axum router (the "admin router") is served
//! over this socket — same HTTP semantics, different transport.
//!
//! ## Security model
//!
//! File-system permissions ARE the auth layer. The socket is mode 0600
//! (owner read+write only). No session cookies, no bearer tokens, no
//! TLS. The admin CLI (`kikan-admin-cli`) connects as the same user and
//! sends plain HTTP requests.
//!
//! ## Graceful shutdown
//!
//! `serve_unix_socket` accepts a `CancellationToken`. When cancelled the
//! listener stops accepting new connections. In-flight requests are
//! drained for up to 5 seconds before forced termination. The socket
//! file is removed on clean shutdown.

use std::path::{Path, PathBuf};

use axum::Router;
use tokio::net::UnixListener;
use tokio_util::sync::CancellationToken;

/// Serve `router` over a Unix domain socket at `socket_path`.
///
/// - Creates the socket file and sets permissions to 0600.
/// - Removes any stale socket file left by a previous crash.
/// - Blocks until `shutdown` is cancelled, then drains for up to 5s.
/// - Removes the socket file on clean shutdown.
///
/// # Errors
///
/// Returns `Err` if the socket cannot be bound (e.g. parent directory
/// missing, or another process holds the path).
pub async fn serve_unix_socket(
    socket_path: &Path,
    router: Router,
    shutdown: CancellationToken,
) -> Result<(), std::io::Error> {
    // Remove stale socket from a previous unclean shutdown.
    if socket_path.exists() {
        tokio::fs::remove_file(socket_path).await?;
    }

    let listener = UnixListener::bind(socket_path)?;

    // Set permissions to 0600 (owner read+write only).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(socket_path, perms)?;
    }

    tracing::info!(
        path = %socket_path.display(),
        "Admin socket listening (mode 0600)"
    );

    let socket_path_owned = socket_path.to_path_buf();

    // Serve with graceful shutdown.
    let result = serve_loop(listener, router, shutdown).await;

    // Clean up the socket file.
    cleanup_socket(&socket_path_owned).await;

    result
}

/// Accept loop: converts Unix stream connections into Axum request/response cycles.
async fn serve_loop(
    listener: UnixListener,
    router: Router,
    shutdown: CancellationToken,
) -> Result<(), std::io::Error> {
    // axum::serve supports unix listeners directly since axum 0.8
    let server = axum::serve(listener, router.into_make_service());

    server
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await
        .map_err(std::io::Error::other)
}

/// Best-effort removal of the socket file.
async fn cleanup_socket(path: &PathBuf) {
    if let Err(e) = tokio::fs::remove_file(path).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            path = %path.display(),
            "Failed to remove admin socket on shutdown: {e}"
        );
    }
}

/// Canonical path for the admin socket within a data directory.
pub fn admin_socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("admin.sock")
}
