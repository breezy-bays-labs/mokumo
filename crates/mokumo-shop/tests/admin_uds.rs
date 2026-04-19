//! Integration tests for the admin UDS router.
//!
//! Exercises control plane handlers via the Unix socket path,
//! satisfying #508 acceptance criterion 5.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serial_test::serial;
use tokio::net::UnixStream;
use tokio_util::sync::CancellationToken;

/// Build a minimal `PlatformState` for testing with in-memory databases.
async fn test_platform_state(data_dir: &Path) -> kikan::PlatformState {
    let demo_db = kikan::db::initialize_database("sqlite::memory:")
        .await
        .unwrap();
    let production_db = kikan::db::initialize_database("sqlite::memory:")
        .await
        .unwrap();

    kikan::PlatformState {
        data_dir: data_dir.to_path_buf(),
        demo_db,
        production_db,
        active_profile: Arc::new(parking_lot::RwLock::new(kikan::SetupMode::Demo)),
        shutdown: CancellationToken::new(),
        started_at: std::time::Instant::now(),
        mdns_status: kikan::MdnsStatus::shared(),
        demo_install_ok: Arc::new(AtomicBool::new(true)),
        is_first_launch: Arc::new(AtomicBool::new(false)),
        setup_completed: Arc::new(AtomicBool::new(false)),
        profile_db_initializer: Arc::new(NoOpInit),
    }
}

struct NoOpInit;
impl kikan::platform_state::ProfileDbInitializer for NoOpInit {
    fn initialize<'a>(
        &'a self,
        _url: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<sea_orm::DatabaseConnection, kikan::db::DatabaseSetupError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(kikan::db::DatabaseSetupError::Migration(
                sea_orm::DbErr::Custom("noop".into()),
            ))
        })
    }
}

/// Poll the socket until connect succeeds or 2s timeout expires.
async fn wait_for_socket(socket_path: &Path) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match UnixStream::connect(socket_path).await {
                Ok(_) => return,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .expect("admin socket did not become ready within 2s");
}

/// Helper: send a GET request over a Unix socket and return (status, body).
async fn uds_get(socket_path: &Path, path: &str) -> (u16, Vec<u8>) {
    uds_request(socket_path, hyper::Method::GET, path, None).await
}

/// Helper: send a POST request with JSON body over a Unix socket.
async fn uds_post(socket_path: &Path, path: &str, body: &[u8]) -> (u16, Vec<u8>) {
    uds_request(socket_path, hyper::Method::POST, path, Some(body)).await
}

/// Internal: send an HTTP request over a Unix socket and return (status, body).
async fn uds_request(
    socket_path: &Path,
    method: hyper::Method,
    path: &str,
    body: Option<&[u8]>,
) -> (u16, Vec<u8>) {
    let stream = UnixStream::connect(socket_path).await.unwrap();
    let io = hyper_util::rt::TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();

    tokio::spawn(async move {
        let _ = conn.await;
    });

    let req = match body {
        Some(data) => hyper::Request::builder()
            .method(method)
            .uri(path)
            .header(hyper::header::HOST, "localhost")
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(http_body_util::Full::new(bytes::Bytes::copy_from_slice(
                data,
            )))
            .unwrap(),
        None => hyper::Request::builder()
            .method(method)
            .uri(path)
            .header(hyper::header::HOST, "localhost")
            .body(http_body_util::Full::new(bytes::Bytes::new()))
            .unwrap(),
    };

    let resp = sender.send_request(req).await.unwrap();
    let status = resp.status().as_u16();

    use http_body_util::BodyExt;
    let body = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();

    (status, body)
}

#[tokio::test]
#[serial]
async fn admin_uds_health_returns_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");
    let shutdown = CancellationToken::new();

    let platform = test_platform_state(tmp.path()).await;
    let router = kikan::admin::build_admin_router(platform);

    let socket_path_clone = socket_path.clone();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        kikan_socket::serve_unix_socket(&socket_path_clone, router, shutdown_clone)
            .await
            .unwrap();
    });

    wait_for_socket(&socket_path).await;

    let (status, body) = uds_get(&socket_path, "/health").await;
    assert_eq!(status, 200);
    assert_eq!(body, b"ok");

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
}

