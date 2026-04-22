//! Transport-neutral diagnostics collection and bundle export for
//! Mokumo's admin surface.
//!
//! The snapshot constructs the `DiagnosticsResponse` wire DTO — whose
//! `DatabaseDiagnostics` names Mokumo's `production` + `demo` profiles
//! and whose `RuntimeDiagnostics::active_profile` field is typed as
//! `SetupMode` — so this module is Mokumo-specific by contract and lives
//! next to the admin UDS router that serves it.
//!
//! ## Signature choice: `&PlatformState`, not `&MokumoState`
//!
//! Diagnostics only reads platform fields (`data_dir`, pool map,
//! `mdns_status`, `active_profile`, `is_first_launch`, `started_at`).
//! Taking the narrower slice is honest about the real dependency and
//! lets the HTTP handler stay mounted on `PlatformState` without a
//! remount. UDS/CLI callers holding a full app state simply pass
//! `&state.platform_state()`.
//!
//! ## Error mapping seam
//!
//! Every internal error path (DB pool read, `tokio::fs::metadata`, zip
//! build) is semantically `Internal` — wrap at the seam via
//! `ControlPlaneError::Internal(anyhow::anyhow!(...))`. The HTTP
//! adapter renders that as 500; UDS renders it identically.

use std::io::{BufRead as _, BufReader, Cursor, Read as _, Write as _};
use std::path::Path;
use std::sync::OnceLock;

use chrono::Utc;
use kikan::{ControlPlaneError, PlatformState};
use kikan_types::SetupMode;
use kikan_types::diagnostics::{
    AppDiagnostics, DatabaseDiagnostics, DiagnosticsResponse, OsDiagnostics, ProfileDbDiagnostics,
    RuntimeDiagnostics, SystemDiagnostics,
};
use regex::Regex;
use sea_orm::DatabaseConnection;
use sysinfo::{Disks, System};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Collect the full diagnostics snapshot. Shared by the HTTP
/// `GET /api/diagnostics` handler and the bundle export so sysinfo is
/// refreshed in one place.
pub async fn collect(state: &PlatformState) -> Result<DiagnosticsResponse, ControlPlaneError> {
    // `DatabaseDiagnostics` names `production` + `demo` in its wire shape,
    // so resolve each explicitly by dir-name through the graft's pool map.
    let production_db_path = state.data_dir.join("production").join(state.db_filename);
    let demo_db_path = state.data_dir.join("demo").join(state.db_filename);
    let production_db = state.db_for("production").ok_or_else(|| {
        ControlPlaneError::Internal(anyhow::anyhow!(
            "production profile pool missing from PlatformState"
        ))
    })?;
    let demo_db = state.db_for("demo").ok_or_else(|| {
        ControlPlaneError::Internal(anyhow::anyhow!(
            "demo profile pool missing from PlatformState"
        ))
    })?;
    let production = read_profile_diagnostics(production_db, &production_db_path).await?;
    let demo = read_profile_diagnostics(demo_db, &demo_db_path).await?;

    let mdns = state.mdns_status.read().clone();
    let lan_url = if mdns.active {
        mdns.hostname
            .as_ref()
            .map(|h| format!("http://{}:{}", h, mdns.port))
    } else {
        None
    };
    let host = mdns
        .hostname
        .clone()
        .unwrap_or_else(|| mdns.bind_host.clone());

    let runtime = RuntimeDiagnostics {
        uptime_seconds: state.started_at.elapsed().as_secs(),
        active_profile: active_profile_as_setup_mode(state),
        setup_complete: state.is_setup_complete(),
        is_first_launch: state
            .is_first_launch
            .load(std::sync::atomic::Ordering::Acquire),
        mdns_active: mdns.active,
        lan_url,
        host,
        port: mdns.port,
    };

    // sysinfo refresh + Disks enumeration is synchronous and can take
    // hundreds of ms on slow filesystems. Hop to the blocking pool so
    // the async executor stays responsive.
    let data_dir = state.data_dir.clone();
    let system = tokio::task::spawn_blocking(move || collect_system_diagnostics(&data_dir))
        .await
        .map_err(|e| {
            ControlPlaneError::Internal(anyhow::anyhow!("system diagnostics worker panicked: {e}"))
        })?;

    Ok(DiagnosticsResponse {
        app: AppDiagnostics {
            name: env!("CARGO_PKG_NAME").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            build_commit: option_env!("VERGEN_GIT_SHA").map(Into::into),
        },
        database: DatabaseDiagnostics { production, demo },
        runtime,
        os: OsDiagnostics {
            family: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        },
        system,
    })
}

