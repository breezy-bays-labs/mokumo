use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Json;
use axum::extract::{FromRequest, Multipart, Request, State};
use axum::http::header::CONTENT_TYPE;
use mokumo_db::restore::{self, RestoreError};
use mokumo_types::error::ErrorCode;
use mokumo_types::setup::{RestoreResponse, RestoreValidateResponse};
use serde::Deserialize;

use crate::SharedState;
use crate::error::AppError;

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
        if !state.is_first_launch.load(Ordering::Acquire) {
            return Err(AppError::Forbidden(
                ErrorCode::Forbidden,
                "Restore is only available on first launch.".into(),
            ));
        }

        // (b) Production DB must not already exist on disk.
        let prod_path = state.data_dir.join("production").join("mokumo.db");
        if prod_path.exists() {
            return Err(AppError::StateConflict(
                ErrorCode::ProductionDbExists,
                "A production database already exists.".into(),
            ));
        }

        // (c) At most one restore in flight at a time.
        if state
            .restore_in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(AppError::StateConflict(
                ErrorCode::RestoreInProgress,
                "Another restore operation is already in progress.".into(),
            ));
        }

        // Double-check after acquiring the flag (TOCTOU mitigation for (a)).
        if !state.is_first_launch.load(Ordering::Acquire) {
            state.restore_in_progress.store(false, Ordering::Release);
            return Err(AppError::Forbidden(
                ErrorCode::Forbidden,
                "Restore is only available on first launch.".into(),
            ));
        }

        Ok(Self {
            flag: state.restore_in_progress.clone(),
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
    // Rate limit before any I/O.
    if !state.restore_limiter.check_and_record("restore") {
        return Err(AppError::TooManyRequests(
            "Too many restore attempts. Please wait before trying again.".into(),
        ));
    }

    // Acquire guard before reading the body — avoids wasting I/O on rejected state.
    let _guard = RestoreGuard::acquire(&state)?;

    let content_type = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    let candidate = extract_candidate(&content_type, req, &state.data_dir, &state).await?;
    let file_name = candidate
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown.db".into());

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
    if !state.restore_limiter.check_and_record("restore") {
        return Err(AppError::TooManyRequests(
            "Too many restore attempts. Please wait before trying again.".into(),
        ));
    }

    // Hold the guard for the entire operation — prevents concurrent restores.
    let _guard = RestoreGuard::acquire(&state)?;

    let content_type = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    let candidate = extract_candidate(&content_type, req, &state.data_dir, &state).await?;
    let production_dir = state.data_dir.join("production");
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
    let profile_path = state.data_dir.join("active_profile");
    let profile_tmp = state.data_dir.join("active_profile.tmp");
    if let Err(e) = tokio::fs::write(&profile_tmp, "production").await {
        tracing::error!("restore: failed to write active_profile.tmp: {e}; rolling back");
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
    let sentinel = state.data_dir.join(".restart");
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
    let shutdown = state.shutdown.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown.cancel();
    });

    Ok(Json(RestoreResponse {
        success: true,
        message: "Shop data imported successfully. Server will restart.".into(),
    }))
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use axum_test::TestServer;
    use kikan::SetupMode;
    use mokumo_types::error::{ErrorBody, ErrorCode};
    use serde_json::json;
    use tempfile::TempDir;

    use crate::{ServerConfig, build_app, ensure_data_dirs};

    // ── Test helper ────────────────────────────────────────────────────────────

    /// Shared setup state: TestServer + data_dir (kept alive by TempDir).
    struct RestoreTestServer {
        server: TestServer,
        data_dir: std::path::PathBuf,
        _tmp: TempDir,
    }

    /// Build a first-launch server.
    ///
    /// - No `active_profile` file → `is_first_launch = true`
    /// - Production DB is in-memory → `data_dir/production/mokumo.db` does not exist
    async fn first_launch_server() -> RestoreTestServer {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        ensure_data_dirs(&data_dir).unwrap();
        let recovery_dir = tmp.path().join("recovery");
        std::fs::create_dir_all(&recovery_dir).unwrap();

        // Demo DB at demo/mokumo.db (normal path).
        let demo_url = format!(
            "sqlite:{}?mode=rwc",
            data_dir.join("demo/mokumo.db").display()
        );
        let demo_db = mokumo_db::initialize_database(&demo_url).await.unwrap();

        // Production DB is in-memory so no file is created at production/mokumo.db.
        let prod_db = mokumo_db::initialize_database("sqlite::memory:?cache=shared")
            .await
            .unwrap();

        let config = ServerConfig {
            port: 0,
            host: "127.0.0.1".into(),
            data_dir: data_dir.clone(),
            recovery_dir,
            #[cfg(debug_assertions)]
            ws_ping_ms: None,
        };

        // No active_profile file → is_first_launch = true.
        let (app, _): (axum::Router, Option<String>) =
            build_app(&config, demo_db, prod_db, SetupMode::Demo)
                .await
                .unwrap();
        let server = TestServer::new(app);

        RestoreTestServer {
            server,
            data_dir,
            _tmp: tmp,
        }
    }

    /// Build a non-first-launch server (active_profile file exists).
    async fn non_first_launch_server() -> RestoreTestServer {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        ensure_data_dirs(&data_dir).unwrap();
        let recovery_dir = tmp.path().join("recovery");
        std::fs::create_dir_all(&recovery_dir).unwrap();

        // Create active_profile → is_first_launch = false.
        std::fs::write(data_dir.join("active_profile"), "demo").unwrap();

        let demo_url = format!(
            "sqlite:{}?mode=rwc",
            data_dir.join("demo/mokumo.db").display()
        );
        let demo_db = mokumo_db::initialize_database(&demo_url).await.unwrap();
        let prod_db = mokumo_db::initialize_database("sqlite::memory:?cache=shared")
            .await
            .unwrap();

        let config = ServerConfig {
            port: 0,
            host: "127.0.0.1".into(),
            data_dir: data_dir.clone(),
            recovery_dir,
            #[cfg(debug_assertions)]
            ws_ping_ms: None,
        };

        let (app, _): (axum::Router, Option<String>) =
            build_app(&config, demo_db, prod_db, SetupMode::Demo)
                .await
                .unwrap();
        let server = TestServer::new(app);

        RestoreTestServer {
            server,
            data_dir,
            _tmp: tmp,
        }
    }

    /// Create a minimal valid Mokumo SQLite database in a temp directory.
    fn make_valid_db(dir: &TempDir) -> std::path::PathBuf {
        let path = dir.path().join("valid.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&format!(
            "PRAGMA application_id = {};
             CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL);
             INSERT INTO seaql_migrations VALUES ('m20260404_000000_set_pragmas', 0);",
            mokumo_db::MOKUMO_APPLICATION_ID
        ))
        .unwrap();
        drop(conn);
        path
    }

    /// Create a non-Mokumo (garbage) file.
    fn make_garbage_file(dir: &TempDir) -> std::path::PathBuf {
        let path = dir.path().join("garbage.db");
        std::fs::write(&path, b"this is not sqlite at all").unwrap();
        path
    }

    // ── RestoreGuard tests ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn validate_returns_403_forbidden_when_not_first_launch() {
        let ctx = non_first_launch_server().await;
        let response = ctx
            .server
            .post("/api/shop/restore/validate")
            .content_type("application/json")
            .json(&json!({"path": "/some/path.db"}))
            .await;

        assert_eq!(response.status_code(), 403);
        let body: ErrorBody = response.json();
        assert_eq!(body.code, ErrorCode::Forbidden);
    }

    #[tokio::test]
    async fn restore_returns_403_forbidden_when_not_first_launch() {
        let ctx = non_first_launch_server().await;
        let response = ctx
            .server
            .post("/api/shop/restore")
            .content_type("application/json")
            .json(&json!({"path": "/some/path.db"}))
            .await;

        assert_eq!(response.status_code(), 403);
        let body: ErrorBody = response.json();
        assert_eq!(body.code, ErrorCode::Forbidden);
    }

    #[tokio::test]
    async fn validate_returns_409_when_production_db_file_exists_on_disk() {
        let ctx = first_launch_server().await;
        // Create the production DB file on disk to trigger the guard.
        std::fs::write(ctx.data_dir.join("production/mokumo.db"), b"dummy").unwrap();

        let response = ctx
            .server
            .post("/api/shop/restore/validate")
            .content_type("application/json")
            .json(&json!({"path": "/some/path.db"}))
            .await;

        assert_eq!(response.status_code(), 409);
        let body: ErrorBody = response.json();
        assert_eq!(body.code, ErrorCode::ProductionDbExists);
    }

    #[tokio::test]
    async fn restore_returns_409_when_production_db_file_exists_on_disk() {
        let ctx = first_launch_server().await;
        std::fs::write(ctx.data_dir.join("production/mokumo.db"), b"dummy").unwrap();

        let response = ctx
            .server
            .post("/api/shop/restore")
            .content_type("application/json")
            .json(&json!({"path": "/some/path.db"}))
            .await;

        assert_eq!(response.status_code(), 409);
        let body: ErrorBody = response.json();
        assert_eq!(body.code, ErrorCode::ProductionDbExists);
    }

    // ── Content-Type tests ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn validate_returns_400_for_bad_content_type() {
        let ctx = first_launch_server().await;
        let response = ctx
            .server
            .post("/api/shop/restore/validate")
            .content_type("text/plain")
            .bytes(b"hello".as_ref().into())
            .await;

        assert_eq!(response.status_code(), 400);
        let body: ErrorBody = response.json();
        assert_eq!(body.code, ErrorCode::ParseError);
    }

    #[tokio::test]
    async fn restore_returns_400_for_bad_content_type() {
        let ctx = first_launch_server().await;
        let response = ctx
            .server
            .post("/api/shop/restore")
            .content_type("text/plain")
            .bytes(b"hello".as_ref().into())
            .await;

        assert_eq!(response.status_code(), 400);
        let body: ErrorBody = response.json();
        assert_eq!(body.code, ErrorCode::ParseError);
    }

    // ── Validate endpoint ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn validate_returns_200_for_valid_mokumo_db() {
        let ctx = first_launch_server().await;
        let db_dir = tempfile::tempdir().unwrap();
        let db_path = make_valid_db(&db_dir);

        let response = ctx
            .server
            .post("/api/shop/restore/validate")
            .content_type("application/json")
            .json(&json!({"path": db_path.to_str().unwrap()}))
            .await;

        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert!(body["file_name"].is_string());
        assert!(body["file_size"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn validate_returns_422_for_invalid_db() {
        let ctx = first_launch_server().await;
        let db_dir = tempfile::tempdir().unwrap();
        let garbage_path = make_garbage_file(&db_dir);

        let response = ctx
            .server
            .post("/api/shop/restore/validate")
            .content_type("application/json")
            .json(&json!({"path": garbage_path.to_str().unwrap()}))
            .await;

        assert_eq!(response.status_code(), 422);
        let body: ErrorBody = response.json();
        assert_eq!(body.code, ErrorCode::NotMokumoDatabase);
    }

    // ── Restore endpoint ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn restore_writes_production_db_and_active_profile_and_sentinel() {
        let ctx = first_launch_server().await;
        let db_dir = tempfile::tempdir().unwrap();
        let db_path = make_valid_db(&db_dir);

        let response = ctx
            .server
            .post("/api/shop/restore")
            .content_type("application/json")
            .json(&json!({"path": db_path.to_str().unwrap()}))
            .await;

        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert_eq!(body["success"], true);

        // Production DB must exist at the expected path.
        assert!(
            ctx.data_dir.join("production/mokumo.db").exists(),
            "production/mokumo.db should exist after restore"
        );
        // active_profile must be written.
        let profile = std::fs::read_to_string(ctx.data_dir.join("active_profile")).unwrap();
        assert_eq!(profile, "production");
        // Restart sentinel must be written.
        assert!(
            ctx.data_dir.join(".restart").exists(),
            ".restart sentinel should exist after restore"
        );
    }

    // ── Rate limit ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn rate_limit_returns_429_after_five_attempts() {
        // Use non-first-launch so each request is rejected quickly (403 before DB I/O)
        // but still counts against the shared restore rate limiter.
        let ctx = non_first_launch_server().await;

        // 3 validate + 2 restore = 5 attempts (shared bucket).
        for _ in 0..3 {
            ctx.server
                .post("/api/shop/restore/validate")
                .content_type("application/json")
                .json(&json!({"path": "/x.db"}))
                .await;
        }
        for _ in 0..2 {
            ctx.server
                .post("/api/shop/restore")
                .content_type("application/json")
                .json(&json!({"path": "/x.db"}))
                .await;
        }

        // 6th attempt must be rate-limited.
        let response = ctx
            .server
            .post("/api/shop/restore/validate")
            .content_type("application/json")
            .json(&json!({"path": "/x.db"}))
            .await;

        assert_eq!(response.status_code(), 429);
        let body: ErrorBody = response.json();
        assert_eq!(body.code, ErrorCode::RateLimited);
    }
}