#[tokio::test]
#[serial]
async fn admin_uds_diagnostics_returns_json() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");
    let shutdown = CancellationToken::new();

    let platform = test_platform_state(tmp.path()).await;
    let router = kikan::admin::build_admin_router(platform);

    let socket_path_clone = socket_path.clone();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        kikan_socket::serve_unix_socket(&socket_path_clone, router, shutdown_clone)
            .await
            .unwrap();
    });

    wait_for_socket(&socket_path).await;

    let (status, body) = uds_get(&socket_path, "/diagnostics").await;
    assert_eq!(status, 200);

    let diag: kikan_types::diagnostics::DiagnosticsResponse =
        serde_json::from_slice(&body).expect("valid diagnostics JSON");
    // CARGO_PKG_NAME is resolved at compile time from whichever crate
    // contains the `collect()` function (kikan), not the test binary.
    assert_eq!(diag.app.name, "kikan");
    assert!(!diag.os.family.is_empty());

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
}

#[tokio::test]
#[serial]
async fn admin_uds_socket_permissions_are_0600() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");
    let shutdown = CancellationToken::new();

    let platform = test_platform_state(tmp.path()).await;
    let router = kikan::admin::build_admin_router(platform);

    // Use a oneshot to propagate bind errors instead of panicking in the
    // spawned task (which would cause a misleading timeout in wait_for_socket).
    let (err_tx, mut err_rx) = tokio::sync::oneshot::channel::<String>();
    let socket_path_clone = socket_path.clone();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        if let Err(e) =
            kikan_socket::serve_unix_socket(&socket_path_clone, router, shutdown_clone).await
        {
            let _ = err_tx.send(format!("serve_unix_socket failed: {e}"));
        }
    });

    // Give the socket a moment to bind, then check for early errors.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    if let Ok(err_msg) = err_rx.try_recv() {
        panic!("{err_msg}");
    }

    wait_for_socket(&socket_path).await;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(&socket_path).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        // The restrictive umask (0o177) ensures the socket is created
        // with at most 0o600. On some platforms socket permission bits
        // are reported differently, but the umask guarantees no group/other
        // access bits are set.
        assert_eq!(
            mode & 0o077,
            0,
            "socket should have no group/other permissions, got {mode:#o}"
        );
    }

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
}

#[tokio::test]
#[serial]
async fn admin_uds_socket_cleaned_up_on_shutdown() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");
    let shutdown = CancellationToken::new();

    let platform = test_platform_state(tmp.path()).await;
    let router = kikan::admin::build_admin_router(platform);

    let socket_path_clone = socket_path.clone();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        kikan_socket::serve_unix_socket(&socket_path_clone, router, shutdown_clone)
            .await
            .unwrap();
    });

    wait_for_socket(&socket_path).await;
    assert!(socket_path.exists(), "socket should exist while serving");

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;

    assert!(
        !socket_path.exists(),
        "socket file should be cleaned up after shutdown"
    );
}

#[tokio::test]
#[serial]
async fn admin_uds_refuses_to_overwrite_regular_file() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");

    // Create a regular file at the socket path.
    std::fs::write(&socket_path, "not a socket").unwrap();

    let shutdown = CancellationToken::new();
    let platform = test_platform_state(tmp.path()).await;
    let router = kikan::admin::build_admin_router(platform);

    let result = kikan_socket::serve_unix_socket(&socket_path, router, shutdown).await;
    assert!(result.is_err(), "should refuse to overwrite a regular file");
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
}

