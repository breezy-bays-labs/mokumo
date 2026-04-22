//! Admin UDS router — Mokumo control-plane endpoints served over the
//! Unix socket with mode `0600` as the capability-based admin channel.
//!
//! The router combines:
//! - Liveness (`/health`)
//! - Mokumo-shaped diagnostics (`/diagnostics`, `/diagnostics/bundle`) —
//!   wire DTOs name `SetupMode` variants.
//! - Profile inventory + switch (`/profiles`, `/profiles/switch`) — wire
//!   DTOs name `SetupMode` variants.
//! - Migration status (`/migrate/status`) — kikan-generic.
//! - Backups (`/backups`, `/backups/create`) — wire DTO names
//!   `production` + `demo` fields.
//!
//! Filesystem permissions on the UDS (mode `0600`, owned by the server
//! user) are the sole access-control gate.

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use kikan::PlatformState;
use kikan_types::SetupMode;
use kikan_types::admin::{BackupCreateRequest, BackupCreatedResponse, ProfileSwitchAdminRequest};

pub fn build_admin_router(state: PlatformState) -> Router {
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
    crate::admin::diagnostics::collect(&state)
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
    let (bytes, filename) = crate::admin::diagnostics::build_bundle(&state)
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
    crate::admin::profile_list::list_profiles(&state)
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
    crate::admin::profile_switch::switch_profile_admin(&state, req.profile)
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
    // UDS response DTO names `production` / `demo` — kikan-types wire
    // shape. Collect per dir_name; the DTO field each entry lands in is
    // the corresponding wire name derived by string match.
    let mut production = kikan_types::ProfileBackups { backups: vec![] };
    let mut demo = kikan_types::ProfileBackups { backups: vec![] };
    for dir in state.profile_dir_names.iter() {
        let path = state.data_dir.join(dir.as_str()).join(state.db_filename);
        let entries = collect_profile_backups(&path).await;
        match dir.as_str() {
            "production" => production = entries,
            "demo" => demo = entries,
            _ => tracing::debug!(
                dir = dir.as_str(),
                "UDS backups_list: dir not represented in BackupStatusResponse DTO"
            ),
        }
    }
    Json(kikan_types::BackupStatusResponse { production, demo })
}

async fn backups_create(
    State(state): State<PlatformState>,
    Json(req): Json<BackupCreateRequest>,
) -> Result<Json<BackupCreatedResponse>, StatusCode> {
    let profile = req
        .profile
        .unwrap_or_else(|| default_profile_from_active(&state));
    let dir = profile.as_dir_name();
    let db_path = state.data_dir.join(dir).join(state.db_filename);

    let output_dir = state.data_dir.join(dir);
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

/// UDS wire-shape bridge: translate the active profile dir-name into
/// the `SetupMode` variant the admin DTO expects. Falls back to Demo
/// if the on-disk dir-name does not round-trip.
fn default_profile_from_active(state: &PlatformState) -> SetupMode {
    use std::str::FromStr;
    let active = state.active_profile.read();
    SetupMode::from_str(active.as_str()).unwrap_or(SetupMode::Demo)
}

async fn collect_profile_backups(db_path: &std::path::Path) -> kikan_types::ProfileBackups {
    let backups = match kikan::backup::collect_existing_backups(db_path).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(path = %db_path.display(), "backup scan failed: {e}");
            return kikan_types::ProfileBackups { backups: vec![] };
        }
    };

    let entries: Vec<kikan_types::BackupEntry> = backups
        .into_iter()
        .rev()
        .map(|(path, mtime)| {
            let version = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.rsplit_once(".backup-v"))
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
