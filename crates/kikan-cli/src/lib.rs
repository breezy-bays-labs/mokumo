//! Admin CLI library — UDS HTTP client for kikan admin subcommands.
//!
//! Provides an HTTP client that connects to the admin Unix domain
//! socket and sends requests to the control-plane router. Used by
//! `mokumo-server` subcommands (garage Pattern 3) when the daemon is
//! running.
//!
//! ## Connection model
//!
//! `UdsClient` connects to `{data_dir}/admin.sock` and sends plain
//! HTTP/1.1 requests over the Unix stream. No TLS, no cookies — the
//! socket's fs-permissions (0600) are the auth layer.

use std::path::{Path, PathBuf};

use hyper::Request;

pub mod backup_cli;
pub mod diagnose;
pub mod format;
pub mod migrate;
pub mod profile;

/// Error type for admin CLI operations.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("daemon not running — socket {path} does not exist")]
    DaemonNotRunning { path: PathBuf },

    #[error("connection refused on {path}: {source}")]
    ConnectionRefused {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("HTTP error: {0}")]
    Http(#[from] hyper::Error),

    #[error("request failed with status {status}: {body}")]
    RequestFailed { status: u16, body: String },

    #[error("{0}")]
    Other(String),
}

impl CliError {
    /// Structured exit code for CLI error kinds.
    ///
    /// 0 = success (not represented here), 1 = general, 2 = usage (clap),
    /// 10+ = admin-specific.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::DaemonNotRunning { .. } => 10,
            Self::ConnectionRefused { .. } => 11,
            Self::Http(_) => 12,
            Self::RequestFailed { .. } => 13,
            Self::Other(_) => 1,
        }
    }
}

/// HTTP client that connects to the admin Unix domain socket.
pub struct UdsClient {
    socket_path: PathBuf,
}

impl UdsClient {
    /// Create a client targeting the given socket path.
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    /// Create a client targeting `{data_dir}/admin.sock`.
    pub fn for_data_dir(data_dir: &Path) -> Self {
        Self::new(data_dir.join("admin.sock"))
    }

    /// Check whether the daemon appears to be running (socket file exists).
    pub fn daemon_available(&self) -> bool {
        self.socket_path.exists()
    }

    /// Send a GET request to the given path and return the response body as bytes.
    pub async fn get(&self, path: &str) -> Result<Vec<u8>, CliError> {
        self.request(hyper::Method::GET, path, None).await
    }

    /// Send a POST request with a JSON-serializable body.
    ///
    /// Handles JSON encoding internally so callers pass a typed request
    /// struct rather than raw bytes.
    pub async fn post<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<Vec<u8>, CliError> {
        let encoded = serde_json::to_vec(body)
            .map_err(|e| CliError::Other(format!("JSON serialization failed: {e}")))?;
        self.request(hyper::Method::POST, path, Some(&encoded))
            .await
    }

    /// Internal: send an HTTP request over the Unix socket.
    async fn request(
        &self,
        method: hyper::Method,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<Vec<u8>, CliError> {
        if !self.daemon_available() {
            return Err(CliError::DaemonNotRunning {
                path: self.socket_path.clone(),
            });
        }

        let stream = tokio::net::UnixStream::connect(&self.socket_path)
            .await
            .map_err(|e| CliError::ConnectionRefused {
                path: self.socket_path.clone(),
                source: e,
            })?;

        let io = hyper_util::rt::TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(CliError::Http)?;

        // Drive the connection in the background.
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                tracing::debug!("UDS connection closed: {e}");
            }
        });

        let req = match body {
            Some(data) => Request::builder()
                .method(method)
                .uri(path)
                .header(hyper::header::HOST, "localhost")
                .header(hyper::header::CONTENT_TYPE, "application/json")
                .body(http_body_util::Full::new(bytes::Bytes::copy_from_slice(
                    data,
                )))
                .expect("valid request"),
            None => Request::builder()
                .method(method)
                .uri(path)
                .header(hyper::header::HOST, "localhost")
                .body(http_body_util::Full::new(bytes::Bytes::new()))
                .expect("valid request"),
        };

        let resp = sender.send_request(req).await.map_err(CliError::Http)?;
        let status = resp.status().as_u16();

        use http_body_util::BodyExt;
        let body_bytes = resp
            .into_body()
            .collect()
            .await
            .map_err(CliError::Http)?
            .to_bytes()
            .to_vec();

        if status >= 400 {
            return Err(CliError::RequestFailed {
                status,
                body: String::from_utf8_lossy(&body_bytes).into_owned(),
            });
        }

        Ok(body_bytes)
    }
}
