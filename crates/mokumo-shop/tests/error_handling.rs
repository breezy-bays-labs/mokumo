use std::path::PathBuf;

use mokumo_shop::startup::ensure_data_dirs;

mod common;

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
    let pool = mokumo_shop::db::initialize_database(&database_url)
        .await
        .unwrap();

    let recovery_dir = data_dir.join("recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();

    let shutdown_token = CancellationToken::new();
    let (app, _) = common::boot_router(
        data_dir,
        recovery_dir,
        pool.clone(),
        pool,
        kikan_types::SetupMode::Production,
        shutdown_token.clone(),
    )
    .await;

    // Bind to an ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let signal_token = shutdown_token.clone();

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
    signal_token.cancel();

    // Server should exit cleanly within a reasonable timeout
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), server_handle).await;
    assert!(result.is_ok(), "server should shut down within 5 seconds");
    result.unwrap().unwrap();
}

#[tokio::test]
async fn try_bind_short_circuits_on_non_addr_in_use_error() {
    // try_bind only iterates past an error when it's AddrInUse; any other
    // error (permissions, unassigned address, etc.) should short-circuit
    // on the first attempt instead of walking all 11 ports.
    //
    // Binding to 192.0.2.1 (RFC 5737 TEST-NET-1 — reserved for
    // documentation, not assignable to a real interface) produces
    // `AddrNotAvailable` regardless of uid, so the test behaves the same
    // for unprivileged users on CI and for root inside a container. The
    // previous variant used port 1 for an `EACCES`, which silently passed
    // as root.
    let result = mokumo_shop::startup::try_bind("192.0.2.1", 8080).await;
    assert!(result.is_err(), "should fail binding to unassignable addr");

    let err = result.unwrap_err();
    assert_ne!(
        err.kind(),
        std::io::ErrorKind::AddrInUse,
        "reserved-range bind should not yield AddrInUse; got: {err}"
    );
    let err_msg = err.to_string();
    assert!(
        !err_msg.contains("All ports"),
        "should short-circuit instead of exhausting the port range. Got: {err_msg}"
    );
}
