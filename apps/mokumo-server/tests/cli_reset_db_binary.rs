//! Binary-level integration tests for `mokumo-server reset-db`.
//!
//! Tests profile targeting, flock contention (blocks while server is running),
//! and idempotent reset behavior.
//!
//! Migrated from `services/api/tests/cli_reset_db_binary.rs` as part of
//! #512 (services/api main.rs deletion — CARGO_BIN_EXE_mokumo-api no longer exists).

use std::fs;
use std::io::{BufRead, BufReader};
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

/// Strip ANSI escape sequences from a string.
///
/// Handles all CSI sequences per ECMA-48: terminates on any character in the
/// range 0x40–0x7E, not just 'm'. This covers cursor movement, clear-screen,
/// and other codes that tracing-subscriber may emit on unusual terminals.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            // End of CSI sequence: any byte in 0x40–0x7E (ECMA-48 §5.4)
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

/// Parse the bound port from a tracing log line like:
///   `2026-03-28T00:00:00.000Z  INFO mokumo_api: Listening on 127.0.0.1:12345`
fn parse_port_from_log(line: &str) -> Option<u16> {
    let clean = strip_ansi(line);
    if !clean.contains("Listening on") {
        return None;
    }
    // The port is the last colon-separated segment
    let port_str = clean.rsplit(':').next()?;
    port_str.trim().parse().ok()
}

/// Wait for the server to become healthy by polling `/api/health`.
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

#[tokio::test]
async fn reset_db_blocked_by_running_server() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");

    // Set up a temp data directory with the required layout
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_path_buf();

    // Create the directory structure the server expects
    mokumo_api::ensure_data_dirs(&data_dir).unwrap();

    // Initialize a real SQLite database so the server can start.
    // The server uses the profile-based path (demo/mokumo.db by default).
    let profile_db_path = data_dir.join("demo").join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", profile_db_path.display());
    let db = mokumo_shop::db::initialize_database(&database_url)
        .await
        .unwrap();
    db.close().await.ok();

    // Spawn the server with --port 0 for an OS-assigned port
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

    // Read both stdout and stderr to find the bound port.
    // tracing-subscriber writes to stdout by default.
    let (port_tx, port_rx) = std::sync::mpsc::channel();

    let port_tx_clone = port_tx.clone();
    let stdout_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut lines = Vec::new();
        for line in reader.lines().map_while(Result::ok) {
            if let Some(port) = parse_port_from_log(&line) {
                let _ = port_tx_clone.send(port);
            }
            lines.push(line);
        }
        lines
    });

    let stderr_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut lines = Vec::new();
        for line in reader.lines().map_while(Result::ok) {
            if let Some(port) = parse_port_from_log(&line) {
                let _ = port_tx.send(port);
            }
            lines.push(line);
        }
        lines
    });

    // Wait for the server to report its port (up to 30s for cold start + migrations)
    let port = match port_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(p) => p,
        Err(_) => {
            drop(guard);
            let stderr_lines = stderr_thread.join().unwrap_or_default();
            let stdout_lines = stdout_thread.join().unwrap_or_default();
            panic!(
                "server did not report its port within 30s.\n\
                 stdout ({} lines):\n{}\n\
                 stderr ({} lines):\n{}",
                stdout_lines.len(),
                stdout_lines.join("\n"),
                stderr_lines.len(),
                stderr_lines.join("\n"),
            );
        }
    };

    // Wait for the health endpoint to respond
    wait_for_health(port, Duration::from_secs(10))
        .await
        .expect("server health check failed");

    // Point reset-db at the temp dir for recovery files so it never touches
    // the real Desktop or cwd, even if the flock guard regresses.
    let recovery_dir = data_dir.join("recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();

    // Now run reset-db against the same data directory — it should be blocked.
    // No --production flag: targets demo profile (the server's active profile).
    let reset_output = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "reset-db",
            "--force",
        ])
        .env("MOKUMO_RECOVERY_DIR", &recovery_dir)
        .output()
        .expect("failed to spawn reset-db");

    let reset_stderr = String::from_utf8_lossy(&reset_output.stderr);

    assert!(
        !reset_output.status.success(),
        "reset-db should have failed with exit code 1, but succeeded. stderr: {reset_stderr}"
    );

    assert!(
        reset_stderr.contains("Cannot reset database while the server is running"),
        "stderr should contain the flock rejection message, got: {reset_stderr}"
    );

    // Verify the profile database still exists (reset-db didn't delete anything)
    assert!(
        profile_db_path.exists(),
        "profile database file should still exist after blocked reset-db"
    );

    // Kill server (ServerGuard handles this on drop) and join reader threads
    drop(guard);
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
}

