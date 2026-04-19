//! Admin UDS router — control-plane endpoints served over the Unix socket.
//!
//! ## Security model
//!
//! No session middleware, no auth layer. The Unix socket's fs-permissions
//! (mode 0600) are the sole access-control gate. Only the owning user
//! can connect.
//!
//! ## Endpoints
//!
//! - `GET  /health`              — liveness probe
//! - `GET  /diagnostics`         — structured diagnostics snapshot
//! - `GET  /diagnostics/bundle`  — zip export
//! - `GET  /profiles`            — list profiles with status
//! - `POST /profiles/switch`     — switch active profile
//! - `GET  /migrate/status`      — applied migration list per profile
//! - `GET  /backups`             — list pre-migration backup files
//! - `POST /backups/create`      — create a database backup

use std::path::Path;

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use kikan::PlatformState;
use kikan_types::admin::{BackupCreateRequest, BackupCreatedResponse, ProfileSwitchAdminRequest};

/// Build the admin router for the Unix domain socket surface.
///
/// Takes `PlatformState` — the narrowest slice that covers all
/// control-plane pure fns needed by the admin CLI. No session layer,
/// no auth layer, no SPA fallback.
pub fn build_admin_uds_router(state: PlatformState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/diagnostics", get(diagnostics))
        .route("/diagnostics/bundle", get(diagnostics_bundle))
        .route("/profiles", get(profiles_list))
        .route("/profiles/switch", post(profiles_switch))
        .route("/migrate/status", get(migrate_status))
        .route("/backups", get(backups_list))
        .route("/backups/create", post(backups_create))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn diagnostics(
    State(state): State<PlatformState>,
) -> Result<Json<kikan_types::diagnostics::DiagnosticsResponse>, StatusCode> {
    kikan::control_plane::diagnostics::collect(&state)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("admin UDS diagnostics failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn diagnostics_bundle(
    State(state): State<PlatformState>,
) -> Result<impl IntoResponse, StatusCode> {
    let (bytes, filename) = kikan::control_plane::diagnostics::build_bundle(&state)
        .await
        .map_err(|e| {
            tracing::error!("admin UDS diagnostics bundle failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let headers = [
        (header::CONTENT_TYPE, "application/zip".to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        ),
    ];
    Ok((headers, bytes))
}

async fn profiles_list(
    State(state): State<PlatformState>,
) -> Result<Json<kikan_types::admin::ProfileListResponse>, StatusCode> {
    kikan::control_plane::profile_list::list_profiles(&state)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("admin UDS profiles list failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn profiles_switch(
    State(state): State<PlatformState>,
    Json(req): Json<ProfileSwitchAdminRequest>,
) -> Result<Json<kikan_types::admin::ProfileSwitchAdminResponse>, StatusCode> {
    kikan::control_plane::profiles::switch_profile_admin(&state, req.profile)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("admin UDS profile switch failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn migrate_status(
    State(state): State<PlatformState>,
) -> Result<Json<kikan_types::admin::MigrationStatusResponse>, StatusCode> {
    kikan::control_plane::migration_status::collect_migration_status(&state)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("admin UDS migration status failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn backups_list(
    State(state): State<PlatformState>,
) -> Json<kikan_types::BackupStatusResponse> {
    // Reuse the same collection logic as the platform backup-status endpoint.
    let production =
        collect_profile_backups(&state.data_dir.join("production").join("mokumo.db")).await;
    let demo = collect_profile_backups(&state.data_dir.join("demo").join("mokumo.db")).await;
    Json(kikan_types::BackupStatusResponse { production, demo })
}

async fn backups_create(
    State(state): State<PlatformState>,
    Json(req): Json<BackupCreateRequest>,
) -> Result<Json<BackupCreatedResponse>, StatusCode> {
    let profile = req.profile.unwrap_or(*state.active_profile.read());
    let db_path = state.data_dir.join(profile.as_dir_name()).join("mokumo.db");

    let output_dir = state.data_dir.join(profile.as_dir_name());
    let output_name = kikan::backup::build_timestamped_name();
    let output_path = output_dir.join(&output_name);

    let db_path_clone = db_path.clone();
    let output_path_clone = output_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        kikan::backup::create_backup(&db_path_clone, &output_path_clone)
    })
    .await
    .map_err(|e| {
        tracing::error!("backup task panicked: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .map_err(|e| {
        tracing::error!("admin UDS backup create failed: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(BackupCreatedResponse {
        path: result.path.display().to_string(),
        size: result.size,
        profile,
    }))
}

/// Collect backup entries for a single profile's database.
async fn collect_profile_backups(db_path: &Path) -> kikan_types::ProfileBackups {
    let backups = match kikan::backup::collect_existing_backups(db_path).await {
        Ok(b) => b,
        Err(_) => return kikan_types::ProfileBackups { backups: vec![] },
    };

    let entries: Vec<kikan_types::BackupEntry> = backups
        .into_iter()
        .rev()
        .map(|(path, mtime)| {
            let version = path
                .to_str()
                .and_then(|s| s.rsplit_once(".backup-v"))
                .map(|(_, v)| v.to_owned())
                .unwrap_or_default();
            let backed_up_at = {
                use chrono::{DateTime, Utc};
                DateTime::<Utc>::from(mtime).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            };
            kikan_types::BackupEntry {
                path: path.display().to_string(),
                version,
                backed_up_at,
            }
        })
        .collect();

    kikan_types::ProfileBackups { backups: entries }
}