/// Build the diagnostics export archive in memory. Returns
/// `(zip_bytes, suggested_filename)`; the HTTP handler sets the
/// `Content-Type: application/zip` and `Content-Disposition` headers.
///
/// Contents:
/// - `metadata.json` — pretty-printed `DiagnosticsResponse` snapshot.
/// - `logs/mokumo*.log` — NDJSON log files from `data_dir/logs/`,
///   line-redacted via [`scrub_line`] before being written to the zip.
pub async fn build_bundle(state: &PlatformState) -> Result<(Vec<u8>, String), ControlPlaneError> {
    let diag = collect(state).await?;
    let metadata_json = serde_json::to_string_pretty(&diag).map_err(|e| {
        ControlPlaneError::Internal(anyhow::anyhow!("failed to serialize diagnostics: {e}"))
    })?;

    let log_dir = state.data_dir.join("logs");
    let zip_bytes = tokio::task::spawn_blocking(move || build_bundle_sync(metadata_json, log_dir))
        .await
        .map_err(|e| {
            ControlPlaneError::Internal(anyhow::anyhow!("bundle worker panicked: {e}"))
        })??;

    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!("mokumo-diagnostics-{timestamp}.zip");
    Ok((zip_bytes, filename))
}

/// Upper bound on the number of `mokumo*.log` files included in a bundle.
/// Log rotation produces at most a handful of files under normal
/// operation; this cap guards against an operator pointing the server
/// at a log directory with accumulated history from other sources.
const MAX_LOG_FILES: usize = 32;

/// Upper bound on uncompressed log bytes written into the bundle. When
/// hit, the zip is finalized with a `logs/TRUNCATED` marker so the
/// operator can tell data was dropped. 64 MiB is generous vs the disk-
/// warning threshold (500 MiB free) while staying well below Axum's
/// default response body limits.
const MAX_LOG_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

fn build_bundle_sync(
    metadata_json: String,
    log_dir: std::path::PathBuf,
) -> Result<Vec<u8>, ControlPlaneError> {
    let buf = Vec::new();
    let cursor = Cursor::new(buf);
    let mut zip = ZipWriter::new(cursor);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("metadata.json", opts)
        .map_err(|e| ControlPlaneError::Internal(anyhow::anyhow!("zip start_file: {e}")))?;
    zip.write_all(metadata_json.as_bytes())
        .map_err(|e| ControlPlaneError::Internal(anyhow::anyhow!("zip write metadata: {e}")))?;

    if log_dir.is_dir() {
        let truncated = write_log_files(&mut zip, opts, &log_dir)?;
        if truncated {
            zip.start_file("logs/TRUNCATED", opts).map_err(|e| {
                ControlPlaneError::Internal(anyhow::anyhow!("zip start_file TRUNCATED: {e}"))
            })?;
            let marker = format!(
                "diagnostics bundle truncated: exceeded MAX_LOG_FILES={MAX_LOG_FILES} \
                 or MAX_LOG_TOTAL_BYTES={MAX_LOG_TOTAL_BYTES}\n"
            );
            zip.write_all(marker.as_bytes()).map_err(|e| {
                ControlPlaneError::Internal(anyhow::anyhow!("zip write TRUNCATED: {e}"))
            })?;
        }
    }

    let cursor = zip
        .finish()
        .map_err(|e| ControlPlaneError::Internal(anyhow::anyhow!("zip finish: {e}")))?;
    Ok(cursor.into_inner())
}

