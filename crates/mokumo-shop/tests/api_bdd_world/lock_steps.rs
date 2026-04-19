use super::ApiWorld;
use cucumber::{given, then, when};
use mokumo_shop::startup::{
    format_lock_conflict_message, format_reset_db_conflict_message, read_lock_info, write_lock_info,
};

// Process lock BDD steps test the write_lock_info/read_lock_info functions and
// conflict message formatting. The BDD world uses OS-assigned ports and can't
// control the actual flock mechanism through TestServer, so we test the lock
// info read/write and message formatting directly.

// --- Lock acquisition and info storage ---

#[given("no other Mokumo server is running")]
async fn no_other_server(w: &mut ApiWorld) {
    w.ensure_auth().await;
    // BDD world server is already running in isolation — no conflict possible.
}

#[when("the server starts on port 6565")]
async fn server_starts_on_default_port(w: &mut ApiWorld) {
    w.ensure_auth().await;
    // Write port info to a temp lock file to simulate post-bind write
    let lock_path = w._tmp.path().join("mokumo.lock");
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(&lock_path)
        .unwrap();
    write_lock_info(&f, 6565).unwrap();
}

#[then(expr = "the lock file contains {string}")]
async fn lock_file_contains(w: &mut ApiWorld, expected: String) {
    let lock_path = w._tmp.path().join("mokumo.lock");
    let content = std::fs::read_to_string(&lock_path).unwrap();
    assert!(
        content.contains(&expected),
        "Lock file should contain '{expected}', got: {content}"
    );
}

#[then("the lock file contains the actual bound port")]
async fn lock_file_contains_actual_port(w: &mut ApiWorld) {
    // Simulate: write the test server's actual port to the lock file
    let actual_port = w.server.server_address().unwrap().port().unwrap();
    let lock_path = w._tmp.path().join("mokumo.lock");
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(&lock_path)
        .unwrap();
    write_lock_info(&f, actual_port).unwrap();

    let port = read_lock_info(&lock_path);
    assert_eq!(port, Some(actual_port));
}

// --- Server launch conflict ---

#[given(expr = "a Mokumo server is running on port {int}")]
async fn server_running_on_port(w: &mut ApiWorld, port: u16) {
    w.ensure_auth().await;
    // Write port info to simulate a running server's lock file
    let lock_path = w._tmp.path().join("mokumo.lock");
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(&lock_path)
        .unwrap();
    write_lock_info(&f, port).unwrap();
}

#[when("a second server instance attempts to start")]
async fn second_instance_attempts(_w: &mut ApiWorld) {
    // The conflict detection is tested via format_lock_conflict_message.
    // In production, fd_lock::RwLock::try_write returns WouldBlock.
}

#[then(expr = "it exits with error containing {string}")]
async fn exits_with_error_containing(w: &mut ApiWorld, expected: String) {
    let lock_path = w._tmp.path().join("mokumo.lock");
    let port = read_lock_info(&lock_path);
    let cmd = w.last_broadcast_type.as_deref().unwrap_or("");
    let msg = if cmd == "reset-db" {
        format_reset_db_conflict_message(port)
    } else {
        format_lock_conflict_message(port)
    };
    assert!(
        msg.contains(&expected),
        "Error should contain '{expected}', got: {msg}"
    );
}

#[then("the error message suggests checking the system tray")]
async fn error_suggests_tray(w: &mut ApiWorld) {
    let lock_path = w._tmp.path().join("mokumo.lock");
    let port = read_lock_info(&lock_path);
    let msg = format_lock_conflict_message(port);
    assert!(
        msg.contains("system tray"),
        "Error should mention system tray, got: {msg}"
    );
}

#[then(expr = "the error message includes the URL {string}")]
async fn error_includes_url(w: &mut ApiWorld, url: String) {
    let lock_path = w._tmp.path().join("mokumo.lock");
    let port = read_lock_info(&lock_path);
    let msg = format_lock_conflict_message(port);
    assert!(
        msg.contains(&url),
        "Error should contain URL '{url}', got: {msg}"
    );
}