#[tokio::test]
#[serial]
async fn admin_uds_profiles_list_returns_two_profiles() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");
    let shutdown = CancellationToken::new();

    let platform = test_platform_state(tmp.path()).await;
    let router = kikan::admin::build_admin_router(platform);

    let socket_path_clone = socket_path.clone();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        kikan_socket::serve_unix_socket(&socket_path_clone, router, shutdown_clone)
            .await
            .unwrap();
    });

    wait_for_socket(&socket_path).await;

    let (status, body) = uds_get(&socket_path, "/profiles").await;
    assert_eq!(status, 200);

    let resp: kikan_types::admin::ProfileListResponse =
        serde_json::from_slice(&body).expect("valid profiles JSON");
    assert_eq!(resp.profiles.len(), 2);
    assert_eq!(resp.active, kikan_types::SetupMode::Demo);

    // Both profiles should be present.
    let names: Vec<_> = resp.profiles.iter().map(|p| p.name).collect();
    assert!(names.contains(&kikan_types::SetupMode::Production));
    assert!(names.contains(&kikan_types::SetupMode::Demo));

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
}

#[tokio::test]
#[serial]
async fn admin_uds_profiles_switch_changes_active() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");
    let shutdown = CancellationToken::new();

    let platform = test_platform_state(tmp.path()).await;
    let router = kikan::admin::build_admin_router(platform);

    let socket_path_clone = socket_path.clone();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        kikan_socket::serve_unix_socket(&socket_path_clone, router, shutdown_clone)
            .await
            .unwrap();
    });

    wait_for_socket(&socket_path).await;

    // Switch from Demo (default) to Production.
    let req = serde_json::json!({"profile": "production"});
    let (status, body) = uds_post(
        &socket_path,
        "/profiles/switch",
        &serde_json::to_vec(&req).unwrap(),
    )
    .await;
    assert_eq!(status, 200);

    let resp: kikan_types::admin::ProfileSwitchAdminResponse =
        serde_json::from_slice(&body).expect("valid switch response");
    assert_eq!(resp.previous, kikan_types::SetupMode::Demo);
    assert_eq!(resp.current, kikan_types::SetupMode::Production);

    // Verify the switch persisted via /profiles.
    let (status, body) = uds_get(&socket_path, "/profiles").await;
    assert_eq!(status, 200);
    let list: kikan_types::admin::ProfileListResponse =
        serde_json::from_slice(&body).expect("valid profiles JSON");
    assert_eq!(list.active, kikan_types::SetupMode::Production);

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
}

#[tokio::test]
#[serial]
async fn admin_uds_migrate_status_returns_json() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");
    let shutdown = CancellationToken::new();

    let platform = test_platform_state(tmp.path()).await;
    let router = kikan::admin::build_admin_router(platform);

    let socket_path_clone = socket_path.clone();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        kikan_socket::serve_unix_socket(&socket_path_clone, router, shutdown_clone)
            .await
            .unwrap();
    });

    wait_for_socket(&socket_path).await;

    let (status, body) = uds_get(&socket_path, "/migrate/status").await;
    assert_eq!(status, 200);

    let resp: kikan_types::admin::MigrationStatusResponse =
        serde_json::from_slice(&body).expect("valid migration status JSON");
    // In-memory databases won't have the kikan_migrations table,
    // so applied lists should be empty.
    assert!(resp.production.applied.is_empty());
    assert!(resp.demo.applied.is_empty());

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
}

#[tokio::test]
#[serial]
async fn admin_uds_backups_list_returns_empty_initially() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");
    let shutdown = CancellationToken::new();

    let platform = test_platform_state(tmp.path()).await;
    let router = kikan::admin::build_admin_router(platform);

    let socket_path_clone = socket_path.clone();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        kikan_socket::serve_unix_socket(&socket_path_clone, router, shutdown_clone)
            .await
            .unwrap();
    });

    wait_for_socket(&socket_path).await;

    let (status, body) = uds_get(&socket_path, "/backups").await;
    assert_eq!(status, 200);

    let resp: kikan_types::BackupStatusResponse =
        serde_json::from_slice(&body).expect("valid backups JSON");
    assert!(resp.production.backups.is_empty());
    assert!(resp.demo.backups.is_empty());

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
}