// ---------------------------------------------------------------------------
// Profile-aware reset-db tests
// ---------------------------------------------------------------------------

#[test]
fn reset_db_default_targets_demo_profile() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();

    // Set up profile structure: demo/ and production/ subdirectories
    mokumo_api::ensure_data_dirs(data_dir).unwrap();

    // Seed both profiles so the isolation assertion is meaningful
    let demo_db = data_dir.join("demo").join("mokumo.db");
    let production_db = data_dir.join("production").join("mokumo.db");
    fs::write(&demo_db, b"demo-data").unwrap();
    fs::write(&production_db, b"production-data").unwrap();

    let recovery_dir = data_dir.join("recovery");
    fs::create_dir_all(&recovery_dir).unwrap();

    // reset-db without --production should target demo/ by default
    let output = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "reset-db",
            "--force",
        ])
        .env("MOKUMO_RECOVERY_DIR", &recovery_dir)
        .output()
        .expect("failed to spawn reset-db");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reset-db should succeed, got exit {}: {stderr}",
        output.status
    );
    assert!(!demo_db.exists(), "demo/mokumo.db should have been deleted");
    assert!(
        production_db.exists(),
        "production/mokumo.db should not have been deleted"
    );
}

#[test]
fn reset_db_production_flag_targets_production_profile() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();

    mokumo_api::ensure_data_dirs(data_dir).unwrap();

    // Seed both profiles so the isolation assertion is meaningful
    let demo_db = data_dir.join("demo").join("mokumo.db");
    let production_db = data_dir.join("production").join("mokumo.db");
    fs::write(&production_db, b"production-data").unwrap();
    fs::write(&demo_db, b"demo-data").unwrap();

    let recovery_dir = data_dir.join("recovery");
    fs::create_dir_all(&recovery_dir).unwrap();

    // reset-db with --production should target production/
    let output = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "reset-db",
            "--force",
            "--production",
        ])
        .env("MOKUMO_RECOVERY_DIR", &recovery_dir)
        .output()
        .expect("failed to spawn reset-db");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reset-db --production should succeed, got exit {}: {stderr}",
        output.status
    );
    assert!(
        !production_db.exists(),
        "production/mokumo.db should have been deleted"
    );
    assert!(
        demo_db.exists(),
        "demo/mokumo.db should not have been deleted"
    );
}

#[test]
fn reset_db_no_db_found_when_neither_profile_exists() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();

    mokumo_api::ensure_data_dirs(data_dir).unwrap();
    // No databases created in either profile

    let recovery_dir = data_dir.join("recovery");
    fs::create_dir_all(&recovery_dir).unwrap();

    let output = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "reset-db",
            "--force",
        ])
        .env("MOKUMO_RECOVERY_DIR", &recovery_dir)
        .output()
        .expect("failed to spawn reset-db");

    // Should exit 0 (idempotent)
    assert!(
        output.status.success(),
        "reset-db with no database should exit 0, got exit {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No database found"),
        "stdout should mention 'No database found', got: {stdout}"
    );
}

