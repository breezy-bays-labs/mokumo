use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    EnvFilter, Layer, Registry, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

/// Initialize the global tracing subscriber with dual-layer output.
///
/// - **Console layer**: human-readable text with ANSI colors, filtered by `console_level`
///   when provided, otherwise by `RUST_LOG` (defaults to `info`).
/// - **File layer** (when `log_dir` is `Some`): JSON (NDJSON) with daily rotation and
///   7-day retention via `max_log_files(7)`. Uses a fixed `info` filter regardless of
///   `RUST_LOG` to keep production log volume predictable.
///
/// `console_level` accepts a tracing directive string (`"error"`, `"warn"`, `"info"`,
/// `"debug"`, `"trace"`). When `Some`, it overrides `RUST_LOG` for the console layer.
/// When `None`, `RUST_LOG` is used (defaulting to `"info"` on parse failure).
///
/// Returns the [`WorkerGuard`] for the non-blocking file writer. The caller **must**
/// hold this guard for the process lifetime — dropping it flushes buffered logs and
/// stops the background writer thread.
///
/// If `log_dir` is `None` or the file appender fails to initialize, only console
/// output is active and `None` is returned.
pub fn init_tracing(log_dir: Option<&Path>, console_level: Option<&str>) -> Option<WorkerGuard> {
    let console_filter = if let Some(level) = console_level {
        EnvFilter::new(level)
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|e| {
            if std::env::var_os("RUST_LOG").is_some() {
                eprintln!("WARNING: Invalid RUST_LOG value, falling back to 'info': {e}");
            }
            "info".into()
        })
    };

    let console_layer = fmt::layer().with_target(true).with_filter(console_filter);

    let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = vec![Box::new(console_layer)];
    let mut guard = None;

    if let Some(dir) = log_dir {
        match build_file_layer(dir) {
            Ok((layer, g)) => {
                layers.push(Box::new(layer));
                guard = Some(g);
            }
            Err(e) => {
                eprintln!("WARNING: Failed to initialize file logging: {e}");
            }
        }
    }

    tracing_subscriber::registry().with(layers).init();

    guard
}

fn build_file_layer(
    log_dir: &Path,
) -> Result<(impl Layer<Registry> + Send + Sync, WorkerGuard), tracing_appender::rolling::InitError>
{
    let appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("mokumo")
        .filename_suffix("log")
        .max_log_files(7)
        .build(log_dir)?;

    let (non_blocking, guard) = tracing_appender::non_blocking(appender);

    let layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .json()
        .with_writer(non_blocking)
        .with_filter(EnvFilter::new("info"));

    Ok((layer, guard))
}

/// Map `--verbose` / `--quiet` CLI flags to a tracing directive string.
///
/// Returns `None` when neither flag is set, deferring to `RUST_LOG`.
/// `quiet` takes precedence over `verbose` when both are somehow present.
pub fn console_level_from_flags(quiet: bool, verbose: u8) -> Option<&'static str> {
    if quiet {
        Some("error")
    } else {
        match verbose {
            0 => None,
            1 => Some("debug"),
            _ => Some("trace"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn console_level_from_flags_defaults_to_none() {
        assert_eq!(console_level_from_flags(false, 0), None);
    }

    #[test]
    fn console_level_from_flags_single_v_is_debug() {
        assert_eq!(console_level_from_flags(false, 1), Some("debug"));
    }

    #[test]
    fn console_level_from_flags_double_v_is_trace() {
        assert_eq!(console_level_from_flags(false, 2), Some("trace"));
        assert_eq!(console_level_from_flags(false, 255), Some("trace"));
    }

    #[test]
    fn console_level_from_flags_quiet_is_error() {
        assert_eq!(console_level_from_flags(true, 0), Some("error"));
    }

    #[test]
    fn console_level_from_flags_quiet_takes_precedence() {
        // quiet wins even if verbose is also somehow set
        assert_eq!(console_level_from_flags(true, 2), Some("error"));
    }

    #[test]
    fn build_file_layer_fails_for_nonexistent_path() {
        assert!(build_file_layer(Path::new("/nonexistent/path/that/should/fail")).is_err());
    }

    #[test]
    fn build_file_layer_creates_appender() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let (_layer, _guard) = build_file_layer(tmp.path()).expect("build file layer");
    }

    #[test]
    fn file_output_is_valid_ndjson() {
        // Use a synchronous in-memory writer to avoid non-blocking flush timing issues.
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = buffer.clone();

        let file_layer = fmt::layer()
            .with_target(true)
            .with_thread_ids(true)
            .json()
            .with_writer(move || -> Box<dyn std::io::Write> {
                Box::new(SharedWriter(writer.clone()))
            })
            .with_filter(EnvFilter::new("info"));

        let console_layer = fmt::layer()
            .with_target(true)
            .with_filter(EnvFilter::new("off"));

        let _guard = tracing_subscriber::registry()
            .with(console_layer)
            .with(file_layer)
            .set_default();

        tracing::info!(test_field = "hello", "test log message");
        tracing::warn!(count = 42, "another message");

        let output = String::from_utf8(buffer.lock().unwrap().clone()).expect("valid UTF-8");
        let lines: Vec<&str> = output.lines().collect();

        assert!(
            lines.len() >= 2,
            "expected at least 2 log lines, got {}",
            lines.len()
        );

        for line in &lines {
            let parsed: serde_json::Value =
                serde_json::from_str(line).expect("each line should be valid JSON");

            assert!(parsed.get("timestamp").is_some(), "missing timestamp");
            assert!(parsed.get("level").is_some(), "missing level");
            assert!(parsed.get("target").is_some(), "missing target");
            assert!(
                parsed.get("fields").is_some() || parsed.get("message").is_some(),
                "missing fields or message"
            );
        }

        // Verify specific field values
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["level"].as_str(), Some("INFO"));

        let fields = &first["fields"];
        assert_eq!(fields["message"].as_str(), Some("test log message"));
        assert_eq!(fields["test_field"].as_str(), Some("hello"));
    }

    /// A writer that appends to a shared buffer, used for synchronous test output.
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
