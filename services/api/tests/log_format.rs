/// Log-format contract tests.
///
/// These tests drive the real tracing formatter against a string buffer and
/// snapshot-test the exact output format. If the log line format changes, these
/// snapshots break — which means `LISTENING_LOG_RE` and `SETUP_TOKEN_RE` in
/// `apps/web/tests/support/local-server.ts` must be updated to match.
///
/// Cross-reference: `apps/web/tests/support/local-server.ts`
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

/// Thread-safe string writer for capturing tracing output.
#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl SharedWriter {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }

    fn into_string(self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl std::io::Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriter;

    fn make_writer(&'a self) -> SharedWriter {
        self.clone()
    }
}

/// Emit one tracing event under a freshly-configured subscriber and return
/// the captured log line. Uses `without_time()` so snapshots are stable
/// across runs — the timestamp format is not the thing we're testing.
fn capture_tracing_line<F: FnOnce()>(f: F) -> String {
    let writer = SharedWriter::new();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer.clone())
        .with_ansi(false)
        .without_time()
        .finish();

    tracing::subscriber::with_default(subscriber, f);

    writer.into_string()
}

/// The "Listening on host:port" line must match `LISTENING_LOG_RE` in
/// `apps/web/tests/support/local-server.ts`.
#[test]
fn listening_line_format() {
    let output = capture_tracing_line(|| {
        tracing::info!("Listening on {}:{}", "127.0.0.1", 6565_u16);
    });
    assert!(
        output.contains("Listening on 127.0.0.1:6565"),
        "log format changed — update LISTENING_LOG_RE in local-server.ts:\n{output}"
    );
    insta::assert_snapshot!(output);
}

/// The "Setup required — token: X" line must match `SETUP_TOKEN_RE` in
/// `apps/web/tests/support/local-server.ts`.
#[test]
fn setup_token_line_format() {
    let output = capture_tracing_line(|| {
        tracing::info!("Setup required \u{2014} token: {}", "abc123-def456"); // gitleaks:allow
    });
    assert!(
        output.contains("Setup required \u{2014} token: abc123-def456"),
        "log format changed — update SETUP_TOKEN_RE in local-server.ts:\n{output}"
    );
    insta::assert_snapshot!(output);
}
