use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::restore::{self, RestoreError};
use axum::Json;
use axum::extract::{FromRequest, Multipart, Request, State};
use axum::http::header::CONTENT_TYPE;
use kikan_types::error::ErrorCode;
use kikan_types::setup::{RestoreResponse, RestoreValidateResponse};
use serde::Deserialize;

use crate::state::SharedMokumoState as SharedState;
use kikan::AppError;

// ── Guard ──────────────────────────────────────────────────────────────────────

/// RAII guard: ensures at most one restore is in-flight at a time, and that
/// the server is in first-launch state (no production database on disk).
///
/// Modelled after `SetupAttemptGuard` in `auth/mod.rs`.
struct RestoreGuard {
    flag: Arc<AtomicBool>,
}

impl RestoreGuard {
    /// Attempt to acquire the guard.
    ///
    /// Checks three conditions in order:
    /// (a) `is_first_launch` is true — the welcome screen is active.
    /// (b) `data_dir/production/mokumo.db` does not exist — no DB to overwrite.
    /// (c) CAS on `restore_in_progress` — no concurrent restore in flight.
    ///
    /// Then re-checks `is_first_launch` after acquiring the flag to close the
    /// TOCTOU window between checks (a) and (c).
    fn acquire(state: &SharedState) -> Result<Self, AppError> {
        // (a) Endpoint is only available during first-launch (welcome screen active).
        //     `is_first_launch` becomes false once a profile is selected — use 403
        //     so the client knows the route is permanently unavailable, not in conflict.
        if !state.is_first_launch().load(Ordering::Acquire) {
            return Err(AppError::Forbidden(
                ErrorCode::Forbidden,
                "Restore is only available on first launch.".into(),
            ));
        }

        // (b) Production DB must not already exist on disk.
        let prod_path = state.data_dir().join("production").join("mokumo.db");
        if prod_path.exists() {
            return Err(AppError::StateConflict(
                ErrorCode::ProductionDbExists,
                "A production database already exists.".into(),
            ));
        }

        // (c) At most one restore in flight at a time.
        if state
            .restore_in_progress()
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(AppError::StateConflict(
                ErrorCode::RestoreInProgress,
                "Another restore operation is already in progress.".into(),
            ));
        }

        // Double-check after acquiring the flag (TOCTOU mitigation for (a)).
        if !state.is_first_launch().load(Ordering::Acquire) {
            state.restore_in_progress().store(false, Ordering::Release);
            return Err(AppError::Forbidden(
                ErrorCode::Forbidden,
                "Restore is only available on first launch.".into(),
            ));
        }

        Ok(Self {
            flag: state.restore_in_progress().clone(),
        })
    }
}

impl Drop for RestoreGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

// ── Candidate extraction ───────────────────────────────────────────────────────

/// Holds the path to a candidate restore file. If the file was uploaded via
/// multipart, the `NamedTempFile` keeps it alive until this struct is dropped.
struct CandidatePath {
    path: PathBuf,
    /// Temp file handle — `None` for path-based (Tauri) requests.
    _temp: Option<tempfile::NamedTempFile>,
}

#[derive(Deserialize)]
struct PathRequest {
    path: String,
}

