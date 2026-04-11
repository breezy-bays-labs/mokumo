use std::path::Path;

use axum::{Json, extract::State};
use mokumo_core::setup::SetupMode;
use mokumo_db::DatabaseConnection;
use mokumo_types::diagnostics::{
    AppDiagnostics, DatabaseDiagnostics, DiagnosticsResponse, OsDiagnostics, ProfileDbDiagnostics,
    RuntimeDiagnostics, SystemDiagnostics,
};
use sysinfo::{Disks, System};

use crate::{SharedState, error::AppError};

pub async fn handler(
    State(state): State<SharedState>,
) -> Result<Json<DiagnosticsResponse>, AppError> {
    Ok(Json(collect(&state).await?))
}

/// Collect the full diagnostics snapshot. Called by both the diagnostics handler
/// and the bundle export handler so sysinfo is only queried in one place.
pub async fn collect(state: &SharedState) -> Result<DiagnosticsResponse, AppError> {
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

    // System facts — sysinfo refresh (fast kernel stat calls, acceptable on a non-hot endpoint)
    let system = collect_system_diagnostics(&state.data_dir);

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

fn collect_system_diagnostics(data_dir: &Path) -> SystemDiagnostics {
    let mut sys = System::new();
    sys.refresh_memory();

    let hostname = System::host_name();

    // Find the disk volume whose mount point is the longest prefix of data_dir.
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
    }
}

fn profile_db_path(data_dir: &Path, mode: SetupMode) -> std::path::PathBuf {
    data_dir.join(mode.as_dir_name()).join("mokumo.db")
}

async fn read_profile_diagnostics(
    db: &DatabaseConnection,
    db_path: &Path,
) -> Result<ProfileDbDiagnostics, AppError> {
    let rt = mokumo_db::read_db_runtime_diagnostics(db).await?;
    let file_size_bytes = tokio::fs::metadata(db_path).await.ok().map(|m| m.len());
    Ok(ProfileDbDiagnostics {
        schema_version: rt.schema_version,
        file_size_bytes,
        wal_mode: rt.wal_mode,
    })
}