/// Targeted profile is absent but the OTHER profile has a database.
/// Verifies the per-profile early-exit message and that the other DB is untouched.
#[test]
fn reset_db_demo_profile_absent_when_production_exists() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path();

    mokumo_api::ensure_data_dirs(data_dir).unwrap();

    // Only production DB exists — demo is absent
    let demo_db = data_dir.join("demo").join("mokumo.db");
    let production_db = data_dir.join("production").join("mokumo.db");
    fs::write(&production_db, b"production-data").unwrap();

    let recovery_dir = data_dir.join("recovery");
    fs::create_dir_all(&recovery_dir).unwrap();

    let output = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "reset-db",
            "--force",
        ])
        .env("MOKUMO_RECOVERY_DIR", &recovery_dir)
        .output()
        .expect("failed to spawn reset-db");

    assert!(
        output.status.success(),
        "reset-db should exit 0 when demo profile is absent, got exit {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No database found for the demo profile"),
        "stdout should report missing demo profile, got: {stdout}"
    );
    assert!(
        production_db.exists(),
        "production/mokumo.db should not have been deleted"
    );
    assert!(!demo_db.exists(), "demo/mokumo.db was never created");
}

/// `reset-db --production` is blocked by a running server (flock guard).
#[tokio::test]
async fn reset_db_production_blocked_by_running_server() {
    let binary = env!("CARGO_BIN_EXE_mokumo-server");

    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_path_buf();

    mokumo_api::ensure_data_dirs(&data_dir).unwrap();

    // Initialize both profile DBs. The server uses demo (default active profile).
    // Production must be a valid SQLite DB so prepare_database's non-active migration
    // step doesn't fail; and it must exist so reset-db --production passes the early-exit check.
    let demo_db_path = data_dir.join("demo").join("mokumo.db");
    let production_db_path = data_dir.join("production").join("mokumo.db");
    for db_url in [
        format!("sqlite:{}?mode=rwc", demo_db_path.display()),
        format!("sqlite:{}?mode=rwc", production_db_path.display()),
    ] {
        let db = mokumo_shop::db::initialize_database(&db_url).await.unwrap();
        db.close().await.ok();
    }

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

    let port = match port_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(p) => p,
        Err(_) => {
            drop(guard);
            panic!("server did not report its port within 30s");
        }
    };
    wait_for_health(port, Duration::from_secs(10))
        .await
        .expect("server health check failed");

    let recovery_dir = data_dir.join("recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();

    // reset-db --production should be blocked by the same flock as demo
    let reset_output = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "reset-db",
            "--force",
            "--production",
        ])
        .env("MOKUMO_RECOVERY_DIR", &recovery_dir)
        .output()
        .expect("failed to spawn reset-db --production");

    let reset_stderr = String::from_utf8_lossy(&reset_output.stderr);

    assert!(
        !reset_output.status.success(),
        "reset-db --production should have been blocked, but succeeded. stderr: {reset_stderr}"
    );
    assert!(
        reset_stderr.contains("Cannot reset database while the server is running"),
        "stderr should contain the flock rejection message, got: {reset_stderr}"
    );
    assert!(
        production_db_path.exists(),
        "production/mokumo.db should still exist after blocked reset"
    );

    drop(guard);
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
}

#[test]
fn parse_port_from_tracing_line() {
    assert_eq!(
        parse_port_from_log(
            "2026-03-28T00:00:00.000Z  INFO mokumo_api: Listening on 127.0.0.1:12345"
        ),
        Some(12345)
    );
    assert_eq!(
        parse_port_from_log("  INFO mokumo_api: Listening on 0.0.0.0:6565"),
        Some(6565)
    );
    assert_eq!(parse_port_from_log("INFO some other log line"), None);
    // ANSI-encoded line (as produced by tracing with colors)
    assert_eq!(
        parse_port_from_log(
            "\x1b[2m2026-03-28T00:00:00Z\x1b[0m \x1b[32m INFO\x1b[0m \x1b[2mmokumo_api\x1b[0m\x1b[2m:\x1b[0m Listening on 127.0.0.1:53578"
        ),
        Some(53578)
    );
}