/// Extract the candidate file path from the request body.
///
/// Content-Type dispatch:
/// - `multipart/form-data` → save the `"file"` field to a temp file in `data_dir`
/// - `application/json`   → parse `{ "path": "/..." }` (Tauri native dialog)
/// - anything else        → 400 Bad Request
async fn extract_candidate(
    content_type: &str,
    req: Request,
    data_dir: &std::path::Path,
    state: &SharedState,
) -> Result<CandidatePath, AppError> {
    if content_type.starts_with("multipart/form-data") {
        let mut mp = Multipart::from_request(req, state).await.map_err(|e| {
            AppError::BadRequest(
                ErrorCode::ParseError,
                format!("Invalid multipart body: {e}"),
            )
        })?;

        while let Some(mut field) = mp.next_field().await.map_err(|e| {
            AppError::BadRequest(ErrorCode::ParseError, format!("Multipart read error: {e}"))
        })? {
            if field.name() != Some("file") {
                continue;
            }

            // Create the temp file first, then stream chunks into it to avoid
            // loading the entire upload (potentially hundreds of MB) into RAM.
            let temp = tempfile::NamedTempFile::new_in(data_dir).map_err(|e| {
                tracing::error!("restore: failed to create temp file: {e}");
                AppError::InternalError("Failed to save uploaded file".into())
            })?;
            let std_file = temp.as_file().try_clone().map_err(|e| {
                tracing::error!("restore: failed to clone temp file handle: {e}");
                AppError::InternalError("Failed to save uploaded file".into())
            })?;
            let mut async_file = tokio::fs::File::from_std(std_file);

            use tokio::io::AsyncWriteExt as _;
            while let Some(chunk) = field.chunk().await.map_err(|e| {
                AppError::BadRequest(ErrorCode::ParseError, format!("Multipart read error: {e}"))
            })? {
                async_file.write_all(&chunk).await.map_err(|e| {
                    tracing::error!("restore: failed to write chunk to temp file: {e}");
                    AppError::InternalError("Failed to save uploaded file".into())
                })?;
            }
            async_file.flush().await.map_err(|e| {
                tracing::error!("restore: failed to flush temp file: {e}");
                AppError::InternalError("Failed to save uploaded file".into())
            })?;
            drop(async_file);

            let path = temp.path().to_owned();
            return Ok(CandidatePath {
                path,
                _temp: Some(temp),
            });
        }

        Err(AppError::BadRequest(
            ErrorCode::ParseError,
            "No 'file' field found in multipart body".into(),
        ))
    } else if content_type.starts_with("application/json") {
        let Json(body) = Json::<PathRequest>::from_request(req, state)
            .await
            .map_err(|e| {
                AppError::BadRequest(ErrorCode::ParseError, format!("Invalid JSON body: {e}"))
            })?;

        Ok(CandidatePath {
            path: PathBuf::from(body.path),
            _temp: None,
        })
    } else {
        Err(AppError::BadRequest(
            ErrorCode::ParseError,
            "Content-Type must be multipart/form-data or application/json".into(),
        ))
    }
}

// ── Error mapping ──────────────────────────────────────────────────────────────