#[tokio::test]
#[serial]
async fn admin_uds_profiles_switch_invalid_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");
    let shutdown = CancellationToken::new();

    let platform = test_platform_state(tmp.path()).await;
    let router = kikan::admin::build_admin_router(platform);

    let socket_path_clone = socket_path.clone();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        kikan_socket::serve_unix_socket(&socket_path_clone, router, shutdown_clone)
            .await
            .unwrap();
    });

    wait_for_socket(&socket_path).await;

    // Send an invalid profile name — should get a 422 deserialization error.
    let req = serde_json::json!({"profile": "nonexistent"});
    let (status, _body) = uds_post(
        &socket_path,
        "/profiles/switch",
        &serde_json::to_vec(&req).unwrap(),
    )
    .await;
    assert!(
        status == 400 || status == 422,
        "expected 4xx for invalid profile, got {status}"
    );

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
}

#[tokio::test]
#[serial]
async fn admin_uds_backups_create_produces_file() {
    let tmp = tempfile::tempdir().unwrap();
    let socket_path = tmp.path().join("admin.sock");
    let shutdown = CancellationToken::new();

    // Create a real on-disk SQLite DB so create_backup has something to copy.
    let demo_dir = tmp.path().join("demo");
    std::fs::create_dir_all(&demo_dir).unwrap();
    let demo_db_path = demo_dir.join("mokumo.db");
    {
        let conn = rusqlite::Connection::open(&demo_db_path).unwrap();
        conn.execute_batch("CREATE TABLE test (id INTEGER PRIMARY KEY)")
            .unwrap();
    }

    // Use the on-disk DB for demo, in-memory for production.
    let demo_db = kikan::db::initialize_database(&format!("sqlite:{}", demo_db_path.display()))
        .await
        .unwrap();
    let production_db = kikan::db::initialize_database("sqlite::memory:")
        .await
        .unwrap();

    let platform = kikan::PlatformState {
        data_dir: tmp.path().to_path_buf(),
        demo_db,
        production_db,
        active_profile: std::sync::Arc::new(parking_lot::RwLock::new(kikan::SetupMode::Demo)),
        shutdown: CancellationToken::new(),
        started_at: std::time::Instant::now(),
        mdns_status: kikan::MdnsStatus::shared(),
        demo_install_ok: std::sync::Arc::new(AtomicBool::new(true)),
        is_first_launch: std::sync::Arc::new(AtomicBool::new(false)),
        setup_completed: std::sync::Arc::new(AtomicBool::new(false)),
        profile_db_initializer: std::sync::Arc::new(NoOpInit),
    };
    let router = kikan::admin::build_admin_router(platform);

    let socket_path_clone = socket_path.clone();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        kikan_socket::serve_unix_socket(&socket_path_clone, router, shutdown_clone)
            .await
            .unwrap();
    });

    wait_for_socket(&socket_path).await;

    // Create a backup of the demo profile.
    let req = serde_json::json!({"profile": "demo"});
    let (status, body) = uds_post(
        &socket_path,
        "/backups/create",
        &serde_json::to_vec(&req).unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "backup create failed: {}",
        String::from_utf8_lossy(&body)
    );

    let resp: kikan_types::admin::BackupCreatedResponse =
        serde_json::from_slice(&body).expect("valid backup create response");
    assert!(!resp.path.is_empty());
    assert!(resp.size > 0);
    assert_eq!(resp.profile, kikan_types::SetupMode::Demo);

    // Verify the backup file exists on disk.
    let backup_path = std::path::Path::new(&resp.path);
    assert!(
        backup_path.exists(),
        "backup file should exist at {}",
        resp.path
    );
    assert_eq!(
        std::fs::metadata(backup_path).unwrap().len(),
        resp.size,
        "backup file size should match response"
    );

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
}
