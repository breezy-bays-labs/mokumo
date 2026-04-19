//! Binary-level integration tests for `mokumo-server backup create` and `mokumo-server restore`.
//!
//! Tests the actual binary CLI parsing and output formatting. Also verifies
//! that `restore` is blocked by a running server's flock guard.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// RAII guard that kills the server child process on drop.
struct ServerGuard {
    child: Child,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii() && (0x40u8..=0x7Eu8).contains(&(c as u8)) {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            result.push(c);
        }
    }
    result
}

fn parse_port_from_log(line: &str) -> Option<u16> {
    let clean = strip_ansi(line);
    if !clean.contains("Listening on") {
        return None;
    }
    let port_str = clean.rsplit(':').next()?;
    port_str.trim().parse().ok()
}

async fn wait_for_health(port: u16, timeout: Duration) -> Result<(), String> {
    let start = Instant::now();
    let url = format!("http://127.0.0.1:{port}/api/health");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();

    loop {
        if start.elapsed() > timeout {
            return Err(format!("Server did not become healthy within {timeout:?}"));
        }

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

fn create_test_db(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT);
         INSERT INTO test (name) VALUES ('alice');",
    )
    .unwrap();
}

// ── backup binary tests ───────────────────────────────────────────────────

#[test]
fn backup_binary_prints_path_and_size() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();

    mokumo_shop::startup::ensure_data_dirs(data_dir).unwrap();
    let db_path = data_dir.join("demo").join("mokumo.db");
    create_test_db(&db_path);

    let output = Command::new(binary)
        .args(["--data-dir", data_dir.to_str().unwrap(), "backup", "create"])
        .output()
        .expect("failed to spawn backup");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "backup should succeed, got exit {}: stderr={stderr}",
        output.status
    );
    assert!(
        stdout.contains("Backup created:"),
        "stdout should contain backup path, got: {stdout}"
    );
    assert!(
        stdout.contains("Size:"),
        "stdout should contain size, got: {stdout}"
    );
}

#[test]
fn backup_binary_with_custom_output() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();
    let output_path = tmp.path().join("custom-backup.db");

    mokumo_shop::startup::ensure_data_dirs(data_dir).unwrap();
    let db_path = data_dir.join("demo").join("mokumo.db");
    create_test_db(&db_path);

    let output = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "backup",
            "create",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to spawn backup");

    assert!(output.status.success());
    assert!(output_path.exists(), "custom output file should exist");
}

#[test]
fn backup_binary_fails_for_missing_db() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();

    mokumo_shop::startup::ensure_data_dirs(data_dir).unwrap();
    // No database created

    let output = Command::new(binary)
        .args(["--data-dir", data_dir.to_str().unwrap(), "backup", "create"])
        .output()
        .expect("failed to spawn backup");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No database found"),
        "stderr should mention missing database, got: {stderr}"
    );
}

// ── restore binary tests ──────────────────────────────────────────────────

#[test]
fn restore_binary_prints_confirmation() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();

    mokumo_shop::startup::ensure_data_dirs(data_dir).unwrap();
    let db_path = data_dir.join("demo").join("mokumo.db");
    create_test_db(&db_path);

    // Create a backup to restore from
    let backup_path = tmp.path().join("backup.db");
    create_test_db(&backup_path);

    let output = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "restore",
            backup_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to spawn restore");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "restore should succeed, got exit {}: stderr={stderr}",
        output.status
    );
    assert!(
        stdout.contains("Restored from:"),
        "stdout should contain restore confirmation, got: {stdout}"
    );
    assert!(
        stdout.contains("Restore complete"),
        "stdout should contain completion message, got: {stdout}"
    );
}

#[tokio::test]
async fn restore_blocked_by_running_server() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_path_buf();

    mokumo_shop::startup::ensure_data_dirs(&data_dir).unwrap();

    // Initialize a real database so the server can start
    let db_path = data_dir.join("demo").join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = mokumo_shop::db::initialize_database(&database_url)
        .await
        .unwrap();
    db.close().await.ok();

    // Create a backup file to try restoring
    let backup_path = data_dir.join("backup.db");
    create_test_db(&backup_path);

    // Spawn the server
    let mut server_proc = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "serve",
            "--port",
            "0",
            "--mode",
            "loopback",
        ])
        .env("RUST_LOG", "info")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn server");

    let stderr = server_proc.stderr.take().expect("stderr not captured");
    let stdout = server_proc.stdout.take().expect("stdout not captured");
    let guard = ServerGuard { child: server_proc };

    let (port_tx, port_rx) = std::sync::mpsc::channel();
    let port_tx_clone = port_tx.clone();
    let stdout_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(port) = parse_port_from_log(&line) {
                let _ = port_tx_clone.send(port);
            }
        }
    });
    let stderr_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(port) = parse_port_from_log(&line) {
                let _ = port_tx.send(port);
            }
        }
    });

    let port = port_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("server did not report its port within 30s");

    wait_for_health(port, Duration::from_secs(10))
        .await
        .expect("server health check failed");

    // Try restore while server is running — should be blocked
    let restore_output = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "restore",
            backup_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to spawn restore");

    let restore_stderr = String::from_utf8_lossy(&restore_output.stderr);

    assert!(
        !restore_output.status.success(),
        "restore should have failed, but succeeded. stderr: {restore_stderr}"
    );
    assert!(
        restore_stderr.contains("in use by a running server"),
        "stderr should contain the flock rejection message, got: {restore_stderr}"
    );

    drop(guard);
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
}
