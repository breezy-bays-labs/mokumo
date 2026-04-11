use std::io::{Cursor, Write as _};

use axum::{extract::State, http::header, response::IntoResponse};
use chrono::Utc;
use regex::Regex;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::{SharedState, diagnostics, error::AppError};

/// Patterns applied to each log line to redact common sensitive values.
/// Compiled once per bundle export (non-hot path).
fn redact_patterns() -> Vec<(Regex, &'static str)> {
    vec![
        (
            Regex::new(r"(?i)bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap(),
            "Bearer [REDACTED]",
        ),
        (
            Regex::new(r"(?i)password\s*[:=]\s*\S+").unwrap(),
            "password=[REDACTED]",
        ),
        (
            Regex::new(r"(?i)secret\s*[:=]\s*\S+").unwrap(),
            "secret=[REDACTED]",
        ),
        (
            Regex::new(r"(?i)api[_-]?key\s*[:=]\s*\S+").unwrap(),
            "api_key=[REDACTED]",
        ),
    ]
}

fn scrub_line(line: &str, patterns: &[(Regex, &'static str)]) -> String {
    let mut result = line.to_string();
    for (pattern, replacement) in patterns {
        result = pattern.replace_all(&result, *replacement).into_owned();
    }
    result
}

pub async fn handler(State(state): State<SharedState>) -> Result<impl IntoResponse, AppError> {
    // Collect diagnostics snapshot (shares sysinfo logic with GET /api/diagnostics).
    let diag = diagnostics::collect(&state).await?;
    let metadata_json = serde_json::to_string_pretty(&diag)
        .map_err(|e| AppError::InternalError(format!("failed to serialize diagnostics: {e}")))?;

    // Build ZIP archive in memory.
    let buf = Vec::new();
    let cursor = Cursor::new(buf);
    let mut zip = ZipWriter::new(cursor);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // Add metadata.json
    zip.start_file("metadata.json", opts)
        .map_err(|e| AppError::InternalError(format!("zip start_file: {e}")))?;
    zip.write_all(metadata_json.as_bytes())
        .map_err(|e| AppError::InternalError(format!("zip write metadata: {e}")))?;

    // Add log files from data_dir/logs/ (NDJSON, one file per rotation day).
    let log_dir = state.data_dir.join("logs");
    if log_dir.is_dir() {
        let patterns = redact_patterns();
        let mut entries: Vec<_> = std::fs::read_dir(&log_dir)
            .map_err(|e| AppError::InternalError(format!("read log dir: {e}")))?
            .filter_map(|entry| match entry {
                Ok(e) => Some(e),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to read directory entry in log dir");
                    None
                }
            })
            .collect();
        // Sort by file name for deterministic zip order.
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) if n.starts_with("mokumo") && n.ends_with(".log") => n.to_string(),
                _ => continue,
            };

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Skipping log file in diagnostics bundle: could not read"
                    );
                    continue;
                }
            };

            // Scrub each line for sensitive patterns.
            let scrubbed: String = content
                .lines()
                .map(|line| scrub_line(line, &patterns))
                .collect::<Vec<_>>()
                .join("\n");

            let zip_path = format!("logs/{name}");
            zip.start_file(&zip_path, opts)
                .map_err(|e| AppError::InternalError(format!("zip start_file logs/{name}: {e}")))?;
            zip.write_all(scrubbed.as_bytes())
                .map_err(|e| AppError::InternalError(format!("zip write logs/{name}: {e}")))?;
        }
    }

    let cursor = zip
        .finish()
        .map_err(|e| AppError::InternalError(format!("zip finish: {e}")))?;
    let zip_bytes = cursor.into_inner();

    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!("mokumo-diagnostics-{timestamp}.zip");

    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        zip_bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::{redact_patterns, scrub_line};

    #[test]
    fn scrubs_bearer_token() {
        let patterns = redact_patterns();
        let result = scrub_line("Authorization: Bearer abc.def.ghi", &patterns);
        assert!(
            !result.contains("abc.def.ghi"),
            "bearer token not scrubbed: {result}"
        );
        assert!(
            result.contains("Bearer [REDACTED]"),
            "expected redaction marker: {result}"
        );
    }

    #[test]
    fn scrubs_password_field() {
        let patterns = redact_patterns();
        let result = scrub_line("user login password: mysecret123", &patterns);
        assert!(
            !result.contains("mysecret123"),
            "password not scrubbed: {result}"
        );
    }

    #[test]
    fn scrubs_api_key() {
        let patterns = redact_patterns();
        let result = scrub_line("api_key=abc123xyz", &patterns);
        assert!(
            !result.contains("abc123xyz"),
            "api_key not scrubbed: {result}"
        );
    }

    #[test]
    fn clean_line_passes_through_unchanged() {
        let patterns = redact_patterns();
        let input = r#"{"level":"info","message":"order created","order_id":"ord_123"}"#;
        let result = scrub_line(input, &patterns);
        assert_eq!(result, input, "clean line should not be modified");
    }

    #[test]
    fn scrubs_multiple_patterns_in_one_line() {
        let patterns = redact_patterns();
        let result = scrub_line("secret=topsecret api_key=mykey", &patterns);
        assert!(
            !result.contains("topsecret"),
            "secret not scrubbed: {result}"
        );
        assert!(!result.contains("mykey"), "api_key not scrubbed: {result}");
    }
}
