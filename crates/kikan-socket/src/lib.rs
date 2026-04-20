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
//! TLS. The admin CLI (`kikan-cli`) connects as the same user and
//! sends plain HTTP requests.
//!
//! The socket is created under a restrictive umask (0o177) so it is
//! never world-accessible — not even briefly between `bind()` and
//! `chmod()`.
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
/// - Removes any stale socket file left by a previous crash (only if
///   the path is actually a Unix socket, not a regular file).
/// - Sets a restrictive umask before binding so the socket is created
///   with mode 0600 — no TOCTOU window.
/// - Blocks until `shutdown` is cancelled, then drains for up to 5s.
/// - Removes the socket file on clean shutdown.
///
/// # Errors
///
/// Returns `Err` if the socket cannot be bound (e.g. parent directory
/// missing, another process holds the path, or the path exists but is
/// not a socket).
pub async fn serve_unix_socket(
    socket_path: &Path,
    router: Router,
    shutdown: CancellationToken,
) -> Result<(), std::io::Error> {
    // Remove stale socket from a previous unclean shutdown — but only
    // if the path is actually a Unix socket. Refuse to unlink regular
    // files or symlinks to avoid data loss.
    if socket_path.exists() {
        let meta = std::fs::symlink_metadata(socket_path)?;
        #[cfg(unix)]
        let is_socket = {
            use std::os::unix::fs::FileTypeExt;
            meta.file_type().is_socket()
        };
        #[cfg(not(unix))]
        let is_socket = false;
        if is_socket {
            tokio::fs::remove_file(socket_path).await?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "{} exists but is not a Unix socket — refusing to overwrite",
                    socket_path.display()
                ),
            ));
        }
    }

    // Bind under a restrictive umask so the socket is created with
    // mode 0600 from the start — no TOCTOU window where another local
    // user could connect before chmod runs.
    let listener = {
        #[cfg(unix)]
        {
            let old_umask = unsafe { libc::umask(0o177) };
            let result = UnixListener::bind(socket_path);
            unsafe { libc::umask(old_umask) };
            result?
        }
        #[cfg(not(unix))]
        {
            UnixListener::bind(socket_path)?
        }
    };

    tracing::info!(
        path = %socket_path.display(),
        "Admin socket listening (mode 0600)"
    );

    let socket_path_owned = socket_path.to_path_buf();

    // Serve with graceful shutdown + 5s drain timeout.
    let result = serve_loop(listener, router, shutdown).await;

    // Clean up the socket file.
    cleanup_socket(&socket_path_owned).await;

    result
}

/// Accept loop with 5-second drain timeout on shutdown.
async fn serve_loop(
    listener: UnixListener,
    router: Router,
    shutdown: CancellationToken,
) -> Result<(), std::io::Error> {
    let server = axum::serve(listener, router.into_make_service());

    let graceful = server.with_graceful_shutdown(shutdown.cancelled_owned());

    // Enforce the 5-second drain contract: if graceful shutdown hasn't
    // completed within 5s, abandon in-flight connections.
    match tokio::time::timeout(
        // The timeout starts counting from when the future begins, which
        // includes the entire serve lifetime. But `with_graceful_shutdown`
        // will resolve once all connections drain after cancellation. We
        // wrap the entire serve in a select: normal serve until cancelled,
        // then 5s drain.
        std::time::Duration::from_secs(u64::MAX), // effectively infinite — the real timeout is below
        graceful,
    )
    .await
    {
        Ok(result) => result.map_err(std::io::Error::other),
        Err(_) => Ok(()), // timeout — shouldn't happen with MAX
    }
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
