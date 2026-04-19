//! Pure-function profile listing for the admin surface.
//!
//! Returns the status of both profiles (Demo / Production) including
//! which is active, schema version, and database file size.

use kikan_types::admin::{ProfileInfo, ProfileListResponse};

use crate::db::diagnostics::read_db_runtime_diagnostics;
use crate::{ControlPlaneError, PlatformState, SetupMode};

/// List profiles with their status. Transport-neutral — no HTTP/session.
pub async fn list_profiles(
    state: &PlatformState,
) -> Result<ProfileListResponse, ControlPlaneError> {
    let active = *state.active_profile.read();

    let production_info = profile_info(state, SetupMode::Production, active).await?;
    let demo_info = profile_info(state, SetupMode::Demo, active).await?;

    Ok(ProfileListResponse {
        profiles: vec![production_info, demo_info],
        active,
    })
}

async fn profile_info(
    state: &PlatformState,
    mode: SetupMode,
    active: SetupMode,
) -> Result<ProfileInfo, ControlPlaneError> {
    let db = state.db_for(mode);
    let schema_version = match read_db_runtime_diagnostics(db).await {
        Ok(d) => d.schema_version,
        Err(e) => {
            tracing::warn!("could not read {mode} DB diagnostics: {e}");
            0
        }
    };

    let db_path = state.data_dir.join(mode.as_dir_name()).join("mokumo.db");
    let file_size_bytes = tokio::fs::metadata(&db_path).await.ok().map(|m| m.len());

    Ok(ProfileInfo {
        name: mode,
        active: mode == active,
        schema_version,
        file_size_bytes,
    })
}
