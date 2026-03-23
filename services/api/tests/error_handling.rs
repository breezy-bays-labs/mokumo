use std::path::PathBuf;

use mokumo_api::ensure_data_dirs;

#[test]
fn ensure_data_dirs_returns_error_with_path_on_read_only_directory() {
    // Use a path that cannot be created — /proc is read-only on Linux,
    // and a non-existent root path works on macOS
    let impossible_path = if cfg!(target_os = "macos") {
        PathBuf::from("/System/Volumes/impossible_test_dir")
    } else {
        PathBuf::from("/proc/impossible_test_dir")
    };

    let result = ensure_data_dirs(&impossible_path);
    assert!(result.is_err(), "should fail on read-only path");

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains(&impossible_path.display().to_string()),
        "error should contain the path, got: {err_msg}"
    );
}

#[tokio::test]
async fn graceful_shutdown_completes_cleanly() {
    use tokio_util::sync::CancellationToken;

    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("shutdown_test");
    ensure_data_dirs(&data_dir).unwrap();

    let db_path = data_dir.join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = mokumo_db::initialize_database(&database_url).await.unwrap();

    let config = mokumo_api::ServerConfig {
        port: 0,
        host: "127.0.0.1".into(),
        data_dir,
    };

    let app = mokumo_api::build_app(&config, pool);

    // Bind to an ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let cancel_token = CancellationToken::new();
    let shutdown_token = cancel_token.clone();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown_token.cancelled().await;
            })
            .await
            .unwrap();
    });

    // Send a health check to verify server is running
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/api/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Signal shutdown
    cancel_token.cancel();

    // Server should exit cleanly within a reasonable timeout
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), server_handle).await;
    assert!(result.is_ok(), "server should shut down within 5 seconds");
    result.unwrap().unwrap();
}
