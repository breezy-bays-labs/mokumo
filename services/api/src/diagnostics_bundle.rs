use std::io::{BufRead as _, BufReader, Cursor, Write as _};
use std::sync::OnceLock;

use axum::{extract::State, http::header, response::IntoResponse};
use chrono::Utc;
use regex::Regex;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::{SharedState, diagnostics, error::AppError};

/// Patterns applied to each log line to redact common sensitive values.
/// Compiled once per process lifetime via OnceLock (non-hot path, but called per log line).
///
/// Pattern design for NDJSON compatibility:
/// - `["']?\s*[:=]\s*["']?` handles both JSON (`"password":"secret"`) and
///   plain-text (`password=secret`, `password: secret`) key-value formats.
/// - `[^\s,{}"']+` stops at JSON delimiters to avoid swallowing adjacent fields
///   (`\S+` was too greedy: `{"password":"x","user":"y"}` → would redact `x","user":"y"}`).
fn redact_patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                Regex::new(r"(?i)bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap(),
                "Bearer [REDACTED]",
            ),
            (
                Regex::new(r#"(?i)password\s*["']?\s*[:=]\s*["']?[^\s,{}"']+"#).unwrap(),
                "password=[REDACTED]",
            ),
            (
                Regex::new(r#"(?i)secret\s*["']?\s*[:=]\s*["']?[^\s,{}"']+"#).unwrap(),
                "secret=[REDACTED]",
            ),
            (
                Regex::new(r#"(?i)api[_-]?key\s*["']?\s*[:=]\s*["']?[^\s,{}"']+"#).unwrap(),
                "api_key=[REDACTED]",
            ),
        ]
    })
}

/// Scrub one log line. Returns a `Cow` so clean lines avoid allocation entirely.
fn scrub_line<'a>(line: &'a str, patterns: &[(Regex, &'static str)]) -> std::borrow::Cow<'a, str> {
    let mut result = std::borrow::Cow::Borrowed(line);
    for (pattern, replacement) in patterns {
        if let std::borrow::Cow::Owned(scrubbed) = pattern.replace_all(&result, *replacement) {
            result = std::borrow::Cow::Owned(scrubbed);
        }
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

            let file = match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Skipping log file in diagnostics bundle: could not open"
                    );
                    continue;
                }
            };

            let zip_path = format!("logs/{name}");
            zip.start_file(&zip_path, opts)
                .map_err(|e| AppError::InternalError(format!("zip start_file logs/{name}: {e}")))?;

            // Stream line-by-line: avoids loading the entire file into memory before writing.
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "Error reading line from log file; skipping remainder"
                        );
                        break;
                    }
                };
                let scrubbed = scrub_line(&line, patterns);
                zip.write_all(scrubbed.as_bytes())
                    .map_err(|e| AppError::InternalError(format!("zip write logs/{name}: {e}")))?;
                zip.write_all(b"\n")
                    .map_err(|e| AppError::InternalError(format!("zip write logs/{name}: {e}")))?;
            }
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
        let result = scrub_line("Authorization: Bearer abc.def.ghi", patterns);
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
        let result = scrub_line("user login password: mysecret123", patterns);
        assert!(
            !result.contains("mysecret123"),
            "password not scrubbed: {result}"
        );
    }

    #[test]
    fn scrubs_api_key() {
        let patterns = redact_patterns();
        let result = scrub_line("api_key=abc123xyz", patterns);
        assert!(
            !result.contains("abc123xyz"),
            "api_key not scrubbed: {result}"
        );
    }

    #[test]
    fn clean_line_passes_through_unchanged() {
        let patterns = redact_patterns();
        let input = r#"{"level":"info","message":"order created","order_id":"ord_123"}"#;
        let result = scrub_line(input, patterns);
        assert_eq!(result, input, "clean line should not be modified");
    }

    #[test]
    fn scrubs_multiple_patterns_in_one_line() {
        let patterns = redact_patterns();
        let result = scrub_line("secret=topsecret api_key=mykey", patterns);
        assert!(
            !result.contains("topsecret"),
            "secret not scrubbed: {result}"
        );
        assert!(!result.contains("mykey"), "api_key not scrubbed: {result}");
    }

    /// Verify `\S+` was not used: password redaction must not swallow adjacent JSON fields.
    #[test]
    fn password_redaction_does_not_over_redact_json() {
        let patterns = redact_patterns();
        let input = r#"{"password":"secret","user":"admin"}"#;
        let result = scrub_line(input, patterns);
        assert!(
            result.contains("\"user\":\"admin\""),
            "adjacent JSON field should not be redacted: {result}"
        );
        assert!(
            !result.contains("secret"),
            "password value should be scrubbed: {result}"
        );
    }
}
