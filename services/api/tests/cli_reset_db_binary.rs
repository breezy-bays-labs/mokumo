//! Binary-level integration test: `mokumo-api reset-db` vs a running server.
//!
//! Spawns the real `mokumo-api` binary as a server subprocess, then runs
//! `mokumo-api reset-db --force` against the same data directory and asserts
//! that the flock guard rejects the reset.

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
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until 'm' (end of ANSI escape sequence)
            for esc_c in chars.by_ref() {
                if esc_c == 'm' {
                    break;
                }
            }
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
    let binary = env!("CARGO_BIN_EXE_mokumo-api");

    // Set up a temp data directory with the required layout
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_path_buf();

    // Create the directory structure the server expects
    mokumo_api::ensure_data_dirs(&data_dir).unwrap();

    // Initialize a real SQLite database so the server can start.
    // The server uses the profile-based path (demo/mokumo.db by default).
    let profile_db_path = data_dir.join("demo").join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", profile_db_path.display());
    let db = mokumo_db::initialize_database(&database_url).await.unwrap();
    db.close().await.ok();

    // Spawn the server with --port 0 for an OS-assigned port
    let mut server_proc = Command::new(binary)
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "--port",
            "0",
            "--host",
            "127.0.0.1",
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

    // reset-db checks for mokumo.db at the data_dir root (flat layout path).
    // Create a sentinel file AFTER the server has started so migrate_flat_layout
    // doesn't interfere with server startup.
    let root_db_path = data_dir.join("mokumo.db");
    std::fs::write(&root_db_path, b"").unwrap();

    // Point reset-db at the temp dir for recovery files so it never touches
    // the real Desktop or cwd, even if the flock guard regresses.
    let recovery_dir = data_dir.join("recovery");
    std::fs::create_dir_all(&recovery_dir).unwrap();

    // Now run reset-db against the same data directory — it should be blocked
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
        reset_stderr.contains("in use by a running server"),
        "stderr should contain the flock rejection message, got: {reset_stderr}"
    );

    // Verify the databases still exist (reset-db didn't delete anything)
    assert!(
        root_db_path.exists(),
        "root database file should still exist after blocked reset-db"
    );
    assert!(
        profile_db_path.exists(),
        "profile database file should still exist after blocked reset-db"
    );

    // Kill server (ServerGuard handles this on drop) and join reader threads
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
