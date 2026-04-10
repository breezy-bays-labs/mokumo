use std::path::Path;

use axum::{Json, extract::State};
use mokumo_core::setup::SetupMode;
use mokumo_db::DatabaseConnection;
use mokumo_types::diagnostics::{
    AppDiagnostics, DatabaseDiagnostics, DiagnosticsResponse, OsDiagnostics, ProfileDbDiagnostics,
    RuntimeDiagnostics,
};

use crate::{SharedState, error::AppError};

pub async fn handler(
    State(state): State<SharedState>,
) -> Result<
    (
        [(axum::http::HeaderName, &'static str); 1],
        Json<DiagnosticsResponse>,
    ),
    AppError,
> {
    let production_db_path = profile_db_path(&state.data_dir, SetupMode::Production);
    let demo_db_path = profile_db_path(&state.data_dir, SetupMode::Demo);

    let production = read_profile_diagnostics(&state.production_db, &production_db_path).await?;
    let demo = read_profile_diagnostics(&state.demo_db, &demo_db_path).await?;

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
        active_profile: *state.active_profile.read(),
        setup_complete: state.is_setup_complete(),
        is_first_launch: state
            .is_first_launch
            .load(std::sync::atomic::Ordering::Acquire),
        mdns_active: mdns.active,
        lan_url,
        host,
        port: mdns.port,
    };

    let response = DiagnosticsResponse {
        app: AppDiagnostics {
            name: env!("CARGO_PKG_NAME").into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        database: DatabaseDiagnostics { production, demo },
        runtime,
        os: OsDiagnostics {
            family: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        },
    };

    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(response),
    ))
}

fn profile_db_path(data_dir: &Path, mode: SetupMode) -> std::path::PathBuf {
    data_dir.join(mode.as_dir_name()).join("mokumo.db")
}

async fn read_profile_diagnostics(
    db: &DatabaseConnection,
    db_path: &Path,
) -> Result<ProfileDbDiagnostics, AppError> {
    let rt = mokumo_db::read_db_runtime_diagnostics(db).await?;
    let file_size_bytes = std::fs::metadata(db_path).ok().map(|m| m.len());
    Ok(ProfileDbDiagnostics {
        schema_version: rt.schema_version,
        file_size_bytes,
        wal_mode: rt.wal_mode,
    })
}