/// Iterate `log_dir`, scrub + append each `mokumo*.log` into `zip`.
/// Returns `true` when the caps were hit and the caller should emit a
/// `logs/TRUNCATED` marker entry.
fn write_log_files(
    zip: &mut ZipWriter<Cursor<Vec<u8>>>,
    opts: SimpleFileOptions,
    log_dir: &Path,
) -> Result<bool, ControlPlaneError> {
    let patterns = redact_patterns();
    let mut entries: Vec<_> = std::fs::read_dir(log_dir)
        .map_err(|e| ControlPlaneError::Internal(anyhow::anyhow!("read log dir: {e}")))?
        .filter_map(|entry| match entry {
            Ok(e) => Some(e),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read directory entry in log dir");
                None
            }
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut bytes_written: u64 = 0;
    let mut files_included: usize = 0;

    for entry in entries {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if n.starts_with("mokumo") && n.ends_with(".log") => n.to_string(),
            _ => continue,
        };

        if files_included >= MAX_LOG_FILES {
            return Ok(true);
        }

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
        zip.start_file(&zip_path, opts).map_err(|e| {
            ControlPlaneError::Internal(anyhow::anyhow!("zip start_file logs/{name}: {e}"))
        })?;
        files_included += 1;

        // Cap raw bytes read so a pathological log file with no newlines
        // (or a single multi-GiB line) cannot exhaust memory. The take()
        // wrapper enforces the cap at read-time, before BufRead::lines
        // allocates the per-line String.
        let remaining_budget = MAX_LOG_TOTAL_BYTES.saturating_sub(bytes_written);
        let reader = BufReader::new(file.take(remaining_budget));
        let mut hit_cap = false;
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
            let line_bytes = scrubbed.len() as u64 + 1;
            if bytes_written.saturating_add(line_bytes) > MAX_LOG_TOTAL_BYTES {
                hit_cap = true;
                break;
            }
            zip.write_all(scrubbed.as_bytes()).map_err(|e| {
                ControlPlaneError::Internal(anyhow::anyhow!("zip write logs/{name}: {e}"))
            })?;
            zip.write_all(b"\n").map_err(|e| {
                ControlPlaneError::Internal(anyhow::anyhow!("zip write logs/{name}: {e}"))
            })?;
            bytes_written += line_bytes;
        }
        if hit_cap || bytes_written >= MAX_LOG_TOTAL_BYTES {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Returns `true` when available disk space for the data directory is
/// below the threshold.
///
/// Threshold is read from `MOKUMO_DISK_WARNING_THRESHOLD_BYTES` (default
/// 500 MiB). Set to `0` to disable the warning entirely — the `u64`
/// comparison `available < 0` is never true. Returns `false` when no
/// disk volume can be found (not a blocking condition).
pub fn compute_disk_warning(data_dir: &Path) -> bool {
    let threshold: u64 = std::env::var("MOKUMO_DISK_WARNING_THRESHOLD_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(524_288_000);

    let disks = Disks::new_with_refreshed_list();
    let disk = disks
        .iter()
        .filter(|d| data_dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len());

    disk.map(|d| d.available_space() < threshold)
        .unwrap_or(false)
}

fn collect_system_diagnostics(data_dir: &Path) -> SystemDiagnostics {
    let mut sys = System::new();
    sys.refresh_memory();

    let hostname = System::host_name();

    let disks = Disks::new_with_refreshed_list();
    let disk = disks
        .iter()
        .filter(|d| data_dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len());

    if disk.is_none() {
        tracing::warn!(
            data_dir = %data_dir.display(),
            "No disk volume found for data directory; disk stats will be null"
        );
    }

    SystemDiagnostics {
        hostname,
        total_memory_bytes: sys.total_memory(),
        used_memory_bytes: sys.used_memory(),
        disk_total_bytes: disk.map(|d| d.total_space()),
        disk_free_bytes: disk.map(|d| d.available_space()),
        disk_warning: compute_disk_warning(data_dir),
    }
}

/// Wire-shape bridge: resolve the active profile dir-name back to the
/// `SetupMode` variant the diagnostics DTO expects. Falls back to Demo
/// when the dir-name does not parse — matches the pre-refactor default.
fn active_profile_as_setup_mode(state: &PlatformState) -> SetupMode {
    use std::str::FromStr;
    let active = state.active_profile.read();
    SetupMode::from_str(active.as_str()).unwrap_or(SetupMode::Demo)
}

async fn read_profile_diagnostics(
    db: &DatabaseConnection,
    db_path: &Path,
) -> Result<ProfileDbDiagnostics, ControlPlaneError> {
    let rt = kikan::db::read_db_runtime_diagnostics(db)
        .await
        .map_err(|e| {
            ControlPlaneError::Internal(anyhow::anyhow!("read_db_runtime_diagnostics failed: {e}"))
        })?;
    let file_size_bytes = match tokio::fs::metadata(db_path).await {
        Ok(m) => Some(m.len()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(ControlPlaneError::Internal(anyhow::anyhow!(
                "metadata({}) failed: {e}",
                db_path.display()
            )));
        }
    };

    let db_path_owned = db_path.to_path_buf();
    let (wal_size_bytes, vacuum_needed) =
        match tokio::task::spawn_blocking(move || kikan::db::diagnose_database(&db_path_owned))
            .await
        {
            Ok(Ok(d)) => (d.wal_size_bytes, d.vacuum_needed()),
            Ok(Err(e)) => {
                tracing::warn!(db = %db_path.display(), "diagnose_database failed: {e}");
                (0, false)
            }
            Err(e) => {
                tracing::warn!("spawn_blocking for diagnose_database panicked: {e}");
                (0, false)
            }
        };

    Ok(ProfileDbDiagnostics {
        schema_version: rt.schema_version,
        file_size_bytes,
        wal_mode: rt.wal_mode,
        wal_size_bytes,
        vacuum_needed,
    })
}

/// Patterns applied to each log line to redact common sensitive values.
/// Compiled once per process lifetime via `OnceLock`.
///
/// Pattern design for NDJSON compatibility:
/// - `["']?\s*[:=]\s*["']?` handles both JSON (`"password":"secret"`) and
///   plain-text (`password=secret`, `password: secret`) key-value formats.
/// - `[^\s,{}"']+` stops at JSON delimiters to avoid swallowing adjacent
///   fields (`\S+` was too greedy: `{"password":"x","user":"y"}` would
///   redact `x","user":"y"}`).
pub(crate) fn redact_patterns() -> &'static [(Regex, &'static str)] {
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

/// Scrub one log line. Returns a `Cow` so clean lines avoid allocation
/// entirely.
pub(crate) fn scrub_line<'a>(
    line: &'a str,
    patterns: &[(Regex, &'static str)],
) -> std::borrow::Cow<'a, str> {
    let mut result = std::borrow::Cow::Borrowed(line);
    for (pattern, replacement) in patterns {
        if let std::borrow::Cow::Owned(scrubbed) = pattern.replace_all(&result, *replacement) {
            result = std::borrow::Cow::Owned(scrubbed);
        }
    }
    result
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

    /// Verify `\S+` was not used: password redaction must not swallow
    /// adjacent JSON fields.
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