fn map_restore_error(err: RestoreError) -> AppError {
    match err {
        RestoreError::NotKikanDatabase { .. } => AppError::UnprocessableEntity(
            ErrorCode::NotMokumoDatabase,
            "This file is not a valid Mokumo database.".into(),
        ),
        RestoreError::DatabaseCorrupt { .. } => AppError::UnprocessableEntity(
            ErrorCode::DatabaseCorrupt,
            "This database file appears to be damaged. Try a different backup file.".into(),
        ),
        RestoreError::SchemaIncompatible { .. } => AppError::UnprocessableEntity(
            ErrorCode::SchemaIncompatible,
            "This database was created with a newer version of Mokumo. \
             Please update Mokumo before importing."
                .into(),
        ),
        RestoreError::ProductionDbExists { .. } => AppError::StateConflict(
            ErrorCode::ProductionDbExists,
            "A production database already exists.".into(),
        ),
        RestoreError::Sqlite(e) => {
            tracing::error!("restore: SQLite error: {e}");
            AppError::InternalError("Database operation failed".into())
        }
        RestoreError::Io(e) => {
            tracing::error!("restore: I/O error: {e}");
            AppError::InternalError("File operation failed".into())
        }
    }
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// POST /api/shop/restore/validate
///
/// Validates a candidate `.db` file without committing anything to disk.
/// Rate-limited to 5 attempts/hour (shared bucket with `/api/shop/restore`).
/// Gated by `RestoreGuard` — returns 403 when not in first-launch state.
pub async fn validate_handler(
    State(state): State<SharedState>,
    req: Request,
) -> Result<Json<RestoreValidateResponse>, AppError> {
    // Acquire guard before reading the body — avoids wasting I/O on rejected state
    // and prevents non-first-launch traffic from exhausting the rate-limit bucket.
    let _guard = RestoreGuard::acquire(&state)?;

    // Rate limit only counts attempts that pass the first-launch guard.
    if !state.restore_limiter().check_and_record("restore") {
        return Err(AppError::TooManyRequests(
            "Too many restore attempts. Please wait before trying again.".into(),
        ));
    }

    let content_type = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    let candidate = extract_candidate(&content_type, req, state.data_dir(), &state).await?;
    let file_name = candidate
        .path
        .file_name()
        .map_or_else(|| "unknown.db".into(), |n| n.to_string_lossy().into_owned());

    let path = candidate.path.clone();
    let info = tokio::task::spawn_blocking(move || restore::validate_candidate(&path))
        .await
        .map_err(|_| AppError::InternalError("Validation task panicked".into()))?
        .map_err(map_restore_error)?;

    // _guard and candidate (+ temp file) dropped here.

    Ok(Json(RestoreValidateResponse {
        file_name,
        file_size: info.file_size.get(),
        schema_version: info.schema_version,
    }))
}

/// POST /api/shop/restore
///
/// Validates, copies the candidate file to the production slot, writes the
/// `active_profile` sentinel, and triggers a graceful server restart.
///
/// Rate-limited (shared bucket with `/api/shop/restore/validate`).
/// Gated by `RestoreGuard` held for the entire operation.
pub async fn restore_handler(
    State(state): State<SharedState>,
    req: Request,
) -> Result<Json<RestoreResponse>, AppError> {
    // Hold the guard for the entire operation — prevents concurrent restores
    // and keeps the rate-limit bucket scoped to legitimate first-launch traffic.
    let _guard = RestoreGuard::acquire(&state)?;

    if !state.restore_limiter().check_and_record("restore") {
        return Err(AppError::TooManyRequests(
            "Too many restore attempts. Please wait before trying again.".into(),
        ));
    }

    let content_type = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    let candidate = extract_candidate(&content_type, req, state.data_dir(), &state).await?;
    let production_dir = state.data_dir().join("production");
    let path = candidate.path.clone();
    let production_dir_clone = production_dir.clone();

    // Step 1: re-validate (TOCTOU) then copy to production slot.
    let copy_result = tokio::task::spawn_blocking(move || {
        restore::validate_candidate(&path)?;
        restore::copy_to_production(&path, &production_dir_clone)
    })
    .await
    .map_err(|_| AppError::InternalError("Restore task panicked".into()))?;

    if let Err(e) = copy_result {
        return Err(map_restore_error(e));
    }

    // Step 2: Write active_profile = "production" (temp + atomic rename).
    let profile_path = state.data_dir().join("active_profile");
    let profile_tmp = state.data_dir().join("active_profile.tmp");
    if let Err(e) = tokio::fs::write(&profile_tmp, "production").await {
        tracing::error!("restore: failed to write active_profile.tmp: {e}; rolling back");
        if let Err(ce) = tokio::fs::remove_file(&profile_tmp).await {
            tracing::debug!("restore: partial tmp cleanup failed (may not exist): {ce}");
        }
        if let Err(rb) = tokio::fs::remove_file(production_dir.join("mokumo.db")).await {
            tracing::error!(
                "restore: CRITICAL — rollback failed, production DB may be orphaned: {rb}"
            );
            return Err(AppError::InternalError(
                "Restore failed and rollback incomplete — manual cleanup may be required".into(),
            ));
        }
        return Err(AppError::InternalError(
            "Failed to persist profile selection".into(),
        ));
    }
    if let Err(e) = tokio::fs::rename(&profile_tmp, &profile_path).await {
        tracing::error!("restore: failed to rename active_profile: {e}; rolling back");
        if let Err(rb) = tokio::fs::remove_file(production_dir.join("mokumo.db")).await {
            tracing::error!(
                "restore: CRITICAL — rollback failed, production DB may be orphaned: {rb}"
            );
            return Err(AppError::InternalError(
                "Restore failed and rollback incomplete — manual cleanup may be required".into(),
            ));
        }
        if let Err(rb) = tokio::fs::remove_file(&profile_tmp).await {
            // Stale .tmp file is not a data-loss risk — log but continue.
            tracing::error!("restore: rollback could not remove active_profile.tmp: {rb}");
        }
        return Err(AppError::InternalError(
            "Failed to persist profile selection".into(),
        ));
    }

    // Step 3: Write restart sentinel.
    let sentinel = state.data_dir().join(".restart");
    if let Err(e) = tokio::fs::write(&sentinel, b"restore").await {
        tracing::error!("restore: failed to write restart sentinel: {e}; rolling back");
        if let Err(rb) = tokio::fs::remove_file(production_dir.join("mokumo.db")).await {
            tracing::error!(
                "restore: CRITICAL — rollback failed, production DB may be orphaned: {rb}"
            );
            return Err(AppError::InternalError(
                "Restore failed and rollback incomplete — manual cleanup may be required".into(),
            ));
        }
        if let Err(rb) = tokio::fs::remove_file(&profile_path).await {
            tracing::error!(
                "restore: CRITICAL — rollback failed, active_profile may be orphaned: {rb}"
            );
            return Err(AppError::InternalError(
                "Restore failed and rollback incomplete — manual cleanup may be required".into(),
            ));
        }
        return Err(AppError::InternalError(
            "Failed to prepare server restart".into(),
        ));
    }

    // Step 4: Trigger graceful shutdown after a short delay (allows response to be sent).
    let shutdown = state.shutdown().clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown.cancel();
    });

    Ok(Json(RestoreResponse {
        success: true,
        message: "Shop data imported successfully. Server will restart.".into(),
    }))
}