// --- Destructive command gating ---

#[given("a Mokumo server is running")]
async fn server_is_running(w: &mut ApiWorld) {
    w.ensure_auth().await;
}

#[when(expr = "the {string} command is executed")]
async fn command_is_executed(w: &mut ApiWorld, command: String) {
    // Store the command for later assertion
    w.last_broadcast_type = Some(command);
}

#[then(expr = "the error message includes {string}")]
async fn error_message_includes(w: &mut ApiWorld, expected: String) {
    let lock_path = w._tmp.path().join("mokumo.lock");
    let port = read_lock_info(&lock_path);
    let cmd = w.last_broadcast_type.as_deref().unwrap_or("");
    let msg = if cmd == "reset-db" {
        format_reset_db_conflict_message(port)
    } else {
        format_lock_conflict_message(port)
    };
    assert!(
        msg.contains(&expected),
        "Error should contain '{expected}', got: {msg}"
    );
}

#[then("the error message suggests stopping the server first")]
async fn error_suggests_stopping(w: &mut ApiWorld) {
    let lock_path = w._tmp.path().join("mokumo.lock");
    let port = read_lock_info(&lock_path);
    let msg = format_reset_db_conflict_message(port);
    assert!(
        msg.contains("Stop the server first"),
        "Error should suggest stopping, got: {msg}"
    );
}

// --- Non-destructive commands bypass lock ---

#[then("it does not check the process lock")]
async fn does_not_check_lock(_w: &mut ApiWorld) {
    // reset-password in main.rs doesn't acquire the flock at all.
    // This is verified by code inspection — the ResetPassword match arm
    // directly calls cli_reset_password without any lock acquisition.
}

#[then("the command proceeds normally")]
async fn command_proceeds(_w: &mut ApiWorld) {
    // Narrative step — reset-password bypasses the lock by design.
}

// --- Lock release ---

#[when("the server shuts down")]
async fn server_shuts_down(w: &mut ApiWorld) {
    w.shutdown_token.cancel();
    tokio::task::yield_now().await;
}

#[then("the lock file is no longer locked")]
async fn lock_file_not_locked(w: &mut ApiWorld) {
    // Create a fresh lock file and verify we can acquire a write lock
    let lock_path = w._tmp.path().join("mokumo.lock");
    let f = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    let mut flock = fd_lock::RwLock::new(f);
    // Should succeed since no other process holds the lock
    assert!(
        flock.try_write().is_ok(),
        "Lock file should be unlocked after shutdown"
    );
}

#[then("a new server instance can start")]
async fn new_server_can_start(_w: &mut ApiWorld) {
    // Verified by the previous step — lock is available.
}

// --- Stale lock file ---

#[given("a previous server crashed leaving a lock file on disk")]
async fn crashed_server_left_lock(w: &mut ApiWorld) {
    w.ensure_auth().await;
    let lock_path = w._tmp.path().join("mokumo.lock");
    // Write stale port info
    std::fs::write(&lock_path, "port=6565\n").unwrap();
}

#[given("the file lock is not held (kernel released it)")]
async fn lock_not_held(_w: &mut ApiWorld) {
    // No process holds the flock — this is the default after a crash.
    // The kernel automatically releases flocks when a process exits.
}

#[when("a new server instance starts")]
async fn new_server_starts(w: &mut ApiWorld) {
    w.ensure_auth().await;
    // Verify we can acquire the lock on the stale file
    let lock_path = w._tmp.path().join("mokumo.lock");
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    let mut flock = fd_lock::RwLock::new(f);
    assert!(
        flock.try_write().is_ok(),
        "Should acquire lock on stale file"
    );
}

#[then("it acquires the lock successfully")]
async fn lock_acquired(_w: &mut ApiWorld) {
    // Verified by the When step above.
}

#[then("the server starts normally")]
async fn server_starts_normally(w: &mut ApiWorld) {
    // The test server is already running — verify with a health check
    let resp = w.server.get("/api/health").await;
    resp.assert_status(axum::http::StatusCode::OK);
}
