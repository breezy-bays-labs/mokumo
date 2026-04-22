//! Profile-listing wire-DTO builder for Mokumo's admin surface.
//!
//! Returns the status of each profile the graft declared — active flag,
//! schema version, database file size — keyed to Mokumo's `SetupMode`
//! wire variants. kikan itself never names `SetupMode`; this module
//! bridges the graft's profile-dir-name strings into the typed
//! `ProfileListResponse` DTO consumed by the admin client.

use std::str::FromStr;

use kikan::db::diagnostics::read_db_runtime_diagnostics;
use kikan::{ControlPlaneError, PlatformState};
use kikan_types::SetupMode;
use kikan_types::admin::{ProfileInfo, ProfileListResponse};

/// List profiles with their status. Transport-neutral — no HTTP/session.
pub async fn list_profiles(
    state: &PlatformState,
) -> Result<ProfileListResponse, ControlPlaneError> {
    let active_dir = state.active_profile.read().clone();
    let active = SetupMode::from_str(active_dir.as_str()).unwrap_or(SetupMode::Demo);

    let mut profiles = Vec::with_capacity(state.profile_dir_names.len());
    for dir in state.profile_dir_names.iter() {
        let mode = match SetupMode::from_str(dir.as_str()) {
            Ok(m) => m,
            Err(_) => {
                tracing::debug!(
                    dir = dir.as_str(),
                    "profile dir does not round-trip to SetupMode wire shape; skipping from ProfileListResponse"
                );
                continue;
            }
        };
        profiles.push(profile_info(state, dir.as_str(), mode, active).await?);
    }
    // Preserve legacy order: production first, then demo.
    profiles.sort_by_key(|p| match p.name {
        SetupMode::Production => 0,
        SetupMode::Demo => 1,
    });

    Ok(ProfileListResponse { profiles, active })
}

async fn profile_info(
    state: &PlatformState,
    dir_name: &str,
    mode: SetupMode,
    active: SetupMode,
) -> Result<ProfileInfo, ControlPlaneError> {
    let db = state.db_for(dir_name).ok_or_else(|| {
        ControlPlaneError::Internal(anyhow::anyhow!(
            "profile pool missing for dir {dir_name} in PlatformState"
        ))
    })?;
    let schema_version = match read_db_runtime_diagnostics(db).await {
        Ok(d) => d.schema_version,
        Err(e) => {
            tracing::warn!("could not read {dir_name} DB diagnostics: {e}");
            0
        }
    };

    let db_path = state.data_dir.join(dir_name).join(state.db_filename);
    let file_size_bytes = tokio::fs::metadata(&db_path).await.ok().map(|m| m.len());

    Ok(ProfileInfo {
        name: mode,
        active: mode == active,
        schema_version,
        file_size_bytes,
    })
}
