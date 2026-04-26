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
        .route(
            "/diagnostics/sidecar-recoveries",
            get(sidecar_recoveries_list),
        )
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

async fn sidecar_recoveries_list(
    State(state): State<PlatformState>,
) -> Json<std::collections::HashMap<String, kikan::SidecarRecoveryDiagnostic>> {
    let map = state.sidecar_recoveries.read();
    Json(
        map.iter()
            .map(|(k, v)| (k.as_str().to_string(), v.clone()))
            .collect(),
    )
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
    crate::admin::migration_status::collect_migration_status(&state)
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
    Json(crate::admin::backup_status::collect(&state).await)
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
/// the `SetupMode` variant the admin DTO expects. `Engine::boot`
/// validated the round-trip for every declared kind, so a parse failure
/// here means the `active_profile` slot has drifted — logged as an
/// error before we fall back to `Demo` for the DTO.
fn default_profile_from_active(state: &PlatformState) -> SetupMode {
    use std::str::FromStr;
    let active = state.active_profile.read();
    match SetupMode::from_str(active.as_str()) {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(
                dir = active.as_str(),
                "admin UDS backup create: kikan-side active dir does not parse to SetupMode: {e}; \
                 defaulting profile to Demo"
            );
            SetupMode::Demo
        }
    }
}
